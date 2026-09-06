//! StateSource implementation for Universal Levenshtein WFST.
//!
//! This module provides [`UniversalLevenshteinStateSource`], which implements
//! lling-llang's [`StateSource`] trait using the Universal Levenshtein Automaton.
//!
//! # Key Differences from Parameterized Automaton
//!
//! The Universal Automaton is query-agnostic and can be precomputed once for a
//! given maximum edit distance. When bound to a query via [`BoundUniversalWfst`],
//! it encodes the dictionary terms as bit vectors and processes them through
//! the automaton.

use std::sync::Arc;

use lling_llang::prelude::{
    ExpansionFailure, ExpansionRequest, Semiring, StateExpansion, StateId, StateSource,
    TropicalWeight, WeightedTransition,
};
use smallvec::SmallVec;

use libdictenstein::{Dictionary, DictionaryNode};
use liblevenshtein::transducer::universal::{
    CharacteristicVector, PositionVariant, UniversalState,
};

use crate::node_key::DictionaryNodeKey;
use crate::node_registry::DepthDictionaryNodeRegistry;
use crate::state_encoding;
use crate::universal_state_support::{
    precompute_relevant_subwords, universal_accepting_weight, universal_query_state_factor,
    universal_state_key, UniversalQueryWindow, UniversalStateRegistry,
};
use crate::{fulfill_expansion_request, DirectStateSource};

/// State source for Universal Levenshtein WFST computation.
///
/// This implements lling-llang's [`StateSource`] trait using the Universal
/// Levenshtein Automaton. States are computed on-demand as the WFST is traversed.
///
/// # Product State Representation
///
/// Each WFST state represents a pair `(dictionary_node_id, automaton_state_id)`:
/// - `dictionary_node_id`: Position in the dictionary trie
/// - `automaton_state_id`: ID of the universal automaton state and exact
///   query-label cursor in the registry
#[derive(Clone)]
pub struct UniversalLevenshteinStateSource<V, D>
where
    V: PositionVariant + Clone + Send + Sync,
    V::State: Send + Sync,
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
{
    /// The dictionary to search
    dictionary: D,
    /// Original query text for allocation-free accessors.
    query_text: Arc<str>,
    /// Query as characters (for computing bit vectors)
    query_chars: Arc<Vec<char>>,
    /// Precomputed relevant query windows indexed by dictionary depth.
    relevant_subwords: Arc<Vec<UniversalQueryWindow>>,
    /// State registry for deduplication
    state_registry: Arc<std::sync::RwLock<UniversalStateRegistry<V>>>,
    /// Node registry for dictionary nodes
    node_registry: Arc<std::sync::RwLock<DepthDictionaryNodeRegistry<D::Node>>>,
    /// Maximum automaton states for encoding
    max_automaton_states: u32,
}

