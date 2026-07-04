//! Generalized Automata WFST wrapper.
//!
//! This module provides [`GeneralizedWfst`], a WFST wrapper for the generalized
//! Levenshtein automaton that supports runtime-configurable operations.
//!
//! # Overview
//!
//! The [`GeneralizedAutomaton`] supports:
//! - Standard operations (match, substitute, insert, delete)
//! - Transposition operations
//! - Merge and split operations
//! - Phonetic operations (digraphs like ph↔f, ch↔k)
//!
//! This WFST wrapper exposes these capabilities in a form compatible with
//! lling-llang WFST composition pipelines.
//!
//! # Example
//!
//! ```rust,no_run
//! use duallity::{GeneralizedWfst, GeneralizedWfstBuilder};
//! use liblevenshtein::transducer::OperationSet;
//! use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
//!
//! let dict = DynamicDawgChar::<()>::from_terms(vec!["phone", "graph", "church"]);
//!
//! // Create with phonetic operations
//! let wfst = GeneralizedWfstBuilder::new(&dict)
//!     .query("fone")
//!     .max_distance(2)
//!     .with_phonetic_digraphs()
//!     .build();
//! ```

use lling_llang::prelude::{
    LazyState, LazyWfst, Semiring, StateId, StateSource, TropicalWeight, WeightedTransition, Wfst,
};
use smallvec::SmallVec;
use std::sync::{Arc, RwLock};

use libdictenstein::{Dictionary, DictionaryNode};
use liblevenshtein::transducer::generalized::GeneralizedAutomaton;
use liblevenshtein::transducer::OperationSet;
#[cfg(test)]
use liblevenshtein::transducer::OperationType;

#[cfg(test)]
use crate::generalized_ops::OperationApplicability;
use crate::generalized_ops::{
    bounded_operation_set, canonical_cost, compute_query_only_costs, operation_applies,
    prepare_operations, str_segment_by_char_width, PreparedOperation,
};
use crate::generalized_state_support::next_label_pair;
#[cfg(test)]
use crate::generalized_state_support::next_state_id;
use crate::generalized_state_support::{
    reserve_operation_path_capacity, ByteBuffer, DictPath, DictPathCache, DictPaths, EmitState,
    LabelBuffer, ProductState, QuerySegment, QuerySegmentCache, RegisteredState, StateRegistry,
};
use crate::lazy_cache::{empty_char_transitions, CachedCharState, LazyStateCache};
use crate::node_key::DictionaryNodeKey;
use crate::node_registry::DictionaryNodeRegistry;

/// Default maximum cache size used when LRU policy delegates to the wrapper default.
const DEFAULT_MAX_CACHE_SIZE: usize = 100_000;

struct OperationPathLabels<'a> {
    from: StateId,
    target: StateId,
    input: &'a [char],
    output: &'a [char],
    weight: f64,
}

struct DictionaryPathCollection<'a, N: DictionaryNode> {
    registry: &'a mut DictionaryNodeRegistry<N>,
    output: &'a mut LabelBuffer,
    bytes: &'a mut ByteBuffer,
    paths: &'a mut DictPaths,
}

/// Generalized Automaton WFST wrapper.
///
/// Exposes the generalized Levenshtein automaton with runtime-configurable
/// operations as a WFST compatible with lling-llang composition.
#[derive(Clone)]
pub struct GeneralizedWfst<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: DictionaryNode<Unit = char>,
{
    /// Owned copy of the dictionary.
    dictionary: D,

    /// The query string.
    query: String,

    /// The generalized automaton.
    automaton: GeneralizedAutomaton,

    /// Runtime-configurable operation set used for lazy product transitions.
    operations: OperationSet,

    /// Cached operation widths, weights, and applicability classes.
    prepared_operations: Vec<PreparedOperation>,

    /// Minimum cost to consume each query suffix without producing dictionary output.
    query_only_costs: Vec<Option<f64>>,

    /// Registry for dictionary nodes discovered during lazy expansion.
    node_registry: Arc<RwLock<DictionaryNodeRegistry<D::Node>>>,

    /// Registry for product and continuation states.
    state_registry: Arc<RwLock<StateRegistry>>,

    /// Cached state computations.
    cache: LazyStateCache<CachedCharState>,
}

