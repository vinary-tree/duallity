//! WallBreaker WFST wrapper for large error bound matching.
//!
//! This module provides [`WallBreakerWfst`], a WFST wrapper for the WallBreaker
//! algorithm that overcomes the "wall effect" in similarity search with large
//! error bounds.
//!
//! # Overview
//!
//! Traditional Levenshtein automata traverse dictionaries left-to-right. With
//! error bound `k`, the first `k` steps must explore ALL prefixes up to length
//! `k` before any filtering occurs. This creates a "wall" that limits performance.
//!
//! WallBreaker overcomes this by:
//! 1. Splitting the query into pieces (pigeonhole principle)
//! 2. Finding exact substring matches using SCDAWG
//! 3. Extending bidirectionally from matches
//! 4. Verifying total distance
//!
//! # WFST Representation
//!
//! The WFST representation models WallBreaker as a lazy transducer where:
//! - **States**: Represent (piece_index, match_position, extension_state)
//! - **Transitions**: Character consumption with edit costs
//! - **Weights**: Edit distances as TropicalWeight
//!
//! # Example
//!
//! ```rust,no_run
//! use duallity::WallBreakerWfst;
//! use libdictenstein::scdawg::Scdawg;
//!
//! let dict = Scdawg::<()>::from_terms(vec!["cathedral", "category", "catering"]);
//! let wfst = WallBreakerWfst::new(&dict, "cathedrel", 2);
//!
//! // Use in WFST pipeline...
//! ```
//!
//! # Algorithm Selection
//!
//! Piece counts are algorithm-dependent (formally verified):
//! - **Standard**: k+1 pieces
//! - **Transposition**: 2k+1 pieces
//! - **MergeAndSplit**: 2k+1 pieces

use std::marker::PhantomData;

use lling_llang::prelude::{
    LazyState, LazyWfst, StateId, StateSource, TropicalWeight, WeightedTransition, Wfst,
};
use smallvec::SmallVec;

use libdictenstein::substring::{BidirectionalDictionaryNode, SubstringDictionary};
use libdictenstein::{Dictionary, DictionaryNode};
use liblevenshtein::transducer::Algorithm;
use liblevenshtein::wallbreaker::WallBreaker;

use crate::lazy_cache::{empty_char_transitions, CachedCharState, LazyStateCache};
use crate::wallbreaker_results::{
    empty_wallbreaker_result_final_weight, next_wallbreaker_char_position,
    next_wallbreaker_result_index, next_wallbreaker_state_id, normalize_wallbreaker_results,
    usize_from_state_id, usize_from_u32, wallbreaker_result_final_weights, ResultCharArena,
    WallBreakerStateKey, SUPER_START_RESULT_INDEX,
};

/// Default maximum cache size used when LRU policy delegates to the wrapper default.
const DEFAULT_MAX_CACHE_SIZE: usize = 100_000;

fn build_wallbreaker_state_index(result_chars: &ResultCharArena) -> Vec<WallBreakerStateKey> {
    let mut id_to_state = Vec::with_capacity(result_chars.state_count());
    id_to_state.push(WallBreakerStateKey {
        result_index: SUPER_START_RESULT_INDEX,
        char_position: 0,
    });

    for result_idx in 0..result_chars.len() {
        let Some(result_index) = next_wallbreaker_result_index(result_idx) else {
            break;
        };
        let Some(term_len) = result_chars.term_len(result_idx) else {
            break;
        };

        for pos in 1..=term_len {
            if next_wallbreaker_state_id(id_to_state.len()).is_none() {
                return id_to_state;
            }

            let Some(char_position) = next_wallbreaker_char_position(pos) else {
                return id_to_state;
            };

            id_to_state.push(WallBreakerStateKey {
                result_index,
                char_position,
            });
        }
    }

    id_to_state
}

