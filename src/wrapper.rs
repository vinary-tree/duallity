//! LevenshteinWfst wrapper for lling-llang Wfst trait.
//!
//! This module provides [`LevenshteinWfst`], a wrapper that exposes a
//! Levenshtein transducer as a lling-llang `Wfst<char, TropicalWeight>`.
//!
//! # UTF-8 Support
//!
//! This implementation properly handles UTF-8 characters. Each Unicode
//! character (not byte) counts as one unit for edit distance calculation.

use lling_llang::prelude::{
    LazyWfst, Semiring, StateId, StateSource, TropicalWeight, WeightedTransition, Wfst,
};

use libdictenstein::{Dictionary, DictionaryNode};
use liblevenshtein::transducer::Algorithm;

use crate::lazy_cache::{
    empty_char_transitions, ensure_cached_char_state, CachedCharState, LazyStateCache,
};
use crate::state_source::LevenshteinStateSource;

/// A Levenshtein transducer exposed as a lling-llang WFST.
///
/// This wrapper presents the product of a dictionary and Levenshtein automaton
/// as a weighted finite state transducer with:
/// - **Input labels**: Query characters (the misspelled input)
/// - **Output labels**: Dictionary characters (the corrections)
/// - **Weights**: Edit distances as `TropicalWeight` (lower is better)
///
/// The product state space (dictionary_node × automaton_state) is encoded
/// into a single `StateId` for compatibility with lling-llang's interface.
///
/// # Type Parameters
///
/// - `D`: Dictionary type implementing [`Dictionary`] with `char` units
///
/// # Example
///
/// ```rust,no_run
/// use duallity::LevenshteinWfst;
/// use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
/// use lling_llang::prelude::*;
///
/// let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help", "world"]);
/// let lev_wfst = LevenshteinWfst::new(&dict, "helo", 2);
///
/// // Use with lling-llang's composition
/// // let composed = compose(lev_wfst, other_wfst);
/// ```
#[derive(Clone)]
pub struct LevenshteinWfst<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
{
    /// The state source for computing transitions
    state_source: LevenshteinStateSource<D>,
    /// Cached states (state_id -> computed state info)
    cache: LazyStateCache<CachedCharState>,
    /// Maximum edit distance
    max_distance: usize,
    /// Algorithm variant
    algorithm: Algorithm,
}

/// Default maximum cache size for LRU policy (100,000 states)
const DEFAULT_MAX_CACHE_SIZE: usize = 100_000;

impl<D> LevenshteinWfst<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
{
    /// Create a new Levenshtein WFST for the given query and max distance.
    ///
    /// # Arguments
    ///
    /// - `dictionary`: The dictionary to search
    /// - `query`: The query string to find corrections for
    /// - `max_distance`: Maximum edit distance for matches
    ///
    /// # Returns
    ///
    /// A new `LevenshteinWfst` ready for composition or traversal.
    ///
    /// # UTF-8 Support
    ///
    /// The query string is properly handled as UTF-8 characters.
    pub fn new(dictionary: &D, query: &str, max_distance: usize) -> Self {
        Self::with_algorithm(dictionary, query, max_distance, Algorithm::Standard)
    }

    /// Create a new Levenshtein WFST with a specific algorithm.
    ///
    /// # Arguments
    ///
    /// - `dictionary`: The dictionary to search
    /// - `query`: The query string to find corrections for
    /// - `max_distance`: Maximum edit distance for matches
    /// - `algorithm`: The Levenshtein algorithm variant to use
    pub fn with_algorithm(
        dictionary: &D,
        query: &str,
        max_distance: usize,
        algorithm: Algorithm,
    ) -> Self {
        let state_source =
            LevenshteinStateSource::with_algorithm(dictionary, query, max_distance, algorithm);

        Self {
            state_source,
            cache: LazyStateCache::new(DEFAULT_MAX_CACHE_SIZE),
            max_distance,
            algorithm,
        }
    }

    /// Get the maximum edit distance.
    pub fn max_distance(&self) -> usize {
        self.max_distance
    }

    /// Get the algorithm being used.
    pub fn algorithm(&self) -> Algorithm {
        self.algorithm
    }

    /// Get the query string.
    pub fn query(&self) -> &str {
        self.state_source.query_str()
    }

    /// Set the maximum cache size for LRU eviction.
    ///
    /// Only takes effect when cache policy is set to LRU.
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
            LevenshteinStateSource::is_valid_product_state,
        );
    }
}

// We implement Wfst via LazyWfstWrapper + StateSource pattern
// This provides proper lazy evaluation with caching

impl<D> Wfst<char, TropicalWeight> for LevenshteinWfst<D>
where
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

    /// For lazy WFSTs, is_empty() returns false if there's a valid start state.
    #[inline]
    fn is_empty(&self) -> bool {
        // A Levenshtein WFST always has at least a start state
        false
    }

    /// For lazy WFSTs, any encoded state is potentially valid.
    /// The actual validity is determined when the state is expanded.
    #[inline]
    fn is_valid_state(&self, state: StateId) -> bool {
        self.state_source.is_valid_product_state(state)
    }
}

impl<D> LazyWfst<char, TropicalWeight> for LevenshteinWfst<D>
where
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

#[cfg(test)]
mod tests {
    use super::*;
    use libdictenstein::dynamic_dawg::char::DynamicDawgChar;

    #[test]
    fn test_levenshtein_wfst_creation() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help", "world"]);
        let wfst = LevenshteinWfst::new(&dict, "helo", 2);

