//! StateSource implementation for lazy Levenshtein WFST composition.
//!
//! This module provides [`LevenshteinStateSource`], which implements lling-llang's
//! [`StateSource`] trait for on-demand computation of Levenshtein transducer states.
//!
//! # UTF-8 Support
//!
//! This implementation uses character-level edit distance, properly handling
//! multi-byte UTF-8 characters. Each character (not byte) counts as one unit
//! for edit distance calculation.

use std::sync::Arc;

use lling_llang::prelude::{
    LazyState, Semiring, StateId, StateSource, TropicalWeight, WeightedTransition,
};
use smallvec::SmallVec;

use libdictenstein::{Dictionary, DictionaryNode};
use liblevenshtein::transducer::Algorithm;

use crate::node_key::DictionaryNodeKey;
use crate::node_registry::DictionaryNodeRegistry;
#[cfg(test)]
use crate::state_source_support::saturating_u32;
use crate::state_source_support::{
    bounded_count_to_f64, next_index, node_has_any_edge, node_has_char_edge, remaining_query_chars,
    usize_from_u32, within_max_distance, AutomatonState, LevenshteinStateCodec,
};

/// State source for lazy Levenshtein WFST computation.
///
/// This implements lling-llang's [`StateSource`] trait, enabling lazy composition
/// of Levenshtein transducers with other WFSTs. States are computed on-demand
/// as the composed transducer is traversed.
///
/// # Product State Representation
///
/// Each WFST state represents a pair `(dictionary_node, automaton_state)`:
/// - `dictionary_node`: Position in the dictionary trie
/// - `automaton_state`: Position in the Levenshtein automaton
///
/// These are encoded into a single `StateId` for the WFST interface.
///
/// # Example
///
/// ```rust,no_run
/// use duallity::LevenshteinStateSource;
/// use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
/// use lling_llang::prelude::*;
///
/// let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help"]);
/// let source = LevenshteinStateSource::new(&dict, "helo", 2);
///
/// // Wrap in LazyWfstWrapper for composition
/// let lazy_wfst = LazyWfstWrapper::new(source);
/// ```
#[derive(Clone)]
pub struct LevenshteinStateSource<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
{
    /// The dictionary to search
    dictionary: D,
    /// Original query text for allocation-free accessors.
    query_text: Arc<str>,
    /// Query as characters (UTF-8 aware)
    query_chars: Arc<Vec<char>>,
    /// Automaton and product-state encoding metadata.
    codec: LevenshteinStateCodec,
    /// Node registry: maps exact path keys to node IDs
    node_registry: Arc<std::sync::RwLock<DictionaryNodeRegistry<D::Node>>>,
}

impl<D> LevenshteinStateSource<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
{
    /// Create a new state source for the given dictionary and query.
    ///
    /// # Arguments
    ///
    /// - `dictionary`: The dictionary to search
    /// - `query`: The query string to find corrections for
    /// - `max_distance`: Maximum edit distance for matches
    ///
    /// # UTF-8 Support
    ///
    /// The query string is properly handled as UTF-8 characters. Each Unicode
    /// character (not byte) counts as one unit for edit distance calculation.
    pub fn new(dictionary: &D, query: &str, max_distance: usize) -> Self {
        Self::with_algorithm(dictionary, query, max_distance, Algorithm::Standard)
    }

    /// Create a new state source with a specific algorithm.
    pub fn with_algorithm(
        dictionary: &D,
        query: &str,
        max_distance: usize,
        algorithm: Algorithm,
    ) -> Self {
        let query_text = Arc::<str>::from(query);
        let query_chars: Vec<char> = query_text.chars().collect();
        let codec = LevenshteinStateCodec::new(query_chars.len(), max_distance, algorithm);

        let root = dictionary.root();
        let registry = DictionaryNodeRegistry::new(root);

        Self {
            dictionary: dictionary.clone(),
            query_text,
            query_chars: Arc::new(query_chars),
            codec,
            node_registry: Arc::new(std::sync::RwLock::new(registry)),
        }
    }