/// WallBreaker WFST wrapper.
///
/// Exposes the WallBreaker algorithm as a WFST compatible with lling-llang
/// composition pipelines. This is particularly effective for large error bounds
/// where traditional automata hit the "wall effect".
///
/// # Architecture
///
/// The WFST lazily computes states by:
/// 1. Running WallBreaker query to get candidate matches
/// 2. Creating states for each (match, position) pair
/// 3. Generating transitions that trace through match terms
///
/// # Type Parameters
///
/// * `D` - Dictionary type implementing `SubstringDictionary` (typically SCDAWG)
#[derive(Clone)]
pub struct WallBreakerWfst<'a, D>
where
    D: Dictionary + SubstringDictionary + Clone + Send + Sync,
    D::Node: BidirectionalDictionaryNode,
    <D::Node as DictionaryNode>::Unit: Into<u32>,
{
    /// Phantom marker preserving the dictionary lifetime parameter.
    _dictionary: PhantomData<&'a D>,

    /// The query string.
    query: String,

    /// Maximum edit distance.
    max_distance: usize,

    /// Algorithm type.
    algorithm: Algorithm,

    /// Number of normalized query results.
    result_count: usize,

    /// Flat UTF-8 character arena for all result terms.
    result_chars: ResultCharArena,

    /// Precomputed representable final weights for each result.
    result_final_weights: Vec<Option<TropicalWeight>>,

    /// Precomputed final weight for the super-start state when the empty term matches.
    empty_result_final_weight: Option<TropicalWeight>,

    /// Dense state ID to state key mapping.
    id_to_state: Vec<WallBreakerStateKey>,

    /// Cached state computations.
    cache: LazyStateCache<CachedCharState>,
}