impl<D> GeneralizedWfst<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: DictionaryNode<Unit = char>,
{
    /// Create a new generalized WFST.
    pub fn new(dictionary: &D, query: &str, max_distance: u8, operations: OperationSet) -> Self {
        let operations = bounded_operation_set(max_distance, operations);
        let automaton = GeneralizedAutomaton::with_operations(max_distance, operations.clone());
        let prepared_operations = prepare_operations(&operations);
        let query_only_costs =
            compute_query_only_costs(query, operations.operations(), &prepared_operations);
        let node_registry = Arc::new(RwLock::new(DictionaryNodeRegistry::new(dictionary.root())));
        let state_registry = Arc::new(RwLock::new(StateRegistry::new()));

        Self {
            dictionary: dictionary.clone(),
            query: query.to_string(),
            automaton,
            operations,
            prepared_operations,
            query_only_costs,
            node_registry,
            state_registry,
            cache: LazyStateCache::new(DEFAULT_MAX_CACHE_SIZE),
        }
    }

    /// Get the query string.
    #[inline]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Get the maximum distance.
    #[inline]
    pub fn max_distance(&self) -> u8 {
        self.automaton.max_distance()
    }

    /// Set the maximum cache size used by `CachePolicy::Lru { max_states: 0 }`.
    pub fn set_max_cache_size(&mut self, size: usize) {
        self.cache.set_max_lru_states(size);
    }

    fn computed_state(&self, state: StateId) -> Option<&CachedCharState> {
        self.cache.get(state)
    }

    /// Ensure a state is computed and cached.
    fn ensure_state(&mut self, state_id: StateId) {
        if self.cache.touch_if_cached(state_id) {
            return;
        }

        if let Some(cached) =
            CachedCharState::from_lazy_state(self.compute_registered_state(state_id))
        {
            self.cache.insert(state_id, cached);
        }
    }

    fn compute_registered_state(&self, state_id: StateId) -> LazyState<char, TropicalWeight> {
        let Some(state) = self.registered_state(state_id) else {
            return LazyState::non_final(SmallVec::new());
        };

        let (is_final, final_weight, transitions) = match state {
            RegisteredState::Product(product) => self.compute_product_state(state_id, product),
            RegisteredState::Emit(emit) => self.compute_emit_state(state_id, &emit),
        };

        if is_final {
            LazyState::final_state(final_weight, transitions)
        } else {
            LazyState::non_final(transitions)
        }
    }

    fn registered_state(&self, state_id: StateId) -> Option<RegisteredState> {
        let registry = crate::read_lock(&self.state_registry);
        registry.get(state_id).cloned()
    }

    fn final_weight_for_state(&self, state_id: StateId) -> Option<TropicalWeight> {
        match self.registered_state(state_id)? {
            RegisteredState::Product(product) => {
                let dict_node = self.dictionary_node(product.dict_node_id)?;
                self.product_final_weight(product, &dict_node)
            }
            RegisteredState::Emit(_) => None,
        }
    }