    /// Borrow the original query string without reconstructing it from characters.
    pub fn query_str(&self) -> &str {
        &self.query_text
    }

    /// Get the query string.
    pub fn query(&self) -> String {
        self.query_str().to_owned()
    }

    pub(crate) fn is_valid_product_state(&self, state: StateId) -> bool {
        let Some((dict_node_id, automaton_state_id)) = self.codec.decode_product_state(state)
        else {
            return false;
        };

        if self
            .codec
            .decode_automaton_state(automaton_state_id)
            .is_none()
        {
            return false;
        }

        let registry = crate::read_lock(&self.node_registry);
        registry.get_node(dict_node_id).is_some()
    }

    pub(crate) fn final_weight_for_state(&self, state: StateId) -> Option<TropicalWeight> {
        let (dict_node_id, automaton_state_id) = self.codec.decode_product_state(state)?;
        let AutomatonState::Normal {
            query_pos,
            edit_cost,
        } = self.codec.decode_automaton_state(automaton_state_id)?
        else {
            return None;
        };

        let registry = crate::read_lock(&self.node_registry);
        let dict_node = registry.get_node(dict_node_id)?;
        self.normal_final_weight(dict_node, query_pos, edit_cost)
    }

    pub(crate) fn registered_state_id_span(&self) -> usize {
        let automaton_states = usize_from_u32(self.codec.max_automaton_states());
        let node_registry = crate::read_lock(&self.node_registry);
        crate::registered_product_state_id_span(
            node_registry.len(),
            automaton_states,
            automaton_states,
        )
    }

    /// Compute transitions for a product state.
    ///
    /// This explores the dictionary edges and computes character-level
    /// edit distance transitions.
    ///
    /// # State Encoding
    ///
    /// The normal `automaton_state_id` range encodes `(query_position,
    /// edit_cost)`. Transposition-capable algorithms reserve a second range for
    /// continuation states that must emit the second half of one adjacent swap.
    ///
    /// # Edit Operations (character-level, UTF-8 aware)
    ///
    /// - **Match**: query[pos] == dict_char, advance pos, cost 0
    /// - **Substitute**: query[pos] != dict_char, advance pos, cost 1
    /// - **Insert**: consume dict_char without advancing pos, cost 1
    /// - **Delete**: advance pos without consuming dict_char (epsilon), cost 1
    /// - **Transpose**: consume two adjacent query characters in swapped
    ///   dictionary order, cost 1
    /// - **Merge**: consume two query characters as one dictionary character, cost 1
    /// - **Split**: consume one query character as two dictionary characters, cost 1
    fn compute_transitions(
        &self,
        dict_node_id: u32,
        automaton_state_id: u32,
    ) -> (
        bool,
        TropicalWeight,
        SmallVec<[WeightedTransition<char, TropicalWeight>; 4]>,
    ) {
        let automaton_state = match self.codec.decode_automaton_state(automaton_state_id) {
            Some(state) => state,
            None => return (false, TropicalWeight::zero(), SmallVec::new()),
        };

        let registry = crate::read_lock(&self.node_registry);

        let dict_node = match registry.get_node(dict_node_id) {
            Some(node) => node.clone(),
            None => {
                // Invalid node ID - return non-final with no transitions
                return (false, TropicalWeight::zero(), SmallVec::new());
            }
        };
        drop(registry); // Release read lock

        let Some(from_state) = self
            .codec
            .encode_product_state(dict_node_id, automaton_state_id)
        else {
            return (false, TropicalWeight::zero(), SmallVec::new());
        };

        match automaton_state {
            AutomatonState::Normal {
                query_pos,
                edit_cost,
            } => self.compute_normal_transitions(
                dict_node_id,
                from_state,
                query_pos,
                edit_cost,
                &dict_node,
            ),
            AutomatonState::TransposeSecond {
                query_pos,
                edit_cost,
            } => self.compute_transpose_second_transitions(
                dict_node_id,
                from_state,
                query_pos,
                edit_cost,
                &dict_node,
            ),
            AutomatonState::MergeSecond {
                query_pos,
                edit_cost,
            } => self.compute_merge_second_transitions(
                dict_node_id,
                from_state,
                query_pos,
                edit_cost,
            ),
            AutomatonState::SplitSecond {
                query_pos,
                edit_cost,
            } => self.compute_split_second_transitions(
                dict_node_id,
                from_state,
                query_pos,
                edit_cost,
                &dict_node,
            ),
        }
    }