impl<'a, D> WallBreakerWfst<'a, D>
where
    D: Dictionary + SubstringDictionary + Clone + Send + Sync,
    D::Node: BidirectionalDictionaryNode,
    <D::Node as DictionaryNode>::Unit: Into<u32>,
{
    /// Create a new WallBreaker WFST.
    ///
    /// Uses the Standard algorithm by default.
    pub fn new(dictionary: &'a D, query: &str, max_distance: usize) -> Self {
        Self::with_algorithm(dictionary, query, max_distance, Algorithm::Standard)
    }

    /// Create a new WallBreaker WFST with specific algorithm.
    pub fn with_algorithm(
        dictionary: &'a D,
        query: &str,
        max_distance: usize,
        algorithm: Algorithm,
    ) -> Self {
        // Run WallBreaker query to get results
        let wb = WallBreaker::with_algorithm(dictionary, max_distance, algorithm);
        let results = normalize_wallbreaker_results(wb.query(query), max_distance);
        let result_count = results.len();
        let result_chars = ResultCharArena::from_results(&results);
        let result_final_weights = wallbreaker_result_final_weights(&results);
        let empty_result_final_weight =
            empty_wallbreaker_result_final_weight(&results, max_distance);
        let id_to_state = build_wallbreaker_state_index(&result_chars);

        Self {
            _dictionary: PhantomData,
            query: query.to_string(),
            max_distance,
            algorithm,
            result_count,
            result_chars,
            result_final_weights,
            empty_result_final_weight,
            id_to_state,
            cache: LazyStateCache::new(DEFAULT_MAX_CACHE_SIZE),
        }
    }

    #[inline]
    fn lookup_state(&self, key: WallBreakerStateKey) -> Option<StateId> {
        let id = if key.result_index == SUPER_START_RESULT_INDEX {
            (key.char_position == 0).then_some(0)
        } else {
            self.result_chars.state_id_for_position(
                usize_from_u32(key.result_index),
                usize_from_u32(key.char_position),
            )
        }?;

        (self.id_to_state.get(usize_from_state_id(id)).copied() == Some(key)).then_some(id)
    }

    /// Get the query string.
    #[inline]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Get the maximum distance.
    #[inline]
    pub fn max_distance(&self) -> usize {
        self.max_distance
    }

    /// Get the algorithm.
    #[inline]
    pub fn algorithm(&self) -> Algorithm {
        self.algorithm
    }

    /// Get the number of results found.
    #[inline]
    pub fn num_results(&self) -> usize {
        self.result_count
    }

    /// Set the maximum cache size used by `CachePolicy::Lru { max_states: 0 }`.
    pub fn set_max_cache_size(&mut self, size: usize) {
        self.cache.set_max_lru_states(size);
    }

    fn computed_state(&self, state: StateId) -> Option<&CachedCharState> {
        self.cache.get(state)
    }

    fn final_weight_for_state(&self, state: StateId) -> Option<TropicalWeight> {
        let key = self.id_to_state.get(usize_from_state_id(state))?;

        if key.result_index == SUPER_START_RESULT_INDEX {
            return self.empty_result_final_weight;
        }

        let result_index = usize_from_u32(key.result_index);
        let term_len = self.result_chars.term_len(result_index)?;
        let pos = usize_from_u32(key.char_position);
        if pos < term_len {
            return None;
        }

        self.result_final_weights
            .get(result_index)
            .copied()
            .flatten()
    }

    /// Ensure a state is computed and cached.
    fn ensure_state(&mut self, state_id: StateId) {
        if self.cache.touch_if_cached(state_id) {
            return;
        }

        let key = match self.id_to_state.get(usize_from_state_id(state_id)) {
            Some(key) => *key,
            None => return,
        };

        let (is_final, final_weight, transitions) = if key.result_index == SUPER_START_RESULT_INDEX
        {
            // Super-start state: transitions to start of each result
            self.compute_super_start_transitions()
        } else {
            self.compute_result_state(state_id, &key)
        };

        self.cache.insert(
            state_id,
            CachedCharState::new(is_final, final_weight, transitions),
        );
    }

    /// Compute transitions from the super-start state.
    fn compute_super_start_transitions(
        &self,
    ) -> (
        bool,
        TropicalWeight,
        SmallVec<[WeightedTransition<char, TropicalWeight>; 4]>,
    ) {
        let mut transitions = SmallVec::with_capacity(self.result_chars.start_transition_count());

        // Create transitions to the start of each result
        for result_idx in 0..self.result_chars.len() {
            let Some(first_char) = self.result_chars.first_char(result_idx) else {
                continue;
            };
            let Some(result_index) = next_wallbreaker_result_index(result_idx) else {
                continue;
            };
            let new_key = WallBreakerStateKey {
                result_index,
                char_position: 1,
            };
            let Some(new_id) = self.lookup_state(new_key) else {
                continue;
            };

            transitions.push(WeightedTransition::new(
                0,
                Some(first_char),
                Some(first_char),
                new_id,
                TropicalWeight::new(0.0),
            ));
        }

        let (is_final, final_weight) = match self.empty_result_final_weight {
            Some(weight) => (true, weight),
            None => (false, TropicalWeight::infinity()),
        };

        (is_final, final_weight, transitions)
    }

    /// Compute state for a result position.
    fn compute_result_state(
        &self,
        state_id: StateId,
        key: &WallBreakerStateKey,
    ) -> (
        bool,
        TropicalWeight,
        SmallVec<[WeightedTransition<char, TropicalWeight>; 4]>,
    ) {
        // Extract info upfront to avoid borrow conflicts
        let result_index = usize_from_u32(key.result_index);
        let Some(term_len) = self.result_chars.term_len(result_index) else {
            return (false, TropicalWeight::infinity(), SmallVec::new());
        };
        let Some(final_result_weight) = self
            .result_final_weights
            .get(result_index)
            .copied()
            .flatten()
        else {
            return (false, TropicalWeight::infinity(), SmallVec::new());
        };

        let pos = usize_from_u32(key.char_position);
        let next_char = self.result_chars.char_at(result_index, pos);

        // Check if we're at the end of the term
        let is_final = pos >= term_len;
        let final_weight = if is_final {
            final_result_weight
        } else {
            TropicalWeight::infinity()
        };

        let mut transitions = SmallVec::with_capacity(usize::from(!is_final));

        // Add transition to next character if not at end
        if let Some(next_char) = next_char {
            let Some(char_position) = next_wallbreaker_char_position(pos.saturating_add(1)) else {
                return (is_final, final_weight, transitions);
            };
            let new_key = WallBreakerStateKey {
                result_index: key.result_index,
                char_position,
            };
            let Some(new_id) = self.lookup_state(new_key) else {
                return (is_final, final_weight, transitions);
            };

            transitions.push(WeightedTransition::new(
                state_id,
                Some(next_char),
                Some(next_char),
                new_id,
                TropicalWeight::new(0.0),
            ));
        }

        (is_final, final_weight, transitions)
    }
}

impl<'a, D> Wfst<char, TropicalWeight> for WallBreakerWfst<'a, D>
where
    D: Dictionary + SubstringDictionary + Clone + Send + Sync,
    D::Node: BidirectionalDictionaryNode,
    <D::Node as DictionaryNode>::Unit: Into<u32>,
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
        self.id_to_state.len()
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.id_to_state.is_empty()
    }

    #[inline]
    fn is_valid_state(&self, state: StateId) -> bool {
        self.id_to_state.get(usize_from_state_id(state)).is_some()
    }
}

