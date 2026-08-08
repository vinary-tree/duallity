//! # duallity — Levenshtein automata as lling-llang WFSTs.
//!
//! This crate exposes liblevenshtein's Levenshtein automata as lling-llang
//! `Wfst` implementers, enabling participation in WFST composition pipelines.
//!
//! # Overview
//!
//! The integration provides:
//!
//! - [`LevenshteinWfst`]: A lazy WFST wrapper that presents the Levenshtein
//!   transducer × dictionary product as a `Wfst<char, TropicalWeight>`.
//!   States encode (dictionary_node, automaton_state) pairs.
//!
//! - [`LevenshteinStateSource`]: A [`StateSource`] implementation for lazy
//!   state computation, enabling efficient composition without materializing
//!   the full product state space.
//!
//! - [`DictionaryBackend`]: An adapter that implements lling-llang's
//!   `LatticeBackend` trait using a libdictenstein dictionary.
//!
//! # Architecture
//!
//! The Levenshtein transducer produces a weighted transducer where:
//! - **Input**: Query characters (the misspelled word)
//! - **Output**: Dictionary characters (corrections)
//! - **Weight**: Edit distance as `TropicalWeight` (min-plus semiring)
//!
//! State encoding uses a composite ID: `state_id = dict_node * automaton_size + automaton_state`
//!
//! # Example
//!
//! ```rust,no_run
//! use duallity::{LevenshteinWfst, DictionaryBackend};
//! use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
//! use lling_llang::prelude::*;
//!
//! // Build a dictionary
//! let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help", "world"]);
//!
//! // Create a Levenshtein WFST for the query "helo" with max distance 2
//! let mut lev_wfst = LevenshteinWfst::new(&dict, "helo", 2);
//!
//! // Expand lazily through the lling-llang WFST interface
//! let start = lev_wfst.start();
//! let transitions = lev_wfst.transitions_lazy(start);
//! assert!(!transitions.is_empty());
//! ```
//!
//! # Phonetic variants
//!
//! Phonetic Levenshtein WFST variants are behind the `phonetic-rules` feature:
//!
//! ```toml
//! [dependencies]
//! duallity = { version = "0.3", features = ["phonetic-rules"] }
//! ```

mod backend;
#[cfg(feature = "bindings-core")]
pub mod bindings;
mod lazy_cache;
mod node_key;
mod node_registry;
mod state_source;
mod state_source_support;
mod universal_state_source;
mod universal_state_support;
mod universal_wrapper;
mod wrapper;

// Phonetic WFST modules
mod composed_phonetic;
#[cfg(feature = "phonetic-rules")]
mod phonetic_anchors;
#[cfg(feature = "phonetic-rules")]
mod phonetic_nfa_support;
#[cfg(feature = "phonetic-rules")]
mod phonetic_nfa_wfst;
mod phonetic_rewrite_support;
mod phonetic_rewrite_wfst;
#[cfg(feature = "phonetic-rules")]
mod phonetic_state_source;
#[cfg(feature = "phonetic-rules")]
mod phonetic_state_support;
#[cfg(feature = "phonetic-rules")]
mod phonetic_wfst;

// Generalized and WallBreaker WFST modules
mod generalized_builder;
mod generalized_ops;
mod generalized_state_support;
mod generalized_wfst;
mod wallbreaker_builder;
mod wallbreaker_results;
mod wallbreaker_wfst;

// fzf max-plus scoring and dictionary adapters
mod fzf_scorer;
mod fzf_state_source;
mod fzf_state_support;
mod fzf_support;
mod fzf_wfst;

pub use backend::DictionaryBackend;
pub use state_source::LevenshteinStateSource;
pub use universal_state_source::UniversalLevenshteinStateSource;
pub use universal_state_support::UniversalStateRegistry;
pub use universal_wrapper::{BoundUniversalWfst, UniversalLevenshteinWfst};
pub use wrapper::LevenshteinWfst;