    fn compute_normal_transitions(
        &self,
        dict_node_id: u32,
        from_state: StateId,
        query_pos: u32,
        edit_cost: u32,
        dict_node: &D::Node,
    ) -> (
        bool,
        TropicalWeight,
        SmallVec<[WeightedTransition<char, TropicalWeight>; 4]>,
    ) {
        let query_len = self.query_chars.len();
        let pos = usize_from_u32(query_pos);
        let max_distance = self.codec.max_distance_u32();
        let can_edit = edit_cost < max_distance;
        let next_pos = next_index(pos, query_len);
        let can_read_query = pos < query_len;
        let algorithm = self.codec.algorithm();
        let supports_transposition = algorithm.supports_transposition();
        let supports_merge_split = algorithm.supports_merge_split();
        let next_edit_cost = edit_cost.saturating_add(1);
        let mut transitions_per_edge = usize::from(can_read_query);
        if can_edit {
            transitions_per_edge = transitions_per_edge.saturating_add(1);
            if supports_transposition && next_pos.is_some() {
                transitions_per_edge = transitions_per_edge.saturating_add(1);
            }
            if supports_merge_split {
                transitions_per_edge = transitions_per_edge
                    .saturating_add(usize::from(next_pos.is_some()))
                    .saturating_add(usize::from(can_read_query));
            }
        }
        let extra_transitions = usize::from(can_read_query && can_edit);
        let mut transitions =
            crate::dictionary_edge_capacity(dict_node, transitions_per_edge, extra_transitions)
                .map_or_else(SmallVec::new, SmallVec::with_capacity);

        // Iterate over dictionary edges only when at least one child-consuming
        // edit operation is legal from this automaton state.
        if transitions_per_edge > 0 {
            let query_char = can_read_query.then(|| self.query_chars[pos]);
            let next_query_pos = query_pos.checked_add(1);
            let insert_automaton_state = if can_edit {
                self.codec.encode_automaton_state(query_pos, next_edit_cost)
            } else {
                None
            };
            let transpose_context = if supports_transposition && can_edit {
                next_pos.and_then(|next_pos| {
                    let first_query_char = self.query_chars[pos];
                    let second_query_char = self.query_chars[next_pos];
                    self.codec
                        .encode_transpose_second_state(query_pos, next_edit_cost)
                        .map(|state| (first_query_char, second_query_char, state))
                })
            } else {
                None
            };
            let merge_context = if supports_merge_split && can_edit {
                next_pos.and_then(|_| {
                    query_char.zip(
                        self.codec
                            .encode_merge_second_state(query_pos, next_edit_cost),
                    )
                })
            } else {
                None
            };
            let split_context = if supports_merge_split && can_edit {
                query_char.zip(
                    self.codec
                        .encode_split_second_state(query_pos, next_edit_cost),
                )
            } else {
                None
            };

            let mut registry = crate::write_lock(&self.node_registry);

            for (unit, child_node) in dict_node.edges() {
                let dict_char: char = unit.into();

                let match_or_substitute = if let Some(query_char) = query_char {
                    let operation_cost = if query_char == dict_char { 0 } else { 1 };
                    let next_cost = edit_cost.saturating_add(operation_cost);

                    (next_cost <= max_distance)
                        .then(|| {
                            next_query_pos
                                .and_then(|next_query_pos| {
                                    self.codec.encode_automaton_state(next_query_pos, next_cost)
                                })
                                .map(|target_state| (query_char, operation_cost, target_state))
                        })
                        .flatten()
                } else {
                    None
                };

                let transpose_transition = match transpose_context {
                    Some((first_query_char, second_query_char, state))
                        if dict_char == second_query_char
                            && first_query_char != second_query_char
                            && node_has_char_edge(&child_node, first_query_char) =>
                    {
                        Some((first_query_char, state))
                    }
                    _ => None,
                };

                let merge_transition = merge_context;
                let split_transition = if split_context.is_some() && node_has_any_edge(&child_node)
                {
                    split_context
                } else {
                    None
                };

                let transpose_automaton_state = transpose_transition.map(|(_, state)| state);
                let merge_automaton_state = merge_transition.map(|(_, state)| state);
                let split_automaton_state = split_transition.map(|(_, state)| state);

                if match_or_substitute.is_none()
                    && insert_automaton_state.is_none()
                    && transpose_automaton_state.is_none()
                    && merge_automaton_state.is_none()
                    && split_automaton_state.is_none()
                {
                    continue;
                }

                let target_automaton_states = [
                    match_or_substitute.map(|(_, _, state)| state),
                    insert_automaton_state,
                    transpose_automaton_state,
                    merge_automaton_state,
                    split_automaton_state,
                ];
                let Some(child_node_id) = self.register_dictionary_node_for_targets(
                    &mut registry,
                    dict_node_id,
                    dict_char,
                    child_node,
                    target_automaton_states.into_iter().flatten(),
                ) else {
                    continue;
                };

                if let Some((query_char, operation_cost, target_automaton_state)) =
                    match_or_substitute
                {
                    if let Some(target_state) = self
                        .codec
                        .encode_transition_target(child_node_id, Some(target_automaton_state))
                    {
                        transitions.push(WeightedTransition::new(
                            from_state,
                            Some(query_char),
                            Some(dict_char),
                            target_state,
                            TropicalWeight::new(f64::from(operation_cost)),
                        ));
                    }
                }

                if let Some(target_automaton_state) = insert_automaton_state {
                    if let Some(insert_target) = self
                        .codec
                        .encode_transition_target(child_node_id, Some(target_automaton_state))
                    {
                        transitions.push(WeightedTransition::new(
                            from_state,
                            None,
                            Some(dict_char),
                            insert_target,
                            TropicalWeight::new(1.0),
                        ));
                    }
                }

                if let Some((first_query_char, target_automaton_state)) = transpose_transition {
                    if let Some(target_state) = self
                        .codec
                        .encode_transition_target(child_node_id, Some(target_automaton_state))
                    {
                        transitions.push(WeightedTransition::new(
                            from_state,
                            Some(first_query_char),
                            Some(dict_char),
                            target_state,
                            TropicalWeight::new(1.0),
                        ));
                    }
                }

                if let Some((first_query_char, target_automaton_state)) = merge_transition {
                    if let Some(target_state) = self
                        .codec
                        .encode_transition_target(child_node_id, Some(target_automaton_state))
                    {
                        transitions.push(WeightedTransition::new(
                            from_state,
                            Some(first_query_char),
                            Some(dict_char),
                            target_state,
                            TropicalWeight::new(1.0),
                        ));
                    }
                }

                if let Some((first_query_char, target_automaton_state)) = split_transition {
                    if let Some(target_state) = self
                        .codec
                        .encode_transition_target(child_node_id, Some(target_automaton_state))
                    {
                        transitions.push(WeightedTransition::new(
                            from_state,
                            Some(first_query_char),
                            Some(dict_char),
                            target_state,
                            TropicalWeight::new(1.0),
                        ));
                    }
                }
            }
        }

        // Delete: consume query input without producing a dictionary character.
        // This represents a missing character in the dictionary term.
        if can_read_query && can_edit {
            let delete_target = self.codec.encode_transition_target(
                dict_node_id, // Stay at same dict node
                query_pos.checked_add(1).and_then(|next_pos| {
                    self.codec.encode_automaton_state(next_pos, next_edit_cost)
                }),
            );

            if let Some(delete_target) = delete_target {
                transitions.push(WeightedTransition::new(
                    from_state,
                    Some(self.query_chars[pos]),
                    None,
                    delete_target,
                    TropicalWeight::new(1.0),
                ));
            }
        }

        let accepted_final_weight = self.normal_final_weight(dict_node, query_pos, edit_cost);
        let is_final = accepted_final_weight.is_some();
        let final_weight = accepted_final_weight.unwrap_or_else(TropicalWeight::zero);

        (is_final, final_weight, transitions)
    }