impl<V, D> UniversalLevenshteinStateSource<V, D>
where
    V: PositionVariant + Clone + Send + Sync,
    V::State: Send + Sync,
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
    pub fn new(dictionary: &D, query: &str, max_distance: u8) -> Self {
        let query_text = Arc::<str>::from(query);
        let query_chars: Vec<char> = query_text.chars().collect();
        let relevant_subwords =
            precompute_relevant_subwords(&query_chars, usize::from(max_distance));
        let base_automaton_states =
            state_encoding::estimate_automaton_states(query_chars.len(), usize::from(max_distance));
        let max_automaton_states = base_automaton_states
            .saturating_mul(universal_query_state_factor(query_chars.len()))
            .max(1);

        let root = dictionary.root();
        let node_registry = DepthDictionaryNodeRegistry::new(root);
        let state_registry = UniversalStateRegistry::new(max_distance);

        Self {
            dictionary: dictionary.clone(),
            query_text,
            query_chars: Arc::new(query_chars),
            relevant_subwords: Arc::new(relevant_subwords),
            state_registry: Arc::new(std::sync::RwLock::new(state_registry)),
            node_registry: Arc::new(std::sync::RwLock::new(node_registry)),
            max_automaton_states,
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
        let Some((dict_node_id, automaton_state_id)) =
            state_encoding::decode(state, self.max_automaton_states)
        else {
            return false;
        };

        let node_is_registered = {
            let registry = crate::read_lock(&self.node_registry);
            registry.get_node(dict_node_id).is_some()
        };
        let state_is_registered = {
            let registry = crate::read_lock(&self.state_registry);
            registry.get_state(automaton_state_id).is_some()
        };

        node_is_registered && state_is_registered
    }

    pub(crate) fn final_weight_for_state(&self, state: StateId) -> Option<TropicalWeight> {
        let (dict_node_id, automaton_state_id) =
            state_encoding::decode(state, self.max_automaton_states)?;
        let node_registry = crate::read_lock(&self.node_registry);
        let state_registry = crate::read_lock(&self.state_registry);

        let dict_node = node_registry.get_node(dict_node_id)?;
        let dict_depth = node_registry.get_depth(dict_node_id)?;
        let registered_state = state_registry.get_registered_state(automaton_state_id)?;

        self.registered_final_weight(
            dict_node,
            registered_state.state.as_ref(),
            registered_state.query_pos,
            self.query_chars.len(),
            dict_depth,
        )
    }

    pub(crate) fn registered_state_id_span(&self) -> usize {
        let node_registry = crate::read_lock(&self.node_registry);
        let state_registry = crate::read_lock(&self.state_registry);
        crate::registered_product_state_id_span(
            node_registry.len(),
            usize::try_from(self.max_automaton_states).unwrap_or(usize::MAX),
            state_registry.len(),
        )
    }

    #[inline]
    fn encode_product_state(&self, dict_node_id: u32, automaton_state_id: u32) -> Option<StateId> {
        state_encoding::try_encode(dict_node_id, automaton_state_id, self.max_automaton_states)
    }

    #[inline]
    fn relevant_subword_for_depth(&self, dict_depth: usize) -> (usize, &[char]) {
        self.relevant_subwords
            .get(dict_depth)
            .map(|window| (window.false_prefix, window.units.as_slice()))
            .unwrap_or((0, &[]))
    }

    /// Compute transitions for a product state.
    fn compute_transitions(
        &self,
        dict_node_id: u32,
        automaton_state_id: u32,
    ) -> (
        bool,
        TropicalWeight,
        SmallVec<[WeightedTransition<char, TropicalWeight>; 4]>,
    ) {
        let node_registry = crate::read_lock(&self.node_registry);
        let state_registry = crate::read_lock(&self.state_registry);

        let (dict_node, dict_depth) = match (
            node_registry.get_node(dict_node_id),
            node_registry.get_depth(dict_node_id),
        ) {
            (Some(node), Some(depth)) => (node.clone(), depth),
            _ => {
                return (false, TropicalWeight::zero(), SmallVec::new());
            }
        };

        let registered_state = match state_registry.get_registered_state(automaton_state_id) {
            Some(state) => state.clone(),
            None => {
                return (false, TropicalWeight::zero(), SmallVec::new());
            }
        };
        let automaton_state = registered_state.state;
        let current_pos = registered_state.query_pos;

        drop(node_registry);
        drop(state_registry);

        let query_len = self.query_chars.len();
        let can_advance_dictionary = current_pos <= query_len;
        let extra_transitions = usize::from(current_pos < query_len);
        let mut transitions = crate::dictionary_edge_capacity(
            &dict_node,
            usize::from(can_advance_dictionary),
            extra_transitions,
        )
        .map_or_else(SmallVec::new, SmallVec::with_capacity);
        let Some(from_state) = self.encode_product_state(dict_node_id, automaton_state_id) else {
            return (false, TropicalWeight::zero(), transitions);
        };
        let relevant_subword =
            can_advance_dictionary.then(|| self.relevant_subword_for_depth(dict_depth));

        // For each dictionary edge, compute the transition. Registry writes
        // are batched across the edge scan to avoid lock churn on high-degree
        // dictionary nodes while preserving the node-before-state lock order.
        if let Some((false_prefix, subword)) = relevant_subword {
            let query_label = self.query_chars.get(current_pos).copied();
            let next_query_pos = current_pos + usize::from(query_label.is_some());
            let mut pending_edges = crate::dictionary_edge_capacity(&dict_node, 1, 0)
                .map_or_else(Vec::new, Vec::with_capacity);

            for (unit, child_node) in dict_node.edges() {
                let dict_char: char = unit.into();
                // The universal automaton is driven by the dictionary path as
                // the processed input and the query as the fixed word. Keep
                // that input index separate from `current_pos`, which is only
                // the WFST label cursor on the query side.
                let bit_vector =
                    CharacteristicVector::from_padded_units(dict_char, false_prefix, subword);

                // Compute next automaton state before taking registry write
                // locks; only ID assignment needs the shared registries.
                if let Some(next_auto_state) =
                    automaton_state.transition(&bit_vector, dict_depth.saturating_add(1))
                {
                    let child_key = DictionaryNodeKey::child(dict_node_id, dict_char);
                    let next_auto_key = universal_state_key(&next_auto_state, next_query_pos);
                    pending_edges.push((
                        dict_char,
                        child_node,
                        child_key,
                        next_auto_state,
                        next_query_pos,
                        next_auto_key,
                    ));
                }
            }

            if !pending_edges.is_empty() {
                let mut node_registry = crate::write_lock(&self.node_registry);
                let mut state_registry = crate::write_lock(&self.state_registry);

                for (
                    dict_char,
                    child_node,
                    child_key,
                    next_auto_state,
                    next_query_pos,
                    next_auto_key,
                ) in pending_edges
                {
                    let Some(candidate_child_id) = node_registry.candidate_id(child_key) else {
                        continue;
                    };
                    let Some(candidate_auto_id) =
                        state_registry.candidate_state_id_for_key(&next_auto_key)
                    else {
                        continue;
                    };
                    if self
                        .encode_product_state(candidate_child_id, candidate_auto_id)
                        .is_none()
                    {
                        continue;
                    }

                    let Some(child_node_id) =
                        node_registry.register_node(child_node, child_key, dict_depth + 1)
                    else {
                        continue;
                    };
                    let Some(next_auto_id) = state_registry.register_state_with_key(
                        next_auto_state,
                        next_query_pos,
                        next_auto_key,
                    ) else {
                        continue;
                    };

                    if let Some(target_state) =
                        self.encode_product_state(child_node_id, next_auto_id)
                    {
                        transitions.push(WeightedTransition::new(
                            from_state,
                            query_label,
                            Some(dict_char),
                            target_state,
                            TropicalWeight::one(),
                        ));
                    }
                }
            }
        }

        // Allow callers that traverse labels explicitly to consume query-side
        // characters without advancing the dictionary path. The universal
        // automaton's edit cost remains represented by the final weight.
        if current_pos < query_len {
            let next_query_pos = current_pos + 1;
            let next_auto_id = {
                let mut registry = crate::write_lock(&self.state_registry);
                let candidate_auto_id =
                    registry.candidate_state_id_at(automaton_state.as_ref(), next_query_pos);
                match candidate_auto_id {
                    Some(candidate_auto_id)
                        if self
                            .encode_product_state(dict_node_id, candidate_auto_id)
                            .is_some() =>
                    {
                        registry
                            .register_shared_state_at(Arc::clone(&automaton_state), next_query_pos)
                    }
                    _ => None,
                }
            };

            if let Some(next_auto_id) = next_auto_id {
                if let Some(target_state) = self.encode_product_state(dict_node_id, next_auto_id) {
                    transitions.push(WeightedTransition::new(
                        from_state,
                        Some(self.query_chars[current_pos]),
                        None,
                        target_state,
                        TropicalWeight::one(),
                    ));
                }
            }
        }

        let accepted_final_weight = self.registered_final_weight(
            &dict_node,
            automaton_state.as_ref(),
            current_pos,
            self.query_chars.len(),
            dict_depth,
        );
        let is_final = accepted_final_weight.is_some();
        let final_weight = accepted_final_weight.unwrap_or_else(TropicalWeight::zero);

        (is_final, final_weight, transitions)
    }

    fn registered_final_weight(
        &self,
        dict_node: &D::Node,
        automaton_state: &UniversalState<V>,
        query_pos: usize,
        fixed_word_len: usize,
        dict_depth: usize,
    ) -> Option<TropicalWeight> {
        // The fixed query is encoded on the WFST input tape. A dictionary node
        // may already be accepting for the universal automaton while query
        // labels remain, but that product state is not final in the transduced
        // relation until the complete fixed input has been consumed.
        if query_pos != fixed_word_len || !dict_node.is_final() {
            return None;
        }

        universal_accepting_weight(automaton_state, fixed_word_len, dict_depth)
            .map(TropicalWeight::new)
    }
}