// Phonetic WFST exports
pub use composed_phonetic::{PhoneticMatch, PhoneticPipelineBuilder, PhoneticPipelineConfig};
#[cfg(feature = "phonetic-rules")]
pub use phonetic_nfa_wfst::PhoneticNfaWfst;
pub use phonetic_rewrite_wfst::{CommonPhoneticRules, RewriteRule, RewriteWfst};
#[cfg(feature = "phonetic-rules")]
pub use phonetic_state_source::PhoneticStateSource;
#[cfg(feature = "phonetic-rules")]
pub use phonetic_wfst::{PhoneticWfst, PhoneticWfstBuilder};

// Generalized and WallBreaker WFST exports
pub use fzf_scorer::{FzfConfig, FzfError, FzfMatch, FzfScheme, FzfScorer, FzfStats};
pub use fzf_state_source::FzfStateSource;
pub use fzf_wfst::FzfWfst;
pub use generalized_builder::GeneralizedWfstBuilder;
pub use generalized_wfst::GeneralizedWfst;
pub use wallbreaker_builder::WallBreakerWfstBuilder;
pub use wallbreaker_wfst::WallBreakerWfst;

#[cfg(feature = "ffi")]
pub mod ffi;

// Re-export commonly used lling-llang types for convenience
pub use lling_llang::prelude::{
    ArcticWeight, LazyState, LazyWfst, LazyWfstWrapper, Semiring, StateId, StateSource,
    TropicalWeight, VocabId, WeightedTransition, Wfst,
};

/// Error returned when a caller supplies an invalid non-negative weight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InvalidWeightError {
    name: &'static str,
    value: f64,
}

impl InvalidWeightError {
    #[inline]
    fn new(name: &'static str, value: f64) -> Self {
        Self { name, value }
    }

    /// Name of the invalid weight parameter.
    #[inline]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Caller-supplied invalid value.
    #[inline]
    pub fn value(&self) -> f64 {
        self.value
    }
}

impl std::fmt::Display for InvalidWeightError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} must be finite and non-negative (got {:?})",
            self.name, self.value
        )
    }
}

impl std::error::Error for InvalidWeightError {}

#[inline]
pub(crate) fn validate_finite_nonnegative_weight(
    name: &'static str,
    weight: f64,
) -> Result<f64, InvalidWeightError> {
    if weight.is_finite() && weight >= 0.0 {
        Ok(weight)
    } else {
        Err(InvalidWeightError::new(name, weight))
    }
}

#[inline]
pub(crate) fn exact_usize_to_f64(value: usize) -> Option<f64> {
    (value <= max_exact_f64_usize()).then_some(value as f64)
}

#[inline]
pub(crate) fn max_exact_f64_usize() -> usize {
    usize::try_from(1u128 << f64::MANTISSA_DIGITS).unwrap_or(usize::MAX)
}

#[inline]
pub(crate) fn fx_hash_map_with_capacity<K, V>(capacity: usize) -> rustc_hash::FxHashMap<K, V> {
    rustc_hash::FxHashMap::with_capacity_and_hasher(capacity, Default::default())
}

#[cfg(feature = "phonetic-rules")]
#[inline]
pub(crate) fn fx_hash_set_with_capacity<K>(capacity: usize) -> rustc_hash::FxHashSet<K> {
    rustc_hash::FxHashSet::with_capacity_and_hasher(capacity, Default::default())
}

pub(crate) const MAX_SPECULATIVE_PREALLOCATION: usize = 16_384;
pub(crate) const MAX_NUM_STATES_HINT: usize = 1_000_000;

#[inline]
pub(crate) fn capped_size_hint_capacity(size_hint: usize) -> usize {
    size_hint.min(MAX_SPECULATIVE_PREALLOCATION)
}

#[inline]
pub(crate) fn dictionary_product_states_hint(
    dictionary_len: Option<usize>,
    states_per_dictionary_node: usize,
) -> Option<usize> {
    dictionary_len.map(|dict_size| {
        dict_size
            .saturating_mul(states_per_dictionary_node)
            .min(MAX_NUM_STATES_HINT)
    })
}