    fn compute_transpose_second_transitions(
        &self,
        dict_node_id: u32,
        from_state: StateId,
        query_pos: u32,
        edit_cost: u32,
        dict_node: &D::Node,
    ) -> (
        bool,
        TropicalWeight,
        SmallVec<[WeightedTransition<char, TropicalWeight>; 4]>,
    ) {
        let pos = usize_from_u32(query_pos);
        let query_len = self.query_chars.len();

        let Some(next_pos) = next_index(pos, query_len) else {
            return (false, TropicalWeight::zero(), SmallVec::new());
        };

        let mut transitions = crate::dictionary_edge_capacity(dict_node, 1, 0)
            .map_or_else(SmallVec::new, SmallVec::with_capacity);
        let second_input = self.query_chars[next_pos];
        let required_output = self.query_chars[pos];
        let Some(target_automaton_state) = query_pos.checked_add(2).and_then(|next_query_pos| {
            self.codec.encode_automaton_state(next_query_pos, edit_cost)
        }) else {
            return (false, TropicalWeight::zero(), transitions);
        };

        let mut registry = crate::write_lock(&self.node_registry);
        for (unit, child_node) in dict_node.edges() {
            let dict_char: char = unit.into();

            if dict_char != required_output {
                continue;
            }

            let Some(child_node_id) = self.register_dictionary_node_for_targets(
                &mut registry,
                dict_node_id,
                dict_char,
                child_node,
                std::iter::once(target_automaton_state),
            ) else {
                continue;
            };

            let Some(target_state) = self
                .codec
                .encode_transition_target(child_node_id, Some(target_automaton_state))
            else {
                continue;
            };

            transitions.push(WeightedTransition::new(
                from_state,
                Some(second_input),
                Some(dict_char),
                target_state,
                TropicalWeight::new(0.0),
            ));
        }

        (false, TropicalWeight::zero(), transitions)
    }