impl<V, D> DirectStateSource<char, TropicalWeight> for UniversalLevenshteinStateSource<V, D>
where
    V: PositionVariant + Clone + Send + Sync,
    V::State: Send + Sync,
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
{
    fn expand_state(&self, state: StateId) -> StateExpansion<char, TropicalWeight> {
        let Some((dict_node_id, automaton_state_id)) =
            state_encoding::decode(state, self.max_automaton_states)
        else {
            return StateExpansion::failed(ExpansionFailure::invalid_state(state));
        };

        let (is_final, final_weight, transitions) =
            self.compute_transitions(dict_node_id, automaton_state_id);

        if is_final {
            StateExpansion::final_state(final_weight, transitions)
        } else {
            StateExpansion::non_final(transitions)
        }
    }
}

impl<V, D> StateSource<char, TropicalWeight> for UniversalLevenshteinStateSource<V, D>
where
    V: PositionVariant + Clone + Send + Sync,
    V::State: Send + Sync,
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
{
    fn compute_state(&self, request: ExpansionRequest<'_>) -> StateExpansion<char, TropicalWeight> {
        fulfill_expansion_request(self, request)
    }

    fn start(&self) -> StateId {
        // Start state is (root_node=0, initial_automaton_state=0).
        0
    }

    fn num_states_hint(&self) -> Option<usize> {
        let node_registry = crate::read_lock(&self.node_registry);
        let state_registry = crate::read_lock(&self.state_registry);
        let states_per_dictionary_node =
            usize::try_from(self.max_automaton_states).unwrap_or(usize::MAX);
        let live_state_id_span = crate::registered_product_state_id_span(
            node_registry.len(),
            states_per_dictionary_node,
            state_registry.len(),
        );

        crate::dictionary_product_states_hint(
            self.dictionary
                .len()
                .map(|dict_size| dict_size.max(node_registry.len())),
            states_per_dictionary_node,
        )
        .map(|estimated_state_count| estimated_state_count.max(live_state_id_span))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::universal_state_support::{
        apply_i32_offset, bounded_count_to_f64, next_universal_registry_id, relevant_subword_at,
        relevant_window_at,
    };
    use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
    use liblevenshtein::transducer::universal::{
        Standard, Transposition, TranspositionState, UniversalPosition,
    };

    #[test]
    fn test_universal_state_registry_creation() {
        let registry = UniversalStateRegistry::<Standard>::new(2);
        assert_eq!(registry.len(), 1); // Initial state
    }

    #[test]
    fn test_universal_state_registry_register() {
        let mut registry = UniversalStateRegistry::<Standard>::new(2);
        let state = UniversalState::initial(2);
        let id = registry.register_state(state.clone());
        assert_eq!(id, Some(0)); // Should be same as initial

        // Same state should get same ID
        let id2 = registry.register_state(state);
        assert_eq!(id, id2);
    }

    #[test]
    fn test_universal_state_key_includes_query_position_and_stays_inline() {
        let registry = UniversalStateRegistry::<Standard>::new(2);
        let state = UniversalState::initial(2);

        let at_start = registry.state_to_key(&state, 0);
        let after_label = registry.state_to_key(&state, 1);

        assert_ne!(at_start, after_label);
        assert_eq!(at_start.query_pos, 0);
        assert_eq!(after_label.query_pos, 1);
        assert_eq!(at_start.length_diff, 0);
        assert!(!at_start.positions.spilled());
    }

    #[test]
    fn test_universal_state_key_includes_length_diff() {
        use liblevenshtein::transducer::universal::CharacteristicVector;

        let state = UniversalState::<Standard>::initial(2);
        let bit_vector = CharacteristicVector::new('a', "abc");
        let both_consumed = state
            .transition_with_consumption(&bit_vector, true, true)
            .expect("both-consumed transition should have a successor");
        let dict_consumed = state
            .transition_with_consumption(&bit_vector, false, true)
            .expect("dict-consumed transition should have a successor");

        assert_ne!(both_consumed.length_diff(), dict_consumed.length_diff());

        let both_key = universal_state_key(&both_consumed, 1);
        let dict_key = universal_state_key(&dict_consumed, 1);

        assert_eq!(both_key.positions, dict_key.positions);
        assert_ne!(both_key, dict_key);

        let mut registry = UniversalStateRegistry::<Standard>::new(2);
        assert_eq!(registry.register_state_at(both_consumed, 1), Some(1));
        assert_eq!(registry.register_state_at(dict_consumed, 1), Some(2));
        assert_eq!(registry.len(), 3);
    }

    #[test]
    fn test_universal_state_key_includes_variant_operation_phase() {
        let mut usual = UniversalState::<Transposition>::new(1);
        usual.add_position(
            UniversalPosition::new_i_with_state(-1, 1, 1, TranspositionState::Usual)
                .expect("ordinary phase is structurally valid"),
        );
        let mut pending = UniversalState::<Transposition>::new(1);
        pending.add_position(
            UniversalPosition::new_i_with_state(-1, 1, 1, TranspositionState::Transposing)
                .expect("pending phase is structurally valid"),
        );

        assert_ne!(
            universal_state_key(&usual, 1),
            universal_state_key(&pending, 1)
        );
    }

    #[test]
    fn test_universal_registered_state_clones_share_state_storage() {
        let mut registry = UniversalStateRegistry::<Standard>::new(2);

        let at_start = registry
            .get_registered_state(0)
            .expect("initial state should be registered")
            .clone();
        let cloned_at_start = registry
            .get_registered_state(0)
            .expect("initial state should remain registered")
            .clone();

        assert!(Arc::ptr_eq(&at_start.state, &cloned_at_start.state));

        let after_label =
            registry.register_shared_state_at(Arc::clone(&at_start.state), at_start.query_pos + 1);
        assert_eq!(after_label, Some(1));

        let registered_after_label = registry
            .get_registered_state(1)
            .expect("shared state should be registered at the next query position");
        assert!(Arc::ptr_eq(&at_start.state, &registered_after_label.state));
        assert_eq!(registered_after_label.query_pos, at_start.query_pos + 1);
    }

    #[test]
    fn test_universal_registry_id_helper_rejects_unrepresentable_len() {
        assert_eq!(next_universal_registry_id(0), Some(0));
        let Ok(max_u32) = usize::try_from(u32::MAX) else {
            return;
        };
        assert_eq!(next_universal_registry_id(max_u32), Some(u32::MAX));

        if let Some(overflowing_len) = max_u32.checked_add(1) {
            assert_eq!(next_universal_registry_id(overflowing_len), None);
        }
    }

    #[test]
    fn test_universal_query_state_factor_saturates_at_u32_limit() {
        assert_eq!(universal_query_state_factor(0), 1);

        let Ok(max_u32) = usize::try_from(u32::MAX) else {
            return;
        };

        if let Some(max_exact_len) = max_u32.checked_sub(1) {
            assert_eq!(universal_query_state_factor(max_exact_len), u32::MAX);
        }
        assert_eq!(universal_query_state_factor(max_u32), u32::MAX);
    }

    #[test]
    fn test_apply_i32_offset_rejects_usize_overflow_and_underflow() {
        assert_eq!(apply_i32_offset(10, 3), Some(13));
        assert_eq!(apply_i32_offset(10, -3), Some(7));
        assert_eq!(apply_i32_offset(usize::MAX, 1), None);
        assert_eq!(apply_i32_offset(0, -1), None);
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
    fn test_universal_state_source_creation() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help", "world"]);
        let source = UniversalLevenshteinStateSource::<Standard, _>::new(&dict, "helo", 2);

        assert_eq!(source.start(), 0);
        assert!(source.num_states_hint().is_some());
    }

    #[test]
    fn test_universal_state_source_query() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["hello"]);
        let source = UniversalLevenshteinStateSource::<Standard, _>::new(&dict, "helo", 2);

        assert_eq!(source.query(), "helo");
        assert_eq!(source.query_str(), "helo");
        let first = source.query_str();
        let second = source.query_str();
        assert!(std::ptr::eq(first.as_ptr(), second.as_ptr()));
    }

    #[test]
    fn test_universal_relevant_subword_avoids_signed_position_wraparound() {
        let chars: Vec<char> = "abcdef".chars().collect();

        assert_eq!(relevant_subword_at(&chars, 2, 1), "$$abcd");
        assert_eq!(relevant_subword_at(&chars, 2, usize::MAX), "");
    }

    #[test]
    fn test_universal_relevant_subwords_are_precomputed_by_depth() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["abcdef"]);
        let source = UniversalLevenshteinStateSource::<Standard, _>::new(&dict, "abcdef", 2);
        let cloned = source.clone();

        let (false_prefix, units) = source.relevant_subword_for_depth(0);
        assert_eq!(false_prefix, 2);
        assert_eq!(units, &['a', 'b', 'c', 'd']);

        let expected = relevant_window_at(&source.query_chars, 2, 4);
        let (false_prefix, units) = source.relevant_subword_for_depth(3);
        assert_eq!(false_prefix, expected.false_prefix);
        assert_eq!(units, expected.units);

        assert_eq!(source.relevant_subword_for_depth(usize::MAX), (0, &[][..]));
        assert!(Arc::ptr_eq(
            &source.relevant_subwords,
            &cloned.relevant_subwords
        ));
    }

    #[test]
    fn test_universal_state_source_prunes_unencodable_targets() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["a"]);
        let mut source = UniversalLevenshteinStateSource::<Standard, _>::new(&dict, "a", 1);
        source.max_automaton_states = 1;

        assert_eq!(registered_dictionary_node_count(&source), 1);
        assert_eq!(crate::read_lock(&source.state_registry).len(), 1);

        match source.expand_state(source.start()) {
            StateExpansion::Expanded { transitions, .. } => {
                assert!(transitions.is_empty());
            }
            StateExpansion::Failed(failure) => panic!("state expansion failed: {failure}"),
            StateExpansion::Cancelled(reason) => panic!("state expansion cancelled: {reason:?}"),
        }

        assert_eq!(registered_dictionary_node_count(&source), 1);
        assert_eq!(crate::read_lock(&source.state_registry).len(), 1);
    }

    #[test]
    fn test_universal_state_source_does_not_register_pruned_dictionary_edges() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["b"]);
        let source = UniversalLevenshteinStateSource::<Standard, _>::new(&dict, "a", 0);

        assert_eq!(registered_dictionary_node_count(&source), 1);

        match source.expand_state(source.start()) {
            StateExpansion::Expanded { transitions, .. } => {
                assert!(
                    !transitions
                        .iter()
                        .any(|transition| transition.output == Some('b')),
                    "mismatched dictionary edge must be pruned at distance zero"
                );
            }
            StateExpansion::Failed(failure) => panic!("state expansion failed: {failure}"),
            StateExpansion::Cancelled(reason) => panic!("state expansion cancelled: {reason:?}"),
        }

        assert_eq!(registered_dictionary_node_count(&source), 1);
    }

    #[test]
    fn test_universal_state_source_hint_covers_registered_state_id_span() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["abcd"]);
        let source = UniversalLevenshteinStateSource::<Standard, _>::new(&dict, "a", 0);

        let _ = source.expand_state(source.start());

        let registered_nodes = registered_dictionary_node_count(&source);
        let states_per_dictionary_node =
            usize::try_from(source.max_automaton_states).unwrap_or(usize::MAX);
        let live_state_id_span = source.registered_state_id_span();
        let expected_hint = crate::dictionary_product_states_hint(
            Some(dict.len().unwrap_or(0).max(registered_nodes)),
            states_per_dictionary_node,
        )
        .map(|state_count| state_count.max(live_state_id_span));

        assert!(registered_nodes > dict.len().unwrap_or(0));
        assert!(live_state_id_span >= registered_nodes);
        assert_eq!(
            StateSource::<char, TropicalWeight>::num_states_hint(&source),
            expected_hint
        );
    }

    fn registered_dictionary_node_count(
        source: &UniversalLevenshteinStateSource<Standard, DynamicDawgChar<()>>,
    ) -> usize {
        let registry = crate::read_lock(&source.node_registry);
        let mut count = 0usize;
        while registry.get_node(count as u32).is_some() {
            count += 1;
        }
        count
    }

    fn start_transition_labels(
        dict_terms: Vec<&str>,
        query: &str,
    ) -> Vec<(Option<char>, Option<char>, f64)> {
        let dict = DynamicDawgChar::<()>::from_terms(dict_terms);
        let source = UniversalLevenshteinStateSource::<Standard, _>::new(&dict, query, 1);
        match source.expand_state(source.start()) {
            StateExpansion::Expanded { transitions, .. } => transitions
                .iter()
                .map(|transition| {
                    (
                        transition.input,
                        transition.output,
                        transition.weight.value(),
                    )
                })
                .collect(),
            StateExpansion::Failed(_) | StateExpansion::Cancelled(_) => Vec::new(),
        }
    }

    fn contains_label(
        labels: &[(Option<char>, Option<char>, f64)],
        input: Option<char>,
        output: Option<char>,
        weight: f64,
    ) -> bool {
        labels
            .iter()
            .any(|(actual_input, actual_output, actual_weight)| {
                *actual_input == input
                    && *actual_output == output
                    && (*actual_weight - weight).abs() <= f64::EPSILON
            })
    }

    fn accepts_any_path(
        source: &UniversalLevenshteinStateSource<Standard, DynamicDawgChar<()>>,
    ) -> bool {
        let mut stack = vec![source.start()];
        let mut seen = std::collections::HashSet::new();

        while let Some(state_id) = stack.pop() {
            if !seen.insert(state_id) {
                continue;
            }

            match source.expand_state(state_id) {
                StateExpansion::Expanded {
                    is_final,
                    transitions,
                    ..
                } => {
                    if is_final {
                        return true;
                    }
                    stack.extend(transitions.iter().map(|transition| transition.to));
                }
                StateExpansion::Failed(failure) => panic!("state expansion failed: {failure}"),
                StateExpansion::Cancelled(reason) => {
                    panic!("state expansion cancelled: {reason:?}")
                }
            }
        }

        false
    }

    fn best_path_weight(
        source: &UniversalLevenshteinStateSource<Standard, DynamicDawgChar<()>>,
    ) -> Option<f64> {
        let mut stack = vec![(source.start(), 0.0)];
        let mut best_by_state = std::collections::HashMap::new();
        let mut best_final: Option<f64> = None;

        while let Some((state_id, cost)) = stack.pop() {
            if best_final.is_some_and(|best| cost >= best) {
                continue;
            }
            if best_by_state
                .get(&state_id)
                .is_some_and(|best: &f64| cost >= *best)
            {
                continue;
            }
            best_by_state.insert(state_id, cost);

            match source.expand_state(state_id) {
                StateExpansion::Expanded {
                    is_final,
                    final_weight,
                    transitions,
                } => {
                    if is_final {
                        let candidate = cost + final_weight.value();
                        if best_final.is_none_or(|best| candidate < best) {
                            best_final = Some(candidate);
                        }
                    }

                    stack.extend(
                        transitions
                            .iter()
                            .map(|transition| (transition.to, cost + transition.weight.value())),
                    );
                }
                StateExpansion::Failed(failure) => panic!("state expansion failed: {failure}"),
                StateExpansion::Cancelled(reason) => {
                    panic!("state expansion cancelled: {reason:?}")
                }
            }
        }

        best_final
    }

    fn reaches_label_pair(
        source: &UniversalLevenshteinStateSource<Standard, DynamicDawgChar<()>>,
        query: &str,
        term: &str,
    ) -> bool {
        let mut stack = vec![(source.start(), String::new(), String::new())];
        let mut seen = std::collections::HashSet::new();

        while let Some((state_id, input, output)) = stack.pop() {
            if !seen.insert((state_id, input.clone(), output.clone())) {
                continue;
            }

            match source.expand_state(state_id) {
                StateExpansion::Expanded {
                    is_final,
                    transitions,
                    ..
                } => {
                    if is_final && input == query && output == term {
                        return true;
                    }

                    for transition in transitions {
                        let mut next_input = input.clone();
                        let mut next_output = output.clone();

                        if let Some(ch) = transition.input {
                            next_input.push(ch);
                        }
                        if let Some(ch) = transition.output {
                            next_output.push(ch);
                        }

                        if query.starts_with(&next_input) && term.starts_with(&next_output) {
                            stack.push((transition.to, next_input, next_output));
                        }
                    }
                }
                StateExpansion::Failed(failure) => panic!("state expansion failed: {failure}"),
                StateExpansion::Cancelled(reason) => {
                    panic!("state expansion cancelled: {reason:?}")
                }
            }
        }

        false
    }

    #[test]
    fn test_universal_state_source_transition_labels_preserve_transducer_sides() {
        let substitution = start_transition_labels(vec!["cat"], "bat");
        assert!(contains_label(&substitution, Some('b'), Some('c'), 0.0));

        let identity = start_transition_labels(vec!["cat"], "cat");
        assert!(contains_label(&identity, Some('c'), Some('c'), 0.0));
    }

    #[test]
    fn test_universal_state_source_accepts_standard_edit_cases() {
        for (term, query) in [("b", "a"), ("test", "tet"), ("cat", "at"), ("at", "cat")] {
            let dict = DynamicDawgChar::<()>::from_terms(vec![term]);
            let source = UniversalLevenshteinStateSource::<Standard, _>::new(&dict, query, 1);

            assert!(
                accepts_any_path(&source),
                "expected universal source to accept {term:?} within distance 1 of {query:?}"
            );
        }
    }

    #[test]
    fn test_universal_state_source_prunes_above_max_distance() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["bbb"]);
        let source = UniversalLevenshteinStateSource::<Standard, _>::new(&dict, "a", 1);

        assert!(!accepts_any_path(&source));
    }

    #[test]
    fn test_universal_state_source_weights_paths_by_final_edit_distance() {
        for (term, query, expected) in [
            ("cat", "cat", 0.0),
            ("b", "a", 1.0),
            ("test", "tet", 1.0),
            ("cat", "at", 1.0),
            ("at", "cat", 1.0),
        ] {
            let dict = DynamicDawgChar::<()>::from_terms(vec![term]);
            let source = UniversalLevenshteinStateSource::<Standard, _>::new(&dict, query, 1);
            let actual = best_path_weight(&source)
                .expect("expected an accepting path for one-edit universal case");

            assert!(
                (actual - expected).abs() <= f64::EPSILON,
                "expected path weight {expected} for {query:?}->{term:?}, got {actual}"
            );
        }
    }

    #[test]
    fn test_universal_state_source_can_spell_full_label_pairs() {
        for (term, query) in [("cat", "at"), ("at", "cat"), ("ab", "axb")] {
            let dict = DynamicDawgChar::<()>::from_terms(vec![term]);
            let source = UniversalLevenshteinStateSource::<Standard, _>::new(&dict, query, 1);

            assert!(
                reaches_label_pair(&source, query, term),
                "expected a full-label path for {query:?}->{term:?}"
            );
        }
    }

    #[test]
    fn test_universal_state_source_tracks_exact_query_position() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["ab"]);
        let source = UniversalLevenshteinStateSource::<Standard, _>::new(&dict, "xy", 2);

        let first_target = match source.expand_state(source.start()) {
            StateExpansion::Expanded { transitions, .. } => transitions
                .iter()
                .find(|transition| {
                    transition.input == Some('x')
                        && transition.output == Some('a')
                        && transition.weight.value() == 0.0
                })
                .map(|transition| transition.to)
                .expect("expected first substitution transition"),
            StateExpansion::Failed(failure) => panic!("state expansion failed: {failure}"),
            StateExpansion::Cancelled(reason) => panic!("state expansion cancelled: {reason:?}"),
        };

        match source.expand_state(first_target) {
            StateExpansion::Expanded { transitions, .. } => {
                assert!(
                    transitions.iter().any(|transition| {
                        transition.input == Some('y') && transition.output == Some('b')
                    }),
                    "second dictionary edge must consume the second query character"
                );
                assert!(
                    !transitions.iter().any(|transition| {
                        transition.input == Some('x') && transition.output == Some('b')
                    }),
                    "query position must not be recovered from abstract offsets"
                );
            }
            StateExpansion::Failed(failure) => panic!("state expansion failed: {failure}"),
            StateExpansion::Cancelled(reason) => panic!("state expansion cancelled: {reason:?}"),
        }
    }
}