    fn compute_product_state(
        &self,
        state_id: StateId,
        product: ProductState,
    ) -> (
        bool,
        TropicalWeight,
        SmallVec<[WeightedTransition<char, TropicalWeight>; 4]>,
    ) {
        let Some(dict_node) = self.dictionary_node(product.dict_node_id) else {
            return (false, TropicalWeight::zero(), SmallVec::new());
        };

        if product.query_byte_pos > self.query.len() || !self.cost_within_bound(product.cost) {
            return (false, TropicalWeight::zero(), SmallVec::new());
        }

        let accepted_final_weight = self.product_final_weight(product, &dict_node);
        let is_final = accepted_final_weight.is_some();
        let final_weight = accepted_final_weight.unwrap_or_else(TropicalWeight::zero);

        let operations = self.operations.operations();
        let mut transitions = SmallVec::with_capacity(self.prepared_operations.len());
        let mut paths_by_width = DictPathCache::new();
        let mut query_segments_by_width = QuerySegmentCache::new();

        for prepared in &self.prepared_operations {
            let Some(op) = operations.get(prepared.index) else {
                continue;
            };
            let dict_width = prepared.consume_x;
            let query_width = prepared.consume_y;

            if dict_width == 0 && query_width == 0 {
                continue;
            }

            let next_cost = product.cost + prepared.weight;
            if !self.cost_within_bound(next_cost) {
                continue;
            }

            let Some(query_segment_index) = self.query_segment_cache_index(
                &mut query_segments_by_width,
                product.query_byte_pos,
                query_width,
            ) else {
                continue;
            };
            let query_segment = &query_segments_by_width[query_segment_index].1;

            let paths_index =
                self.dict_path_cache_index(&mut paths_by_width, product.dict_node_id, dict_width);
            let paths = &paths_by_width[paths_index].1;

            let mut applicable_paths = SmallVec::<[&DictPath; 4]>::new();
            for path in paths.iter() {
                if operation_applies(
                    prepared,
                    op,
                    &path.output,
                    &path.bytes,
                    &query_segment.chars,
                    query_segment.bytes,
                ) {
                    applicable_paths.push(path);
                }
            }

            if applicable_paths.is_empty() {
                continue;
            }

            reserve_operation_path_capacity(&mut transitions, applicable_paths.len());
            let mut state_registry = crate::write_lock(&self.state_registry);
            for path in applicable_paths {
                let Some(target) = state_registry.register_product(ProductState {
                    dict_node_id: path.target_node_id,
                    query_byte_pos: query_segment.end_byte_pos,
                    cost: canonical_cost(next_cost),
                }) else {
                    continue;
                };

                self.push_operation_path(
                    &mut state_registry,
                    &mut transitions,
                    OperationPathLabels {
                        from: state_id,
                        target,
                        input: &query_segment.chars,
                        output: &path.output,
                        weight: prepared.weight,
                    },
                );
            }
        }

        (is_final, final_weight, transitions)
    }

    fn compute_emit_state(
        &self,
        state_id: StateId,
        emit: &EmitState,
    ) -> (
        bool,
        TropicalWeight,
        SmallVec<[WeightedTransition<char, TropicalWeight>; 4]>,
    ) {
        let mut transitions = SmallVec::with_capacity(1);
        if let Some((input, output, next_input, next_output)) =
            next_label_pair(&emit.input, &emit.output, emit.input_pos, emit.output_pos)
        {
            let target = if next_input == emit.input.len() && next_output == emit.output.len() {
                Some(emit.target)
            } else {
                let mut state_registry = crate::write_lock(&self.state_registry);
                state_registry.register_emit(EmitState {
                    input: Arc::clone(&emit.input),
                    output: Arc::clone(&emit.output),
                    input_pos: next_input,
                    output_pos: next_output,
                    target: emit.target,
                })
            };

            if let Some(target) = target {
                transitions.push(WeightedTransition::new(
                    state_id,
                    input,
                    output,
                    target,
                    TropicalWeight::new(0.0),
                ));
            }
        }

        (false, TropicalWeight::zero(), transitions)
    }

    fn push_operation_path(
        &self,
        state_registry: &mut StateRegistry,
        transitions: &mut SmallVec<[WeightedTransition<char, TropicalWeight>; 4]>,
        labels: OperationPathLabels<'_>,
    ) {
        let Some((input_label, output_label, next_input, next_output)) =
            next_label_pair(labels.input, labels.output, 0, 0)
        else {
            return;
        };

        let to = if next_input == labels.input.len() && next_output == labels.output.len() {
            Some(labels.target)
        } else {
            state_registry.register_emit(EmitState {
                input: Arc::from(labels.input),
                output: Arc::from(labels.output),
                input_pos: next_input,
                output_pos: next_output,
                target: labels.target,
            })
        };

        if let Some(to) = to {
            transitions.push(WeightedTransition::new(
                labels.from,
                input_label,
                output_label,
                to,
                TropicalWeight::new(labels.weight),
            ));
        }
    }

