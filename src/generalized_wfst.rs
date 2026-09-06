//! Generalized Automata WFST wrapper.
//!
//! This module provides [`GeneralizedWfst`], a WFST wrapper for the generalized
//! Levenshtein automaton that supports runtime-configurable operations.
//!
//! # Overview
//!
//! The native [`GeneralizedAutomaton`](liblevenshtein::transducer::generalized::GeneralizedAutomaton) supports:
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
    CancellationToken, ExpansionError, ExpansionFailure, ExpansionRequest, ExpansionStatus,
    LazyWfst, Semiring, StateExpansion, StateId, StateSource, TropicalWeight, WeightedTransition,
    Wfst,
};
use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use std::sync::{Arc, RwLock};

use crate::generalized_expansion::{
    expansion_failure, require_at_most, resolved_node, ExpansionBudget, ExpansionStaging,
    PendingOperationArc, StateBatch,
};
use crate::generalized_limits::{
    GeneralizedWfstError, GeneralizedWfstLimits, GeneralizedWfstResource,
};
use libdictenstein::{Dictionary, DictionaryNode};
use liblevenshtein::cost::CostScale;
use liblevenshtein::transducer::OperationSet;
#[cfg(test)]
use liblevenshtein::transducer::OperationSetValidationError;
#[cfg(test)]
use liblevenshtein::transducer::OperationType;

use crate::generalized_ops::{
    bounded_operation_set, operation_applies, prepare_operations, str_segment_by_char_width,
    PreparedOperation,
};
#[cfg(test)]
use crate::generalized_state_support::next_state_id;
use crate::generalized_state_support::{
    ByteBuffer, DictPath, DictPathCache, DictPaths, EmissionChain, LabelBuffer,
    PendingDictionaryNode, ProductState, QuerySegment, QuerySegmentCache, RegisteredState,
    StateRegistry, WidthCacheEntry,
};
use crate::lazy_cache::{
    cache_char_state_expansion, cached_char_state_status, empty_char_transitions,
    invalid_state_error, CachedCharState, LazyStateCache,
};
use crate::node_key::DictionaryNodeKey;
use crate::node_registry::{next_registry_id, DictionaryNodeRegistry};
use crate::DirectStateSource;

#[path = "generalized_computation.rs"]
mod computation;