#[inline]
pub(crate) fn registered_dictionary_product_states_hint(
    dictionary_len: Option<usize>,
    registered_node_count: usize,
    states_per_dictionary_node: usize,
) -> Option<usize> {
    let live_state_count = registered_product_state_id_span(
        registered_node_count,
        states_per_dictionary_node,
        states_per_dictionary_node,
    );

    dictionary_product_states_hint(
        dictionary_len.map(|dict_size| dict_size.max(registered_node_count)),
        states_per_dictionary_node,
    )
    .map(|estimated_state_count| estimated_state_count.max(live_state_count))
}

#[inline]
pub(crate) fn registered_product_state_id_span(
    registered_node_count: usize,
    states_per_dictionary_node: usize,
    registered_states_in_last_node: usize,
) -> usize {
    if registered_node_count == 0 || registered_states_in_last_node == 0 {
        return 0;
    }

    registered_node_count
        .saturating_sub(1)
        .saturating_mul(states_per_dictionary_node)
        .saturating_add(registered_states_in_last_node)
}

#[inline]
pub(crate) fn dictionary_edge_capacity<N>(
    node: &N,
    transitions_per_edge: usize,
    extra_transitions: usize,
) -> Option<usize>
where
    N: libdictenstein::DictionaryNode,
{
    node.edge_count().map(|edge_count| {
        edge_count
            .saturating_mul(transitions_per_edge)
            .saturating_add(extra_transitions)
    })
}