        assert_eq!(wfst.max_distance(), 2);
        assert_eq!(wfst.algorithm(), Algorithm::Standard);
        assert_eq!(wfst.query(), "helo");
        let first = wfst.query();
        let second = wfst.query();
        assert!(std::ptr::eq(first.as_ptr(), second.as_ptr()));
    }

    #[test]
    fn test_levenshtein_wfst_start_state() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help", "world"]);
        let wfst = LevenshteinWfst::new(&dict, "helo", 2);

        // Start state should be (0, 0) encoded
        let start = wfst.start();
        assert_eq!(start, 0);
    }

    #[test]
    fn test_levenshtein_wfst_with_algorithm() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
        let wfst = LevenshteinWfst::with_algorithm(&dict, "tset", 2, Algorithm::Transposition);

        assert_eq!(wfst.algorithm(), Algorithm::Transposition);
    }

    #[test]
    fn test_levenshtein_wfst_transposition_reaches_final_state() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["ab"]);
        let mut wfst = LevenshteinWfst::with_algorithm(&dict, "ba", 1, Algorithm::Transposition);

        let start = wfst.start();
        let first_targets = wfst
            .transitions_lazy(start)
            .iter()
            .filter(|transition| {
                transition.input == Some('b')
                    && transition.output == Some('a')
                    && (transition.weight.value() - 1.0).abs() <= f64::EPSILON
            })
            .map(|transition| transition.to)
            .collect::<Vec<_>>();

        let mut reaches_final = false;

        for first_target in first_targets {
            let second_targets = wfst
                .transitions_lazy(first_target)
                .iter()
                .filter(|transition| {
                    transition.input == Some('a')
                        && transition.output == Some('b')
                        && transition.weight.value() == 0.0
                })
                .map(|transition| transition.to)
                .collect::<Vec<_>>();

            for second_target in second_targets {
                wfst.expand(second_target);
                reaches_final |=
                    wfst.is_final(second_target) && wfst.final_weight(second_target).value() == 0.0;
            }
        }

        assert!(reaches_final, "adjacent swap should accept at distance 1");
    }

    #[test]
    fn test_levenshtein_wfst_merge_and_split_reaches_final_state() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["m"]);
        let mut wfst = LevenshteinWfst::with_algorithm(&dict, "rn", 1, Algorithm::MergeAndSplit);

        let start = wfst.start();
        let first_targets = wfst
            .transitions_lazy(start)
            .iter()
            .filter(|transition| {
                transition.input == Some('r')
                    && transition.output == Some('m')
                    && (transition.weight.value() - 1.0).abs() <= f64::EPSILON
            })
            .map(|transition| transition.to)
            .collect::<Vec<_>>();

        let mut reaches_final = false;

        for first_target in first_targets {
            let second_targets = wfst
                .transitions_lazy(first_target)
                .iter()
                .filter(|transition| {
                    transition.input == Some('n')
                        && transition.output.is_none()
                        && transition.weight.value() == 0.0
                })
                .map(|transition| transition.to)
                .collect::<Vec<_>>();

            for second_target in second_targets {
                wfst.expand(second_target);
                reaches_final |=
                    wfst.is_final(second_target) && wfst.final_weight(second_target).value() == 0.0;
            }
        }

        assert!(reaches_final, "merge should accept at distance 1");
    }

    #[test]
    fn test_levenshtein_wfst_expand_state() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help"]);
        let mut wfst = LevenshteinWfst::new(&dict, "helo", 2);

        // Initially no states are expanded
        let start = wfst.start();
        assert!(!wfst.is_expanded(start));

        // Expand the start state
        wfst.expand(start);
        assert!(wfst.is_expanded(start));

        // Should have some transitions from start state
        let transitions = wfst.transitions(start);
        assert!(!transitions.is_empty());
    }

    #[test]
    fn test_levenshtein_wfst_utf8_support() {
        // Test with non-ASCII characters
        let dict = DynamicDawgChar::<()>::from_terms(vec!["café", "naïve", "北京"]);
        let mut wfst = LevenshteinWfst::new(&dict, "cafe", 1);

        let start = wfst.start();
        wfst.expand(start);

        // Should handle UTF-8 properly
        assert!(wfst.computed_states() > 0);
    }

    #[test]
    fn test_levenshtein_wfst_cache_policy() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
        let mut wfst = LevenshteinWfst::new(&dict, "test", 1);

        // Default should be CacheAll
        assert!(matches!(
            wfst.cache_policy(),
            lling_llang::wfst::CachePolicy::CacheAll
        ));

        // Can change to LRU
        wfst.set_cache_policy(lling_llang::wfst::CachePolicy::Lru { max_states: 1000 });
        assert!(matches!(
            wfst.cache_policy(),
            lling_llang::wfst::CachePolicy::Lru { .. }
        ));
    }

    #[test]
    fn test_levenshtein_wfst_rejects_invalid_state_without_caching() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
        let mut wfst = LevenshteinWfst::new(&dict, "test", 1);
        let invalid_state = u32::MAX;

        assert!(!wfst.is_valid_state(invalid_state));
        wfst.expand(invalid_state);

        assert_eq!(wfst.computed_states(), 0);
        assert!(wfst.transitions_lazy(invalid_state).is_empty());
        assert_eq!(wfst.computed_states(), 0);
    }

    #[test]
    fn test_levenshtein_wfst_no_cache_policy_uses_scratch_only() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
        let mut wfst = LevenshteinWfst::new(&dict, "test", 1);
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
    fn test_levenshtein_wfst_lru_policy_evicts_least_recently_used_state() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
        let mut wfst = LevenshteinWfst::new(&dict, "test", 1);
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
}