    fn dictionary_paths_exact_chars(&self, start_node_id: u32, char_len: usize) -> DictPaths {
        let Some(start_node) = self.dictionary_node(start_node_id) else {
            return DictPaths::new();
        };

        if char_len == 0 {
            let mut paths = DictPaths::new();
            paths.push(DictPath {
                target_node_id: start_node_id,
                output: LabelBuffer::new(),
                bytes: ByteBuffer::new(),
            });
            return paths;
        }

        let mut paths = DictPaths::with_capacity(start_node.edge_count().unwrap_or(0));
        let mut output = LabelBuffer::new();
        let mut bytes = ByteBuffer::with_capacity(char_len.saturating_mul(4));
        let mut registry = crate::write_lock(&self.node_registry);
        let mut collection = DictionaryPathCollection {
            registry: &mut registry,
            output: &mut output,
            bytes: &mut bytes,
            paths: &mut paths,
        };
        self.collect_dictionary_paths(&mut collection, start_node_id, start_node, char_len);
        paths
    }

    fn collect_dictionary_paths(
        &self,
        collection: &mut DictionaryPathCollection<'_, D::Node>,
        parent_id: u32,
        node: D::Node,
        remaining_chars: usize,
    ) {
        for (dict_char, child_node) in node.edges() {
            if remaining_chars == 0 {
                continue;
            }

            let next_node = (remaining_chars > 1).then(|| child_node.clone());
            let Some(child_node_id) = collection
                .registry
                .register_node(child_node, DictionaryNodeKey::child(parent_id, dict_char))
            else {
                continue;
            };
            collection.output.push(dict_char);
            let mut encoded = [0u8; 4];
            let encoded = dict_char.encode_utf8(&mut encoded).as_bytes();
            collection.bytes.extend_from_slice(encoded);

            if remaining_chars == 1 {
                collection.paths.push(DictPath {
                    target_node_id: child_node_id,
                    output: collection.output.clone(),
                    bytes: collection.bytes.clone(),
                });
            } else if let Some(next_node) = next_node {
                self.collect_dictionary_paths(
                    collection,
                    child_node_id,
                    next_node,
                    remaining_chars - 1,
                );
            }

            collection
                .bytes
                .truncate(collection.bytes.len() - encoded.len());
            collection.output.pop();
        }
    }

    fn query_segment_cache_index<'a>(
        &'a self,
        cache: &mut QuerySegmentCache<'a>,
        start: usize,
        char_len: usize,
    ) -> Option<usize> {
        if let Some(index) = cache.iter().position(|(width, _)| *width == char_len) {
            return Some(index);
        }

        let (segment, end_byte_pos) = self.query_segment(start, char_len)?;
        cache.push((char_len, QuerySegment::new(segment, end_byte_pos)));
        Some(cache.len() - 1)
    }

    fn dict_path_cache_index(
        &self,
        cache: &mut DictPathCache,
        start_node_id: u32,
        char_len: usize,
    ) -> usize {
        if let Some(index) = cache.iter().position(|(width, _)| *width == char_len) {
            return index;
        }

        let paths = self.dictionary_paths_exact_chars(start_node_id, char_len);
        cache.push((char_len, paths));
        cache.len() - 1
    }

    fn dictionary_node(&self, node_id: u32) -> Option<D::Node> {
        let registry = crate::read_lock(&self.node_registry);
        registry.get_node(node_id).cloned()
    }

    fn product_final_weight(
        &self,
        product: ProductState,
        dict_node: &D::Node,
    ) -> Option<TropicalWeight> {
        if product.query_byte_pos > self.query.len()
            || !self.cost_within_bound(product.cost)
            || !dict_node.is_final()
        {
            return None;
        }

        self.query_only_costs
            .get(product.query_byte_pos)
            .copied()
            .flatten()
            .filter(|extra| self.cost_within_bound(product.cost + extra))
            .map(TropicalWeight::new)
    }

    fn query_segment(&self, start: usize, char_len: usize) -> Option<(&str, usize)> {
        str_segment_by_char_width(&self.query, start, char_len)
    }

    #[inline]
    fn cost_within_bound(&self, cost: f64) -> bool {
        cost <= f64::from(self.automaton.max_distance()) + f64::EPSILON
    }

    #[inline]
    fn is_registered_state(&self, state: StateId) -> bool {
        let registry = crate::read_lock(&self.state_registry);
        registry.get(state).is_some()
    }
}

impl<D> Wfst<char, TropicalWeight> for GeneralizedWfst<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: DictionaryNode<Unit = char>,
{
    fn start(&self) -> StateId {
        0
    }