impl<'a, D> LazyWfst<char, TropicalWeight> for WallBreakerWfst<'a, D>
where
    D: Dictionary + SubstringDictionary + Clone + Send + Sync,
    D::Node: BidirectionalDictionaryNode,
    <D::Node as DictionaryNode>::Unit: Into<u32>,
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

impl<'a, D> StateSource<char, TropicalWeight> for WallBreakerWfst<'a, D>
where
    D: Dictionary + SubstringDictionary + Clone + Send + Sync,
    D::Node: BidirectionalDictionaryNode,
    <D::Node as DictionaryNode>::Unit: Into<u32>,
{
    fn compute_state(&self, state: StateId) -> LazyState<char, TropicalWeight> {
        let Some(key) = self.id_to_state.get(usize_from_state_id(state)).copied() else {
            return LazyState::non_final(SmallVec::new());
        };

        let (is_final, final_weight, transitions) = if key.result_index == SUPER_START_RESULT_INDEX
        {
            self.compute_super_start_transitions()
        } else {
            self.compute_result_state(state, &key)
        };

        if is_final {
            LazyState::final_state(final_weight, transitions)
        } else {
            LazyState::non_final(transitions)
        }
    }

    fn start(&self) -> StateId {
        0
    }

    fn num_states_hint(&self) -> Option<usize> {
        Some(self.id_to_state.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libdictenstein::scdawg::Scdawg;

    #[test]
    fn test_wallbreaker_result_state_with_unrepresentable_weight_is_non_final() {
        let dict = Scdawg::<()>::from_terms(vec!["a"]);
        let mut wfst = WallBreakerWfst::new(&dict, "a", 0);

        assert_eq!(wfst.num_results(), 1);
        wfst.result_final_weights[0] = None;

        let terminal_state = wfst
            .lookup_state(WallBreakerStateKey {
                result_index: 0,
                char_position: 1,
            })
            .expect("single-character result path should register terminal state");

        wfst.expand(terminal_state);

        assert!(!wfst.is_final(terminal_state));
        assert!(wfst.final_weight(terminal_state).value().is_infinite());
    }

    #[test]
    fn test_wallbreaker_caches_utf8_result_chars_for_state_hint() {
        let dict = Scdawg::<()>::from_terms(vec!["é"]);
        let wfst = WallBreakerWfst::new(&dict, "é", 0);

        assert_eq!(wfst.num_results(), 1);
        assert_eq!(wfst.result_chars.chars, vec!['é']);
        assert_eq!(wfst.result_chars.first_char(0), Some('é'));
        assert_eq!(
            StateSource::<char, TropicalWeight>::num_states_hint(&wfst),
            Some(2)
        );
    }

    #[test]
    fn test_wallbreaker_state_index_reserves_initial_state_space() {
        let dict = Scdawg::<()>::from_terms(vec!["hello", "help"]);
        let wfst = WallBreakerWfst::new(&dict, "helo", 2);
        let expected_states = wfst.result_chars.state_count();

        assert_eq!(wfst.id_to_state.len(), expected_states);
        assert!(wfst.id_to_state.capacity() >= expected_states);
        assert_eq!(
            wfst.lookup_state(WallBreakerStateKey {
                result_index: 0,
                char_position: 1,
            }),
            Some(1)
        );
    }

    #[test]
    fn test_wallbreaker_result_count_tracks_compiled_result_tables() {
        let dict = Scdawg::<()>::from_terms(vec!["hello", "help"]);
        let wfst = WallBreakerWfst::new(&dict, "helo", 2);

        assert_eq!(wfst.num_results(), wfst.result_count);
        assert_eq!(wfst.result_count, wfst.result_chars.len());
        assert_eq!(wfst.result_final_weights.len(), wfst.result_count);
    }

    #[test]
    fn test_wallbreaker_state_hint_uses_registered_states() {
        let dict = Scdawg::<()>::from_terms(vec!["hello", "help"]);
        let mut wfst = WallBreakerWfst::new(&dict, "helo", 2);
        let registered_states = wfst.id_to_state.len();

        wfst.result_chars.state_count = registered_states.saturating_add(10);

        assert_eq!(
            StateSource::<char, TropicalWeight>::num_states_hint(&wfst),
            Some(registered_states)
        );
    }
}