    fn compute_merge_second_transitions(
        &self,
        dict_node_id: u32,
        from_state: StateId,
        query_pos: u32,
        edit_cost: u32,
    ) -> (
        bool,
        TropicalWeight,
        SmallVec<[WeightedTransition<char, TropicalWeight>; 4]>,
    ) {
        let mut transitions = SmallVec::with_capacity(1);
        let pos = usize_from_u32(query_pos);
        let query_len = self.query_chars.len();

        let Some(next_pos) = next_index(pos, query_len) else {
            return (false, TropicalWeight::zero(), transitions);
        };

        let Some(target_state) = self.codec.encode_transition_target(
            dict_node_id,
            query_pos.checked_add(2).and_then(|next_query_pos| {
                self.codec.encode_automaton_state(next_query_pos, edit_cost)
            }),
        ) else {
            return (false, TropicalWeight::zero(), transitions);
        };

        transitions.push(WeightedTransition::new(
            from_state,
            Some(self.query_chars[next_pos]),
            None,
            target_state,
            TropicalWeight::new(0.0),
        ));

        (false, TropicalWeight::zero(), transitions)
    }

    fn compute_split_second_transitions(
        &self,
        dict_node_id: u32,
        from_state: StateId,
        query_pos: u32,
        edit_cost: u32,
        dict_node: &D::Node,
    ) -> (
        bool,
        TropicalWeight,
        SmallVec<[WeightedTransition<char, TropicalWeight>; 4]>,
    ) {
        let pos = usize_from_u32(query_pos);
        let query_len = self.query_chars.len();

        if pos >= query_len {
            return (false, TropicalWeight::zero(), SmallVec::new());
        }

        let mut transitions = crate::dictionary_edge_capacity(dict_node, 1, 0)
            .map_or_else(SmallVec::new, SmallVec::with_capacity);
        let Some(target_automaton_state) = query_pos.checked_add(1).and_then(|next_query_pos| {
            self.codec.encode_automaton_state(next_query_pos, edit_cost)
        }) else {
            return (false, TropicalWeight::zero(), transitions);
        };

        let mut registry = crate::write_lock(&self.node_registry);
        for (unit, child_node) in dict_node.edges() {
            let dict_char: char = unit.into();
            let Some(child_node_id) = self.register_dictionary_node_for_targets(
                &mut registry,
                dict_node_id,
                dict_char,
                child_node,
                std::iter::once(target_automaton_state),
            ) else {
                continue;
            };

            let Some(target_state) = self
                .codec
                .encode_transition_target(child_node_id, Some(target_automaton_state))
            else {
                continue;
            };

            transitions.push(WeightedTransition::new(
                from_state,
                None,
                Some(dict_char),
                target_state,
                TropicalWeight::new(0.0),
            ));
        }

        (false, TropicalWeight::zero(), transitions)
    }