    fn is_final(&self, state: StateId) -> bool {
        self.computed_state(state)
            .map(|s| s.is_final)
            .unwrap_or_else(|| self.final_weight_for_state(state).is_some())
    }

    fn final_weight(&self, state: StateId) -> TropicalWeight {
        self.computed_state(state)
            .map(|s| s.final_weight)
            .unwrap_or_else(|| {
                self.final_weight_for_state(state)
                    .unwrap_or_else(TropicalWeight::infinity)
            })
    }

    fn transitions(&self, state: StateId) -> &[WeightedTransition<char, TropicalWeight>] {
        match self.computed_state(state) {
            Some(cached) => cached.transitions.as_slice(),
            None => empty_char_transitions(),
        }
    }

    fn total_transitions(&self) -> usize {
        self.cache.total_cached_transitions()
    }

    fn num_states(&self) -> usize {
        let registry = crate::read_lock(&self.state_registry);
        registry.len()
    }

    #[inline]
    fn is_empty(&self) -> bool {
        false
    }

    #[inline]
    fn is_valid_state(&self, state: StateId) -> bool {
        self.is_registered_state(state)
    }
}

impl<D> LazyWfst<char, TropicalWeight> for GeneralizedWfst<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: DictionaryNode<Unit = char>,
{
    fn is_expanded(&self, state: StateId) -> bool {
        self.cache.is_expanded(state)
    }

    fn expand(&mut self, state: StateId) {
        self.ensure_state(state);
    }

    fn transitions_lazy(&mut self, state: StateId) -> &[WeightedTransition<char, TropicalWeight>] {
        self.ensure_state(state);
        self.transitions(state)
    }

    fn cache_policy(&self) -> lling_llang::wfst::CachePolicy {
        self.cache.policy()
    }

    fn set_cache_policy(&mut self, policy: lling_llang::wfst::CachePolicy) {
        self.cache.set_policy(policy);
    }

    fn computed_states(&self) -> usize {
        self.cache.len()
    }

    fn clear_cache(&mut self) {
        self.cache.clear();
    }
}

impl<D> StateSource<char, TropicalWeight> for GeneralizedWfst<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: DictionaryNode<Unit = char>,
{
    fn compute_state(&self, state: StateId) -> LazyState<char, TropicalWeight> {
        self.compute_registered_state(state)
    }

    fn start(&self) -> StateId {
        0
    }

    fn num_states_hint(&self) -> Option<usize> {
        let query_positions = self.query.len().saturating_add(1);
        let max_dist = usize::from(self.automaton.max_distance());
        let cost_levels = self
            .prepared_operations
            .len()
            .saturating_mul(max_dist + 1)
            .max(1);
        let registered_states = crate::read_lock(&self.state_registry).len();

        crate::dictionary_product_states_hint(
            self.dictionary.len(),
            query_positions.saturating_mul(cost_levels),
        )
        .map(|product_hint| product_hint.max(registered_states))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
    use liblevenshtein::transducer::OperationSetBuilder;

    #[test]
    fn test_generalized_registry_capacity_helper_rejects_overflow() {
        let Ok(max_state_id) = usize::try_from(StateId::MAX) else {
            return;
        };
        assert_eq!(next_state_id(max_state_id), Some(StateId::MAX));

        if let Some(overflowing_len) = max_state_id.checked_add(1) {
            assert_eq!(next_state_id(overflowing_len), None);
        }
    }

    #[test]
    fn test_generalized_registries_preallocate_start_state() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["a"]);
        let node_registry = DictionaryNodeRegistry::new(dict.root());
        let state_registry = StateRegistry::new();