/// Default maximum cache size used when LRU policy delegates to the wrapper default.
const DEFAULT_MAX_CACHE_SIZE: usize = 100_000;

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

    /// Public unit budget; all internal pruning uses its exact scaled form.
    max_distance: u8,

    /// Runtime-configurable operation set used for lazy product transitions.
    operations: OperationSet,

    /// Cached operation indexes, widths, and weights.
    prepared_operations: Vec<PreparedOperation>,
    source_width_slot_count: usize,
    query_width_slot_count: usize,

    /// Exact fixed-point domain shared by every configured operation.
    cost_scale: CostScale,

    /// Maximum accepted cost represented in [`Self::cost_scale`] units.
    max_cost: usize,

    /// Immutable resource ceilings shared by all clones.
    limits: GeneralizedWfstLimits,

    /// Registry for dictionary nodes discovered during lazy expansion.
    node_registry: Arc<RwLock<DictionaryNodeRegistry<Arc<D::Node>>>>,

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
    ///
    /// # Panics
    ///
    /// Panics when `operations` is not a valid bounded alignment grammar. Use
    /// [`Self::try_new`] when operation sets come from an untrusted or dynamic
    /// source and the caller must handle validation errors.
    pub fn new(dictionary: &D, query: &str, max_distance: u8, operations: OperationSet) -> Self {
        Self::try_new(dictionary, query, max_distance, operations)
            .expect("invalid generalized WFST operation set")
    }

    /// Create a generalized WFST after validating the complete operation set.
    ///
    /// Validation precedes budget filtering and semantic deduplication so an
    /// invalid operation cannot disappear before its error is reported.
    pub fn try_new(
        dictionary: &D,
        query: &str,
        max_distance: u8,
        operations: OperationSet,
    ) -> Result<Self, GeneralizedWfstError> {
        Self::try_new_with_limits(
            dictionary,
            query,
            max_distance,
            operations,
            GeneralizedWfstLimits::default(),
        )
    }

    /// Create a generalized WFST with explicit inclusive resource ceilings.
    pub fn try_new_with_limits(
        dictionary: &D,
        query: &str,
        max_distance: u8,
        operations: OperationSet,
        limits: GeneralizedWfstLimits,
    ) -> Result<Self, GeneralizedWfstError> {
        operations.validate()?;
        Self::validate_construction_limits(query, &operations, limits)?;
        let cost_scale = CostScale::for_operations(&operations)?;
        let max_cost = cost_scale.scale_budget(max_distance)?;
        let operations = bounded_operation_set(cost_scale, max_cost, operations)?;
        let prepared_operations = prepare_operations(&operations, cost_scale)?;
        let source_width_slot_count = prepared_operations
            .iter()
            .map(|op| op.source_width_slot + 1)
            .max()
            .unwrap_or(0);
        let query_width_slot_count = prepared_operations
            .iter()
            .map(|op| op.query_width_slot + 1)
            .max()
            .unwrap_or(0);
        let node_registry = Arc::new(RwLock::new(DictionaryNodeRegistry::new(Arc::new(
            dictionary.root(),
        ))));
        let state_registry = Arc::new(RwLock::new(StateRegistry::new()));

        Ok(Self {
            dictionary: dictionary.clone(),
            query: query.to_string(),
            max_distance,
            operations,
            prepared_operations,
            source_width_slot_count,
            query_width_slot_count,
            cost_scale,
            max_cost,
            limits,
            node_registry,
            state_registry,
            cache: LazyStateCache::new(DEFAULT_MAX_CACHE_SIZE),
        })
    }

    fn validate_construction_limits(
        query: &str,
        operations: &OperationSet,
        limits: GeneralizedWfstLimits,
    ) -> Result<(), GeneralizedWfstError> {
        Self::require_at_most(
            GeneralizedWfstResource::QueryBytes,
            query.len(),
            limits.max_query_bytes,
        )?;
        Self::require_at_most(
            GeneralizedWfstResource::QueryScalars,
            query.chars().count(),
            limits.max_query_scalars,
        )?;
        Self::require_at_most(
            GeneralizedWfstResource::RetainedDictionaryNodes,
            1,
            limits.max_retained_dictionary_nodes,
        )?;
        Self::require_at_most(
            GeneralizedWfstResource::RetainedWfstStates,
            1,
            limits.max_retained_wfst_states,
        )?;
        for operation in operations.operations() {
            Self::require_at_most(
                GeneralizedWfstResource::OperationSourceScalars,
                operation.consume_x(),
                limits.max_operation_source_scalars,
            )?;
            Self::require_at_most(
                GeneralizedWfstResource::OperationQueryScalars,
                operation.consume_y(),
                limits.max_operation_query_scalars,
            )?;
        }
        Ok(())
    }

    #[inline]
    fn require_at_most(
        resource: GeneralizedWfstResource,
        required: usize,
        limit: usize,
    ) -> Result<(), GeneralizedWfstError> {
        if required <= limit {
            Ok(())
        } else {
            Err(GeneralizedWfstError::limit(resource, limit, required))
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
        self.max_distance
    }

    /// Dictionary snapshot used by this product.
    pub fn dictionary(&self) -> &D {
        &self.dictionary
    }

    /// Exact scale used for internal costs, pruning, and state identity.
    pub fn cost_scale(&self) -> CostScale {
        self.cost_scale
    }

    /// Immutable resource ceilings used by this WFST and its clones.
    pub fn limits(&self) -> GeneralizedWfstLimits {
        self.limits
    }

    /// Expand a state and return its complete arcs, propagating any failure.
    ///
    /// A limit failure commits no new dictionary/state identities or cached
    /// arcs from this expansion. Earlier successful expansions remain valid.
    pub fn try_transitions(
        &mut self,
        state: StateId,
    ) -> Result<&[WeightedTransition<char, TropicalWeight>], ExpansionError> {
        self.ensure_state(state)?;
        Ok(self.transitions(state))
    }

    /// Set the maximum cache size used by `CachePolicy::Lru { max_states: 0 }`.
    pub fn set_max_cache_size(&mut self, size: usize) {
        self.cache.set_max_lru_states(size);
    }

    fn computed_state(&self, state: StateId) -> Option<&CachedCharState> {
        self.cache.get(state)
    }

    /// Ensure a state is computed and cached.
    fn ensure_state(&mut self, state_id: StateId) -> Result<ExpansionStatus, ExpansionError> {
        if self.cache.touch_if_cached(state_id) {
            return cached_char_state_status(&self.cache, state_id);
        }

        let expansion = self.compute_registered_state(state_id);
        cache_char_state_expansion(&mut self.cache, state_id, expansion)
    }

    fn dictionary_node(&self, node_id: u32) -> Option<Arc<D::Node>> {
        let registry = crate::read_lock(&self.node_registry);
        registry.get_node(node_id).map(Arc::clone)
    }

    fn product_final_weight(
        &self,
        product: ProductState,
        dict_node: &D::Node,
    ) -> Option<TropicalWeight> {
        if product.query_byte_pos != self.query.len()
            || !self.cost_within_bound(product.cost)
            || !dict_node.is_final()
        {
            return None;
        }
        Some(TropicalWeight::new(0.0))
    }

    fn query_segment(&self, start: usize, char_len: usize) -> Option<(&str, usize)> {
        str_segment_by_char_width(&self.query, start, char_len)
    }

    #[inline]
    fn cost_within_bound(&self, cost: usize) -> bool {
        cost <= self.max_cost
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

    fn expand(&mut self, state: StateId) -> Result<ExpansionStatus, ExpansionError> {
        self.ensure_state(state)
    }

    fn transitions_lazy(&mut self, state: StateId) -> &[WeightedTransition<char, TropicalWeight>] {
        self.try_transitions(state).expect(
            "generalized expansion failed; use try_transitions or LazyWfst::expand to handle failures",
        )
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

impl<D> DirectStateSource<char, TropicalWeight> for GeneralizedWfst<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: DictionaryNode<Unit = char>,
{
    fn expand_state(&self, state: StateId) -> StateExpansion<char, TropicalWeight> {
        self.compute_registered_state(state)
    }
}

impl<D> StateSource<char, TropicalWeight> for GeneralizedWfst<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: DictionaryNode<Unit = char>,
{
    fn compute_state(&self, request: ExpansionRequest<'_>) -> StateExpansion<char, TropicalWeight> {
        self.compute_registered_state_with_check(
            request.state(),
            Some(request.cancellation()),
            &|| Ok(()),
        )
    }

    fn start(&self) -> StateId {
        0
    }

    fn num_states_hint(&self) -> Option<usize> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
    use liblevenshtein::transducer::OperationSetBuilder;

    fn counts(wfst: &GeneralizedWfst<DynamicDawgChar<()>>) -> (usize, usize, usize) {
        (
            crate::read_lock(&wfst.node_registry).len(),
            wfst.num_states(),
            wfst.computed_states(),
        )
    }

    #[test]
    fn constructor_checks_original_grammar_and_scale_before_filtering() {
        let dict = DynamicDawgChar::<()>::from_terms(["a"]);
        assert!(matches!(
            GeneralizedWfst::try_new(
                &dict,
                "a",
                0,
                OperationSetBuilder::new()
                    .with_operation(OperationType::new(0, 0, 2.0, "no_progress"))
                    .build()
            ),
            Err(GeneralizedWfstError::InvalidOperations(
                OperationSetValidationError::NoProgress { .. }
            ))
        ));
        for weight in [1e-100, 1e20] {
            let operations = OperationSetBuilder::new()
                .with_operation(OperationType::new(1, 1, weight, "unrepresentable"))
                .build();
            assert!(matches!(
                GeneralizedWfst::try_new(&dict, "a", 0, operations),
                Err(GeneralizedWfstError::CostScale(_))
            ));
        }
    }

    #[test]
    fn constructor_retains_exact_original_scale_after_filtering() {
        let dict = DynamicDawgChar::<()>::from_terms(["a"]);
        let operations = OperationSetBuilder::new()
            .with_match()
            .with_operation(OperationType::new(1, 1, 0.125, "eighth"))
            .with_operation(OperationType::new(1, 1, 0.15, "twentieth"))
            .build();
        let wfst = GeneralizedWfst::new(&dict, "a", 0, operations);
        assert_eq!(wfst.cost_scale().denominator(), 40);
        assert_eq!(wfst.prepared_operations.len(), 1);
        assert_eq!(wfst.prepared_operations[0].scaled_weight, 0);
        assert_eq!(counts(&wfst), (1, 1, 0));
        assert_eq!(StateSource::num_states_hint(&wfst), None);
    }

    #[test]
    fn state_identifier_overflow_is_detected() {
        assert_eq!(next_state_id(0), Some(0));
        if let Ok(max) = usize::try_from(StateId::MAX) {
            assert_eq!(next_state_id(max - 1), Some(StateId::MAX - 1));
            assert_eq!(next_state_id(max), None);
            if let Some(over) = max.checked_add(1) {
                assert_eq!(next_state_id(over), None);
            }
        }
    }

    #[test]
    fn distinct_width_cache_has_a_linear_charged_initialization_and_lookup_budget() {
        let dict = DynamicDawgChar::<()>::from_terms(Vec::<&str>::new());
        let mut builder = OperationSetBuilder::new();
        for width in 1..=90 {
            builder = builder.with_operation(OperationType::new(width, 0, 1.0, "wide"));
        }
        let operations = builder.build();
        // Widths sum to 4095: a valid adversarial native grammar. The ledger
        // is finality (1) + slots (91) + 90*(rule + edges + next + DFS-pop).
        let exact_work = 1 + 91 + 90 * 4;
        for limit in [exact_work - 1, exact_work] {
            let mut wfst = GeneralizedWfst::try_new_with_limits(
                &dict,
                "",
                1,
                operations.clone(),
                GeneralizedWfstLimits {
                    max_work_units_per_expansion: limit,
                    ..Default::default()
                },
            )
            .expect("valid grammar");
            assert_eq!(
                (wfst.source_width_slot_count, wfst.query_width_slot_count),
                (90, 1)
            );
            if limit < exact_work {
                assert!(wfst.try_transitions(0).is_err());
                assert_eq!(counts(&wfst), (1, 1, 0));
            } else {
                assert!(wfst
                    .try_transitions(0)
                    .expect("linear bounded expansion")
                    .is_empty());
            }
        }
    }

    #[test]
    fn complete_emission_chain_is_atomic_and_reused_after_cache_clear() {
        let dict = DynamicDawgChar::<()>::from_terms(["abcd"]);
        let operations = OperationSetBuilder::new()
            .with_operation(OperationType::new(4, 0, 1.0, "delete_four"))
            .build();
        let limits = GeneralizedWfstLimits {
            max_retained_wfst_states: 5,
            max_retained_dictionary_nodes: 5,
            ..Default::default()
        };
        let mut wfst =
            GeneralizedWfst::try_new_with_limits(&dict, "", 1, operations.clone(), limits)
                .expect("exact fitting limits");
        let first = wfst.try_transitions(0).expect("full chain").to_vec();
        assert_eq!(counts(&wfst), (5, 5, 1));
        let mut state = first[0].to;
        let mut last_chain = None;
        while let Some(RegisteredState::Emit(emit)) = wfst.registered_state(state) {
            if let Some(previous) = &last_chain {
                assert!(Arc::ptr_eq(previous, &emit.chain));
            }
            last_chain = Some(Arc::clone(&emit.chain));
            let next = emit.next;
            wfst.try_transitions(state)
                .expect("precommitted continuation");
            assert_eq!(wfst.num_states(), 5);
            state = next;
        }
        assert!(wfst.is_final(state));
        wfst.clear_cache();
        assert_eq!(wfst.try_transitions(0).expect("reuse")[0].to, first[0].to);
        assert_eq!(counts(&wfst), (5, 5, 1));

        let mut too_small = GeneralizedWfst::try_new_with_limits(
            &dict,
            "",
            1,
            operations,
            GeneralizedWfstLimits {
                max_retained_wfst_states: 4,
                ..limits
            },
        )
        .expect("valid constructor");
        for _ in 0..3 {
            assert!(too_small.try_transitions(0).is_err());
            assert_eq!(counts(&too_small), (1, 1, 0));
        }
    }

    #[test]
    fn exact_fit_chains_keep_their_language_under_every_cache_policy() {
        use lling_llang::wfst::CachePolicy;
        let dict = DynamicDawgChar::<()>::from_terms(["abcd"]);
        for policy in [
            CachePolicy::CacheAll,
            CachePolicy::NoCache,
            CachePolicy::Lru { max_states: 1 },
        ] {
            let mut wfst = GeneralizedWfst::try_new_with_limits(
                &dict,
                "",
                1,
                OperationSetBuilder::new()
                    .with_operation(OperationType::new(4, 0, 1.0, "delete_four"))
                    .build(),
                GeneralizedWfstLimits {
                    max_retained_wfst_states: 5,
                    max_retained_dictionary_nodes: 5,
                    ..Default::default()
                },
            )
            .expect("exact fit");
            wfst.set_cache_policy(policy);
            let mut previous_ids = None;
            for _ in 0..3 {
                let mut state = 0;
                let mut output = String::new();
                let mut cost = 0.0;
                let mut ids = vec![state];
                for _ in 0..4 {
                    let arcs = wfst.try_transitions(state).expect("complete chain");
                    assert_eq!(arcs.len(), 1);
                    assert_eq!(arcs[0].input, None);
                    output.push(arcs[0].output.expect("dictionary label"));
                    cost += arcs[0].weight.value();
                    state = arcs[0].to;
                    ids.push(state);
                }
                assert_eq!(output, "abcd");
                assert_eq!(cost, 1.0);
                assert!(wfst.is_final(state));
                assert_eq!(wfst.num_states(), 5);
                if let Some(previous) = &previous_ids {
                    assert_eq!(&ids, previous);
                }
                previous_ids = Some(ids);
                wfst.clear_cache();
            }
        }
    }

    #[test]
    fn missing_query_segments_and_empty_path_sets_are_cached_once() {
        let dict = DynamicDawgChar::<()>::from_terms(Vec::<&str>::new());
        for query_width in [0, 3] {
            let mut builder = OperationSetBuilder::new();
            for cost in 1..=3 {
                builder = builder.with_operation(OperationType::new(
                    2,
                    query_width,
                    f64::from(cost),
                    "same_width",
                ));
            }
            // Empty source: finality1 + slots2 + rules3 + iterator/next/pop3.
            // Missing query: finality1 + slots2 + rules3 + scalar scans6.
            let exact_work = if query_width == 0 { 9 } else { 12 };
            for work in [exact_work - 1, exact_work] {
                let mut wfst = GeneralizedWfst::try_new_with_limits(
                    &dict,
                    "",
                    3,
                    builder.clone().build(),
                    GeneralizedWfstLimits {
                        max_work_units_per_expansion: work,
                        ..Default::default()
                    },
                )
                .expect("valid grammar");
                if work == exact_work {
                    assert!(wfst.try_transitions(0).expect("cached absence").is_empty());
                } else {
                    assert!(wfst.try_transitions(0).is_err());
                    assert_eq!(counts(&wfst), (1, 1, 0));
                }
            }
        }
    }

    #[test]
    fn expansion_limits_leave_all_logical_registries_unchanged() {
        let dict = DynamicDawgChar::<()>::from_terms(["a", "b"]);
        let operations = OperationSetBuilder::new()
            .with_operation(OperationType::new(1, 1, 1.0, "any"))
            .build();
        let cases = [
            GeneralizedWfstLimits {
                max_retained_dictionary_nodes: 2,
                ..Default::default()
            },
            GeneralizedWfstLimits {
                max_retained_wfst_states: 2,
                ..Default::default()
            },
            GeneralizedWfstLimits {
                max_paths_per_expansion: 1,
                ..Default::default()
            },
            GeneralizedWfstLimits {
                max_work_units_per_expansion: 1,
                ..Default::default()
            },
        ];
        for limits in cases {
            let mut wfst =
                GeneralizedWfst::try_new_with_limits(&dict, "a", 1, operations.clone(), limits)
                    .expect("valid constructor");
            for _ in 0..3 {
                let error = wfst
                    .try_transitions(0)
                    .expect_err("expansion must exceed its bound");
                assert!(matches!(error, ExpansionError::Failure(ref failure)
                    if failure.kind() == lling_llang::prelude::ExpansionFailureKind::ResourceExhausted
                        && !failure.is_retryable()));
                assert_eq!(counts(&wfst), (1, 1, 0));
            }
        }
    }

    #[test]
    fn cancellation_and_late_source_failure_never_publish_staging() {
        let dict = DynamicDawgChar::<()>::from_terms(["abcd"]);
        let wfst = GeneralizedWfst::new(
            &dict,
            "",
            1,
            OperationSetBuilder::new()
                .with_operation(OperationType::new(4, 0, 1.0, "delete"))
                .build(),
        );
        let cancellation = CancellationToken::new();
        let calls = std::cell::Cell::new(0);
        let check = || {
            let count = calls.get() + 1;
            calls.set(count);
            if count == 5 {
                cancellation.cancel(lling_llang::prelude::CancellationReason::Requested);
            }
            Ok(())
        };
        assert!(matches!(
            wfst.compute_registered_state_with_check(0, Some(&cancellation), &check),
            StateExpansion::Cancelled(_)
        ));
        assert_eq!(counts(&wfst), (1, 1, 0));
        let calls = std::cell::Cell::new(0);
        let check = || {
            calls.set(calls.get() + 1);
            if calls.get() >= 5 {
                Err(ExpansionError::Failure(
                    ExpansionFailure::resource_exhausted("provider page failed"),
                ))
            } else {
                Ok(())
            }
        };
        assert!(matches!(
            wfst.compute_registered_state_with_check(0, None, &check),
            StateExpansion::Failed(_)
        ));
        assert_eq!(counts(&wfst), (1, 1, 0));
    }

    #[test]
    fn exact_fit_concurrent_clones_reconcile_shared_identities() {
        let dict = DynamicDawgChar::<()>::from_terms(["abcd"]);
        let wfst = GeneralizedWfst::try_new_with_limits(
            &dict,
            "",
            1,
            OperationSetBuilder::new()
                .with_operation(OperationType::new(4, 0, 1.0, "delete"))
                .build(),
            GeneralizedWfstLimits {
                max_retained_dictionary_nodes: 5,
                max_retained_wfst_states: 5,
                ..Default::default()
            },
        )
        .expect("exact limits");
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let mut workers = Vec::new();
        for _ in 0..2 {
            let wfst = wfst.clone();
            let barrier = Arc::clone(&barrier);
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                match wfst.expand_state(0) {
                    StateExpansion::Expanded { transitions, .. } => transitions[0].to,
                    result => panic!("unexpected expansion result: {result:?}"),
                }
            }));
        }
        let ids: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().expect("worker"))
            .collect();
        assert_eq!(ids[0], ids[1]);
        assert_eq!(counts(&wfst), (5, 5, 0));
    }
}