    fn normal_final_weight(
        &self,
        dict_node: &D::Node,
        query_pos: u32,
        edit_cost: u32,
    ) -> Option<TropicalWeight> {
        if !dict_node.is_final() {
            return None;
        }

        let remaining = remaining_query_chars(self.query_chars.len(), query_pos)?;
        within_max_distance(edit_cost, remaining, self.codec.max_distance())
            .then(|| bounded_count_to_f64(remaining).map(TropicalWeight::new))
            .flatten()
    }

    fn register_dictionary_node_for_targets<I>(
        &self,
        registry: &mut DictionaryNodeRegistry<D::Node>,
        parent_id: u32,
        edge_label: char,
        node: D::Node,
        target_automaton_states: I,
    ) -> Option<u32>
    where
        I: IntoIterator<Item = u32>,
    {
        let key = DictionaryNodeKey::child(parent_id, edge_label);
        let candidate_id = registry.candidate_id(key)?;
        if !target_automaton_states.into_iter().any(|automaton_state| {
            self.codec
                .encode_product_state(candidate_id, automaton_state)
                .is_some()
        }) {
            return None;
        }

        registry.register_node(node, key)
    }
}

impl<D> StateSource<char, TropicalWeight> for LevenshteinStateSource<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
{
    fn compute_state(&self, state: StateId) -> LazyState<char, TropicalWeight> {
        let Some((dict_node_id, automaton_state_id)) = self.codec.decode_product_state(state)
        else {
            return LazyState::non_final(SmallVec::new());
        };

        let (is_final, final_weight, transitions) =
            self.compute_transitions(dict_node_id, automaton_state_id);

        if is_final {
            LazyState::final_state(final_weight, transitions)
        } else {
            LazyState::non_final(transitions)
        }
    }

    fn start(&self) -> StateId {
        // Start state is (root_node=0, initial_automaton_state=0).
        0
    }

