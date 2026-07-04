//! UniversalLevenshteinWfst wrapper for lling-llang Wfst trait.
//!
//! This module provides [`UniversalLevenshteinWfst`], a wrapper that exposes a
//! Universal Levenshtein transducer as a lling-llang `Wfst<char, TropicalWeight>`.
//!
//! # Key Benefits over Parameterized WFST
//!
//! - **Precomputation**: The automaton structure is query-agnostic and can be
//!   precomputed once for a given max_distance
//! - **State Deduplication**: Uses a registry to deduplicate universal states
//! - **Variant Support**: Supports Standard, Transposition, and MergeAndSplit variants

use lling_llang::prelude::{
    LazyWfst, Semiring, StateId, StateSource, TropicalWeight, WeightedTransition, Wfst,
};

use libdictenstein::{Dictionary, DictionaryNode};
use liblevenshtein::transducer::universal::PositionVariant;

use crate::lazy_cache::{
    empty_char_transitions, ensure_cached_char_state, CachedCharState, LazyStateCache,
};
use crate::universal_state_source::UniversalLevenshteinStateSource;

/// A Universal Levenshtein transducer exposed as a lling-llang WFST.
///
/// This wrapper presents the product of a dictionary and Universal Levenshtein
/// automaton as a weighted finite state transducer with:
/// - **Input labels**: Query characters (the misspelled input)
/// - **Output labels**: Dictionary characters (the corrections)
/// - **Weights**: Edit distances as `TropicalWeight` (lower is better)
///
/// # Type Parameters
///
/// - `V`: Position variant (Standard, Transposition, or MergeAndSplit)
/// - `D`: Dictionary type implementing [`Dictionary`] with `char` units
///
/// # Example
///
/// ```rust,no_run
/// use duallity::UniversalLevenshteinWfst;
/// use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
/// use liblevenshtein::transducer::universal::Standard;
/// use lling_llang::prelude::*;
///
/// let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help", "world"]);
/// let lev_wfst = UniversalLevenshteinWfst::<Standard, _>::new(&dict, "helo", 2);
///
/// // Use with lling-llang's composition
/// // let composed = compose(lev_wfst, other_wfst);
/// ```
#[derive(Clone)]
pub struct UniversalLevenshteinWfst<V, D>
where
    V: PositionVariant + Clone + Send + Sync,
    V::State: Send + Sync,
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
{
    /// The state source for computing transitions
    state_source: UniversalLevenshteinStateSource<V, D>,
    /// Cached states (state_id -> computed state info)
    cache: LazyStateCache<CachedCharState>,
    /// Maximum edit distance
    max_distance: u8,
}

/// Default maximum cache size for LRU policy (100,000 states)
const DEFAULT_MAX_CACHE_SIZE: usize = 100_000;

impl<V, D> UniversalLevenshteinWfst<V, D>
where
    V: PositionVariant + Clone + Send + Sync,
    V::State: Send + Sync,
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
{
    /// Create a new Universal Levenshtein WFST for the given query and max distance.
    ///
    /// # Arguments
    ///
    /// - `dictionary`: The dictionary to search
    /// - `query`: The query string to find corrections for
    /// - `max_distance`: Maximum edit distance for matches
    ///
    /// # Returns
    ///
    /// A new `UniversalLevenshteinWfst` ready for composition or traversal.
    pub fn new(dictionary: &D, query: &str, max_distance: u8) -> Self {
        let state_source = UniversalLevenshteinStateSource::new(dictionary, query, max_distance);

        Self {
            state_source,
            cache: LazyStateCache::new(DEFAULT_MAX_CACHE_SIZE),
            max_distance,
        }
    }

    /// Get the maximum edit distance.
    pub fn max_distance(&self) -> u8 {
        self.max_distance
    }

    /// Get the query string.
    pub fn query(&self) -> &str {
        self.state_source.query_str()
    }

    /// Set the maximum cache size for LRU eviction.
    pub fn set_max_cache_size(&mut self, size: usize) {
        self.cache.set_max_lru_states(size);
    }

    fn computed_state(&self, state: StateId) -> Option<&CachedCharState> {
        self.cache.get(state)
    }

    /// Ensure a state is computed and cached.
    fn ensure_state(&mut self, state: StateId) {
        ensure_cached_char_state(
            &mut self.cache,
            &self.state_source,
            state,
            UniversalLevenshteinStateSource::is_valid_product_state,
        );
    }
}