#[inline]
pub(crate) fn read_lock<T>(lock: &std::sync::RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[inline]
pub(crate) fn write_lock<T>(lock: &std::sync::RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Composite state ID encoding for the product automaton.
///
/// States in the Levenshtein WFST encode (dictionary_node, automaton_state) pairs
/// as a single `StateId` using the formula:
///
/// ```text
/// state_id = dict_node_id * max_automaton_states + automaton_state_id
/// ```
///
/// This module provides utilities for encoding and decoding these composite states.
pub mod state_encoding {
    use lling_llang::wfst::StateId;

    /// Try to encode a `(dictionary_node, automaton_state)` pair.
    ///
    /// Returns `None` if the product state space cannot fit in lling-llang's
    /// `u32` `StateId`.
    #[inline]
    pub fn try_encode(
        dict_node: u32,
        automaton_state: u32,
        max_automaton_states: u32,
    ) -> Option<StateId> {
        if max_automaton_states == 0 || automaton_state >= max_automaton_states {
            return None;
        }

        dict_node
            .checked_mul(max_automaton_states)
            .and_then(|base| base.checked_add(automaton_state))
    }

    /// Decode a `StateId` back into `(dictionary_node, automaton_state)`.
    ///
    /// Returns `None` when `max_automaton_states` is zero because there is no
    /// valid product-space width to divide the composite state by.
    #[inline]
    pub fn decode(state_id: StateId, max_automaton_states: u32) -> Option<(u32, u32)> {
        if max_automaton_states == 0 {
            return None;
        }

        let automaton_state = state_id % max_automaton_states;
        let dict_node = state_id / max_automaton_states;
        Some((dict_node, automaton_state))
    }

    /// Estimate the maximum automaton states for a given query length and max distance.
    ///
    /// The Levenshtein automaton for a query of length n with max distance k
    /// has at most O((n+1) * (2k+1)) states (bounded by the position-distance lattice).
    #[inline]
    pub fn estimate_automaton_states(query_len: usize, max_distance: usize) -> u32 {
        let positions = query_len.saturating_add(1);
        let distances = max_distance.saturating_mul(2).saturating_add(1);
        saturating_nonzero_u32(positions.saturating_mul(distances))
    }

    /// Exact state count for the bounded standard Levenshtein adapter.
    ///
    /// The adapter encodes automaton states as `(query_position, edit_cost)`,
    /// so it needs `(query_len + 1) * (max_distance + 1)` distinct IDs.
    #[inline]
    pub fn bounded_levenshtein_states(query_len: usize, max_distance: usize) -> u32 {
        let positions = query_len.saturating_add(1);
        let costs = max_distance.saturating_add(1);
        saturating_nonzero_u32(positions.saturating_mul(costs))
    }

    #[inline]
    fn saturating_nonzero_u32(value: usize) -> u32 {
        u32::try_from(value.max(1)).unwrap_or(u32::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_encoding_roundtrip() {
        let max_states = 100u32;

        for dict_node in 0..10 {
            for auto_state in 0..10 {
                let decoded = state_encoding::try_encode(dict_node, auto_state, max_states)
                    .and_then(|encoded| state_encoding::decode(encoded, max_states));

                assert_eq!(decoded, Some((dict_node, auto_state)));
            }
        }
    }

    #[test]
    fn test_estimate_automaton_states() {
        // Query "hello" (len 5) with max distance 2
        // Positions: 0..5 (6 positions)
        // Distances: -2..2 (5 distances per position conceptually, but bounded)
        let estimate = state_encoding::estimate_automaton_states(5, 2);
        assert!(estimate > 0);
        assert!(estimate <= 100); // Reasonable upper bound
    }

    #[test]
    fn test_state_encoding_estimators_are_nonzero_and_saturating() {
        assert_eq!(state_encoding::estimate_automaton_states(0, 0), 1);
        assert_eq!(state_encoding::bounded_levenshtein_states(0, 0), 1);
        assert_eq!(
            state_encoding::estimate_automaton_states(usize::MAX, usize::MAX),
            u32::MAX
        );
        assert_eq!(
            state_encoding::bounded_levenshtein_states(usize::MAX, usize::MAX),
            u32::MAX
        );
    }

    #[test]
    fn test_state_encoding_rejects_out_of_range_components() {
        assert_eq!(state_encoding::try_encode(0, 10, 10), None);
        assert_eq!(state_encoding::try_encode(0, 0, 0), None);
        assert_eq!(state_encoding::decode(0, 0), None);
    }

    #[test]
    fn test_registered_dictionary_product_hint_uses_live_lower_bound() {
        assert_eq!(
            registered_dictionary_product_states_hint(Some(1), 2, MAX_NUM_STATES_HINT + 1),
            Some(2usize.saturating_mul(MAX_NUM_STATES_HINT + 1))
        );
        assert_eq!(registered_dictionary_product_states_hint(None, 2, 3), None);
    }

    #[test]
    fn test_registered_product_state_id_span_covers_sparse_last_band() {
        assert_eq!(registered_product_state_id_span(0, 10, 1), 0);
        assert_eq!(registered_product_state_id_span(1, 10, 1), 1);
        assert_eq!(registered_product_state_id_span(2, 10, 3), 13);
        assert_eq!(
            registered_product_state_id_span(usize::MAX, 2, usize::MAX),
            usize::MAX
        );
    }

    #[test]
    fn test_exact_usize_to_f64_accepts_only_exact_integer_range() {
        assert_eq!(exact_usize_to_f64(2), Some(2.0));

        let max_u32 = usize::try_from(u32::MAX).unwrap_or(usize::MAX);
        assert_eq!(exact_usize_to_f64(max_u32), Some(f64::from(u32::MAX)));

        let max_exact = max_exact_f64_usize();
        assert_eq!(exact_usize_to_f64(max_exact), Some(max_exact as f64));

        if let Some(unrepresentable_value) = max_exact.checked_add(1) {
            assert_eq!(exact_usize_to_f64(unrepresentable_value), None);
        }
    }

    #[test]
    fn test_capped_size_hint_capacity_limits_speculative_reservations() {
        assert_eq!(capped_size_hint_capacity(0), 0);
        assert_eq!(
            capped_size_hint_capacity(MAX_SPECULATIVE_PREALLOCATION - 1),
            MAX_SPECULATIVE_PREALLOCATION - 1
        );
        assert_eq!(
            capped_size_hint_capacity(usize::MAX),
            MAX_SPECULATIVE_PREALLOCATION
        );
    }

    #[test]
    fn test_lock_helpers_recover_poisoned_lock() {
        let lock = std::sync::Arc::new(std::sync::RwLock::new(1usize));
        let poisoned_lock = lock.clone();

        let _ = std::thread::spawn(move || {
            let mut guard = match poisoned_lock.write() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            *guard = 2;
            std::panic::panic_any("poison test lock");
        })
        .join();

        assert!(lock.is_poisoned());
        assert_eq!(*read_lock(&lock), 2);

        *write_lock(&lock) = 3;
        assert_eq!(*read_lock(&lock), 3);
    }
}