    fn num_states_hint(&self) -> Option<usize> {
        let automaton_states = usize_from_u32(self.codec.max_automaton_states());
        let node_registry = crate::read_lock(&self.node_registry);

        crate::registered_dictionary_product_states_hint(
            self.dictionary.len(),
            node_registry.len(),
            automaton_states,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libdictenstein::dynamic_dawg::char::DynamicDawgChar;

    #[test]
    fn test_saturating_u32_caps_unrepresentable_lengths() {
        assert_eq!(saturating_u32(0), 0);
        let Ok(max_u32) = usize::try_from(u32::MAX) else {
            return;
        };
        assert_eq!(saturating_u32(max_u32), u32::MAX);

        if let Some(above_max_u32) = max_u32.checked_add(1) {
            assert_eq!(saturating_u32(above_max_u32), u32::MAX);
        }
    }

    #[test]
    fn test_next_index_rejects_overflow_and_bounds() {
        assert_eq!(next_index(0, 2), Some(1));
        assert_eq!(next_index(0, 1), None);
        assert_eq!(next_index(usize::MAX, usize::MAX), None);
    }

    #[test]
    fn test_within_max_distance_checks_cost_sum_overflow() {
        assert!(within_max_distance(1, 2, 3));
        assert!(!within_max_distance(2, 2, 3));
        assert!(!within_max_distance(u32::MAX, usize::MAX, usize::MAX));
    }

    #[test]
    fn test_remaining_query_chars_uses_full_usize_length() {
        assert_eq!(remaining_query_chars(5, 2), Some(3));
        assert_eq!(remaining_query_chars(2, 5), None);

        let Ok(max_u32) = usize::try_from(u32::MAX) else {
            return;
        };
        let Some(huge_query_len) = max_u32.checked_add(10) else {
            return;
        };

        assert_eq!(remaining_query_chars(huge_query_len, u32::MAX), Some(10));
    }

    #[test]
    fn test_bounded_count_to_f64_rejects_unrepresentable_counts() {
        assert_eq!(bounded_count_to_f64(3), Some(3.0));

        let Ok(max_u32) = usize::try_from(u32::MAX) else {
            return;
        };
        assert_eq!(bounded_count_to_f64(max_u32), Some(f64::from(u32::MAX)));

        if let Some(count_after_u32) = max_u32.checked_add(1) {
            assert_eq!(
                bounded_count_to_f64(count_after_u32),
                Some(count_after_u32 as f64)
            );
        }

        let max_exact = crate::max_exact_f64_usize();
        assert_eq!(bounded_count_to_f64(max_exact), Some(max_exact as f64));

        if let Some(unrepresentable_count) = max_exact.checked_add(1) {
            assert_eq!(bounded_count_to_f64(unrepresentable_count), None);
        }
    }

    #[test]
    fn test_state_source_does_not_register_pruned_normal_edges_at_max_distance() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["b"]);
        let source = LevenshteinStateSource::new(&dict, "a", 0);

        assert_eq!(registered_dictionary_node_count(&source), 1);

        match source.compute_state(source.start()) {
            LazyState::Computed { transitions, .. } => {
                assert!(
                    !transitions
                        .iter()
                        .any(|transition| transition.output == Some('b')),
                    "mismatched dictionary edge must be pruned at distance zero"
                );
            }
            LazyState::Pending => {
                std::panic::panic_any("Levenshtein state source should compute eagerly".to_owned());
            }
        }

        assert_eq!(registered_dictionary_node_count(&source), 1);
    }

    #[test]
    fn test_state_source_does_not_register_unencodable_dictionary_edges() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["a"]);
        let mut source = LevenshteinStateSource::new(&dict, "a", 1);
        source.codec.set_max_automaton_states_for_testing(1);

        assert_eq!(registered_dictionary_node_count(&source), 1);

        match source.compute_state(source.start()) {
            LazyState::Computed { transitions, .. } => {
                assert!(transitions.is_empty());
            }
            LazyState::Pending => {
                std::panic::panic_any("Levenshtein state source should compute eagerly".to_owned());
            }
        }

        assert_eq!(registered_dictionary_node_count(&source), 1);
    }

    #[test]
    fn test_state_source_hint_uses_registered_node_lower_bound() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["abcd"]);
        let source = LevenshteinStateSource::new(&dict, "a", 0);

        let _ = source.compute_state(source.start());

        let registered_nodes = registered_dictionary_node_count(&source);
        let automaton_states = usize_from_u32(source.codec.max_automaton_states());
        let live_state_count = registered_nodes.saturating_mul(automaton_states);

        assert!(registered_nodes > dict.len().unwrap_or(0));
        assert_eq!(
            StateSource::<char, TropicalWeight>::num_states_hint(&source),
            Some(live_state_count)
        );
    }

    #[test]
    fn test_state_source_query_str_borrows_original_utf8_query() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["éclair"]);
        let source = LevenshteinStateSource::new(&dict, "éclair", 1);

        let first = source.query_str();
        let second = source.query_str();

        assert_eq!(first, "éclair");
        assert_eq!(source.query(), "éclair");
        assert!(std::ptr::eq(first.as_ptr(), second.as_ptr()));
    }

    fn registered_dictionary_node_count(
        source: &LevenshteinStateSource<DynamicDawgChar<()>>,
    ) -> usize {
        let registry = crate::read_lock(&source.node_registry);
        let mut count = 0usize;
        while registry.get_node(count as u32).is_some() {
            count += 1;
        }
        count
    }
}