impl<V, D> Wfst<char, TropicalWeight> for UniversalLevenshteinWfst<V, D>
where
    V: PositionVariant + Clone + Send + Sync,
    V::State: Send + Sync,
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
{
    fn start(&self) -> StateId {
        self.state_source.start()
    }

    fn is_final(&self, state: StateId) -> bool {
        self.computed_state(state)
            .map(|s| s.is_final)
            .unwrap_or_else(|| self.state_source.final_weight_for_state(state).is_some())
    }

    fn final_weight(&self, state: StateId) -> TropicalWeight {
        self.computed_state(state)
            .map(|s| s.final_weight)
            .unwrap_or_else(|| {
                self.state_source
                    .final_weight_for_state(state)
                    .unwrap_or_else(TropicalWeight::zero)
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
        self.state_source
            .num_states_hint()
            .unwrap_or(0)
            .max(self.state_source.registered_state_id_span())
    }

    #[inline]
    fn is_empty(&self) -> bool {
        false
    }

    #[inline]
    fn is_valid_state(&self, state: StateId) -> bool {
        self.state_source.is_valid_product_state(state)
    }
}

impl<V, D> LazyWfst<char, TropicalWeight> for UniversalLevenshteinWfst<V, D>
where
    V: PositionVariant + Clone + Send + Sync,
    V::State: Send + Sync,
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
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

/// A pre-bound Universal WFST that can be cloned efficiently.
///
/// This is useful when you want to create multiple queries against the same
/// dictionary with the same automaton variant.
pub struct BoundUniversalWfst<V, D>
where
    V: PositionVariant + Clone + Send + Sync,
    V::State: Send + Sync,
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
{
    dictionary: D,
    max_distance: u8,
    _phantom: std::marker::PhantomData<V>,
}

impl<V, D> BoundUniversalWfst<V, D>
where
    V: PositionVariant + Clone + Send + Sync,
    V::State: Send + Sync,
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
{
    /// Create a new bound universal WFST builder.
    pub fn new(dictionary: D, max_distance: u8) -> Self {
        Self {
            dictionary,
            max_distance,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Create a WFST for a specific query.
    pub fn with_query(&self, query: &str) -> UniversalLevenshteinWfst<V, D> {
        UniversalLevenshteinWfst::new(&self.dictionary, query, self.max_distance)
    }

    /// Get the maximum edit distance.
    pub fn max_distance(&self) -> u8 {
        self.max_distance
    }
}

impl<V, D> Clone for BoundUniversalWfst<V, D>
where
    V: PositionVariant + Clone + Send + Sync,
    V::State: Send + Sync,
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
{
    fn clone(&self) -> Self {
        Self {
            dictionary: self.dictionary.clone(),
            max_distance: self.max_distance,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
    use liblevenshtein::transducer::universal::Standard;

    #[test]
    fn test_universal_levenshtein_wfst_creation() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help", "world"]);
        let wfst = UniversalLevenshteinWfst::<Standard, _>::new(&dict, "helo", 2);

        assert_eq!(wfst.max_distance(), 2);
        assert_eq!(wfst.query(), "helo");
        let first = wfst.query();
        let second = wfst.query();
        assert!(std::ptr::eq(first.as_ptr(), second.as_ptr()));
    }

    #[test]
    fn test_universal_levenshtein_wfst_start_state() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help", "world"]);
        let wfst = UniversalLevenshteinWfst::<Standard, _>::new(&dict, "helo", 2);

        let start = wfst.start();
        assert_eq!(start, 0);
    }

    #[test]
    fn test_universal_levenshtein_wfst_expand_state() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help"]);
        let mut wfst = UniversalLevenshteinWfst::<Standard, _>::new(&dict, "helo", 2);

        let start = wfst.start();
        assert!(!wfst.is_expanded(start));

        wfst.expand(start);
        assert!(wfst.is_expanded(start));
        assert!(wfst.computed_states() >= 1);
    }

    #[test]
    fn test_universal_levenshtein_wfst_tracks_exact_query_position() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["ab"]);
        let mut wfst = UniversalLevenshteinWfst::<Standard, _>::new(&dict, "xy", 2);

        let start = wfst.start();
        let first_target = wfst
            .transitions_lazy(start)
            .iter()
            .find(|transition| {
                transition.input == Some('x')
                    && transition.output == Some('a')
                    && transition.weight.value() == 0.0
            })
            .map(|transition| transition.to)
            .expect("expected first substitution transition");

        let second_transitions = wfst.transitions_lazy(first_target);
        assert!(
            second_transitions.iter().any(|transition| {
                transition.input == Some('y') && transition.output == Some('b')
            }),
            "second dictionary edge must consume the second query character"
        );
        assert!(
            !second_transitions.iter().any(|transition| {
                transition.input == Some('x') && transition.output == Some('b')
            }),
            "query position must not be recovered from abstract offsets"
        );
    }

    #[test]
    fn test_bound_universal_wfst() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help", "world"]);
        let bound = BoundUniversalWfst::<Standard, _>::new(dict, 2);

        let wfst1 = bound.with_query("helo");
        let wfst2 = bound.with_query("wrld");

        assert_eq!(wfst1.query(), "helo");
        assert_eq!(wfst2.query(), "wrld");
        assert_eq!(wfst1.max_distance(), wfst2.max_distance());
    }

    #[test]
    fn test_universal_levenshtein_wfst_cache_policy() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
        let mut wfst = UniversalLevenshteinWfst::<Standard, _>::new(&dict, "test", 1);

        assert!(matches!(
            wfst.cache_policy(),
            lling_llang::wfst::CachePolicy::CacheAll
        ));

        wfst.set_cache_policy(lling_llang::wfst::CachePolicy::Lru { max_states: 1000 });
        assert!(matches!(
            wfst.cache_policy(),
            lling_llang::wfst::CachePolicy::Lru { .. }
        ));
    }

    #[test]
    fn test_universal_levenshtein_wfst_rejects_invalid_state_without_caching() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
        let mut wfst = UniversalLevenshteinWfst::<Standard, _>::new(&dict, "test", 1);
        let invalid_state = u32::MAX;

        assert!(!wfst.is_valid_state(invalid_state));
        wfst.expand(invalid_state);

        assert_eq!(wfst.computed_states(), 0);
        assert!(wfst.transitions_lazy(invalid_state).is_empty());
        assert_eq!(wfst.computed_states(), 0);
    }

    #[test]
    fn test_universal_levenshtein_wfst_no_cache_policy_uses_scratch_only() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
        let mut wfst = UniversalLevenshteinWfst::<Standard, _>::new(&dict, "test", 1);
        wfst.set_cache_policy(lling_llang::wfst::CachePolicy::NoCache);

        let start = wfst.start();
        let transition_count = wfst.transitions_lazy(start).len();

        assert!(transition_count > 0);
        assert_eq!(wfst.computed_states(), 0);
        assert!(!wfst.is_expanded(start));
        assert_eq!(wfst.transitions(start).len(), transition_count);
        assert_eq!(wfst.total_transitions(), transition_count);
    }

    #[test]
    fn test_universal_levenshtein_wfst_lru_policy_evicts_least_recently_used_state() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
        let mut wfst = UniversalLevenshteinWfst::<Standard, _>::new(&dict, "test", 1);
        wfst.set_cache_policy(lling_llang::wfst::CachePolicy::Lru { max_states: 1 });

        let start = wfst.start();
        let next = wfst
            .transitions_lazy(start)
            .first()
            .expect("expected start transition")
            .to;

        assert!(wfst.is_expanded(start));
        assert_eq!(wfst.computed_states(), 1);

        wfst.expand(next);

        assert_eq!(wfst.computed_states(), 1);
        assert!(!wfst.is_expanded(start));
        assert!(wfst.is_expanded(next));
    }

    #[test]
    fn test_universal_wfst_transposition_variant() {
        use liblevenshtein::transducer::universal::Transposition;

        let dict = DynamicDawgChar::<()>::from_terms(vec!["test", "tset"]);
        let wfst = UniversalLevenshteinWfst::<Transposition, _>::new(&dict, "tset", 1);

        assert_eq!(wfst.max_distance(), 1);
        assert_eq!(wfst.query(), "tset");
    }

    #[test]
    fn test_universal_wfst_merge_and_split_variant() {
        use liblevenshtein::transducer::universal::MergeAndSplit;

        let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "helo"]);
        let wfst = UniversalLevenshteinWfst::<MergeAndSplit, _>::new(&dict, "helo", 1);

        assert_eq!(wfst.max_distance(), 1);
        assert_eq!(wfst.query(), "helo");
    }
}