        assert!(node_registry.node_capacity() >= 1);
        assert!(node_registry.key_capacity() >= 1);
        assert!(state_registry.id_to_state.capacity() >= 1);
        assert!(state_registry.product_to_id.capacity() >= 1);
        match state_registry.get(0) {
            Some(RegisteredState::Product(product)) => {
                assert_eq!(product.dict_node_id, 0);
                assert_eq!(product.query_byte_pos, 0);
                assert_eq!(product.cost, 0.0);
            }
            _ => std::panic::panic_any("expected start product state".to_owned()),
        }
    }

    #[test]
    fn test_generalized_state_hint_never_underreports_registered_states() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["a"]);
        let operations = OperationSetBuilder::new().build();
        let wfst = GeneralizedWfst::new(&dict, "", 0, operations);

        assert_eq!(
            StateSource::<char, TropicalWeight>::num_states_hint(&wfst),
            Some(1)
        );

        {
            let mut registry = crate::write_lock(&wfst.state_registry);
            let emit = EmitState {
                input: Arc::from(&['f'][..]),
                output: Arc::from(&['p'][..]),
                input_pos: 0,
                output_pos: 0,
                target: 0,
            };
            assert!(registry.register_emit(emit).is_some());
        }

        assert_eq!(Wfst::num_states(&wfst), 2);
        assert_eq!(
            StateSource::<char, TropicalWeight>::num_states_hint(&wfst),
            Some(2)
        );
    }

    #[test]
    fn test_generalized_reserves_operation_path_fanout_capacity() {
        let mut transitions =
            SmallVec::<[WeightedTransition<char, TropicalWeight>; 4]>::with_capacity(1);
        let initial_capacity = transitions.capacity();

        reserve_operation_path_capacity(&mut transitions, 1);
        assert_eq!(transitions.len(), 0);
        assert_eq!(transitions.capacity(), initial_capacity);

        reserve_operation_path_capacity(&mut transitions, 8);
        assert_eq!(transitions.len(), 0);
        assert!(transitions.capacity() >= 8);
    }

    #[test]
    fn test_generalized_constructor_stores_only_bounded_operations() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["a"]);
        let operations = OperationSetBuilder::new()
            .with_operation(OperationType::new(1, 1, 0.0, "match"))
            .with_operation(OperationType::new(1, 1, 1.0, "over_budget"))
            .build();

        let wfst = GeneralizedWfst::new(&dict, "a", 0, operations);

        assert_eq!(wfst.operations.len(), 1);
        assert_eq!(wfst.prepared_operations.len(), 1);
        assert_eq!(wfst.operations.operations()[0].name(), "match");
        assert_eq!(wfst.prepared_operations[0].index, 0);
        assert_eq!(
            wfst.prepared_operations[0].applicability,
            OperationApplicability::Match
        );
    }

    #[test]
    fn test_generalized_emit_registry_shares_map_and_state_storage() {
        let mut registry = StateRegistry::new();
        let emit = EmitState {
            input: Arc::from(&['p', 'h'][..]),
            output: Arc::from(&['f'][..]),
            input_pos: 1,
            output_pos: 0,
            target: 7,
        };

        let id = registry
            .register_emit(emit.clone())
            .expect("emit state should be registered");
        assert_eq!(registry.register_emit(emit.clone()), Some(id));

        let stored_emit = match registry.get(id) {
            Some(RegisteredState::Emit(stored_emit)) => stored_emit,
            _ => std::panic::panic_any("expected registered emit state".to_owned()),
        };
        let (map_emit, _) = registry
            .emit_to_id
            .get_key_value(&emit)
            .expect("emit key should exist in map");

        assert!(Arc::ptr_eq(stored_emit, map_emit));
        assert_eq!(registry.emit_to_id.len(), 1);
    }

    #[test]
    fn test_generalized_emit_continuations_share_label_storage() {
        let mut registry = StateRegistry::new();
        let emit = EmitState {
            input: Arc::from(&['p', 'h'][..]),
            output: Arc::from(&['f'][..]),
            input_pos: 0,
            output_pos: 0,
            target: 7,
        };
        let next_emit = EmitState {
            input: Arc::clone(&emit.input),
            output: Arc::clone(&emit.output),
            input_pos: 1,
            output_pos: 1,
            target: 7,
        };

        let id = registry
            .register_emit(emit.clone())
            .expect("emit state should be registered");
        let next_id = registry
            .register_emit(next_emit)
            .expect("continuation should be registered");

        let stored_emit = match registry.get(id) {
            Some(RegisteredState::Emit(stored_emit)) => stored_emit,
            _ => std::panic::panic_any("expected registered emit state".to_owned()),
        };
        let stored_next = match registry.get(next_id) {
            Some(RegisteredState::Emit(stored_emit)) => stored_emit,
            _ => std::panic::panic_any("expected registered continuation state".to_owned()),
        };

        assert!(Arc::ptr_eq(&stored_emit.input, &stored_next.input));
        assert!(Arc::ptr_eq(&stored_emit.output, &stored_next.output));
        assert_ne!(id, next_id);
    }

    #[test]
    fn test_query_segment_reuses_borrowed_bytes_and_decodes_chars() {
        let segment = QuerySegment::new("éa", "éa".len());

        assert_eq!(segment.bytes, "éa".as_bytes());
        assert_eq!(segment.chars.as_slice(), &['é', 'a']);
        assert_eq!(segment.end_byte_pos, 3);
        assert!(!segment.chars.spilled());
    }

    #[test]
    fn test_generalized_width_caches_reuse_entries() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["ab", "ac"]);
        let wfst = GeneralizedWfst::new(&dict, "ab", 1, OperationSet::standard());

        let mut query_cache = QuerySegmentCache::new();
        let first_query = wfst
            .query_segment_cache_index(&mut query_cache, 0, 1)
            .expect("query character width should be valid");
        let second_query = wfst
            .query_segment_cache_index(&mut query_cache, 0, 1)
            .expect("query character width should remain cached");
        assert_eq!(first_query, second_query);
        assert_eq!(query_cache.len(), 1);
        assert_eq!(query_cache[0].1.chars.as_slice(), &['a']);

        let mut path_cache = DictPathCache::new();
        let first_paths = wfst.dict_path_cache_index(&mut path_cache, 0, 1);
        let second_paths = wfst.dict_path_cache_index(&mut path_cache, 0, 1);
        assert_eq!(first_paths, second_paths);
        assert_eq!(path_cache.len(), 1);
        assert_eq!(path_cache[0].1.len(), 1);
        assert!(!path_cache[0].1.spilled());
        assert_eq!(path_cache[0].1[0].output.as_slice(), &['a']);
    }

    #[test]
    fn test_generalized_width_helpers_count_unicode_chars() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["éa"]);
        let wfst = GeneralizedWfst::new(&dict, "éa", 1, OperationSet::standard());

        let (segment, end_byte_pos) = wfst
            .query_segment(0, 1)
            .expect("one Unicode scalar should be a valid segment");
        assert_eq!(segment, "é");
        assert_eq!(end_byte_pos, "é".len());

        let paths = wfst.dictionary_paths_exact_chars(0, 1);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].output.as_slice(), &['é']);
        assert_eq!(paths[0].bytes.as_slice(), "é".as_bytes());
    }

    #[test]
    fn test_generalized_skips_dictionary_path_expansion_for_over_budget_operations() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["b"]);
        let operations = OperationSetBuilder::new()
            .with_operation(OperationType::new(1, 1, 1.0, "over_budget"))
            .build();
        let wfst = GeneralizedWfst::new(&dict, "a", 0, operations);

        assert_eq!(registered_dictionary_node_count(&wfst), 1);

        match StateSource::compute_state(&wfst, Wfst::start(&wfst)) {
            LazyState::Computed { transitions, .. } => {
                assert!(transitions.is_empty());
            }
            LazyState::Pending => {
                std::panic::panic_any("generalized WFST should compute eagerly".to_owned())
            }
        }

        assert_eq!(registered_dictionary_node_count(&wfst), 1);
    }

    fn registered_dictionary_node_count(wfst: &GeneralizedWfst<DynamicDawgChar<()>>) -> usize {
        let registry = crate::read_lock(&wfst.node_registry);
        let mut count = 0usize;
        while registry.get_node(count as u32).is_some() {
            count += 1;
        }
        count
    }

    #[test]
    fn test_short_generalized_operation_buffers_stay_inline() {
        let emit = EmitState {
            input: Arc::from(&['f'][..]),
            output: Arc::from(&['p', 'h'][..]),
            input_pos: 0,
            output_pos: 0,
            target: 1,
        };
        let path = DictPath {
            target_node_id: 1,
            output: ['p', 'h'].into_iter().collect(),
            bytes: [b'p', b'h'].into_iter().collect(),
        };

        assert_eq!(emit.input.as_ref(), &['f']);
        assert_eq!(emit.output.as_ref(), &['p', 'h']);
        assert!(!path.output.spilled());
        assert!(!path.bytes.spilled());
    }
}
