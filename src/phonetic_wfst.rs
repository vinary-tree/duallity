//! PhoneticWfst wrapper for lling-llang Wfst trait.
//!
//! This module provides [`PhoneticWfst`], a wrapper that exposes a phonetic
//! transducer (NFA × Levenshtein × Dictionary) as a lling-llang WFST.
//!
//! # Key Benefits
//!
//! - **Sound-alike matching**: Matches phonetically similar words (ph ↔ f)
//! - **Edit tolerance**: Combined with Levenshtein for typo tolerance
//! - **Dictionary integration**: Efficiently traverses dictionary structure
//! - **WFST composition**: Can be composed with language models

use lling_llang::prelude::{
    LazyWfst, Semiring, StateId, StateSource, TropicalWeight, WeightedTransition, Wfst,
};

use libdictenstein::{Dictionary, DictionaryNode};

use crate::lazy_cache::{
    empty_char_transitions, ensure_cached_char_state, CachedCharState, LazyStateCache,
};
#[cfg(feature = "phonetic-rules")]
use crate::phonetic_state_source::PhoneticStateSource;
#[cfg(feature = "phonetic-rules")]
use liblevenshtein::phonetic::nfa::NFAChar;

/// A phonetic transducer exposed as a lling-llang WFST.
///
/// This wrapper presents the product of a phonetic NFA, Levenshtein automaton,
/// and dictionary as a weighted finite state transducer with:
/// - **Input labels**: Dictionary characters
/// - **Output labels**: Dictionary characters
/// - **Weights**: Combined phonetic + edit distance as `TropicalWeight`
///
/// # Type Parameters
///
/// - `D`: Dictionary type implementing [`Dictionary`] with `char` units
///
/// # Example
///
/// ```rust,no_run
/// use duallity::PhoneticWfst;
/// use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
/// use liblevenshtein::phonetic::nfa::compile;
/// use liblevenshtein::phonetic::regex::parse;
/// use lling_llang::prelude::*;
///
/// let dict = DynamicDawgChar::<()>::from_terms(vec!["phone", "fone", "bone"]);
/// let nfa = compile(&parse("(ph|f)one").expect("valid pattern")).expect("compiles");
/// let wfst = PhoneticWfst::new(&dict, nfa, 2);
///
/// // Use with lling-llang's composition
/// // let composed = compose(wfst, language_model);
/// ```
#[cfg(feature = "phonetic-rules")]
#[derive(Clone)]
pub struct PhoneticWfst<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
{
    /// The state source for computing transitions
    state_source: PhoneticStateSource<D>,
    /// Cached states (state_id -> computed state info)
    cache: LazyStateCache<CachedCharState>,
    /// Maximum edit distance
    max_distance: u8,
    /// Phonetic weight
    phonetic_weight: f64,
    /// Edit distance weight multiplier
    edit_weight: f64,
}

/// Default maximum cache size for LRU policy (100,000 states)
const DEFAULT_MAX_CACHE_SIZE: usize = 100_000;

#[cfg(feature = "phonetic-rules")]
impl<D> PhoneticWfst<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
{
    /// Create a new phonetic WFST for the given NFA pattern and max distance.
    ///
    /// # Arguments
    ///
    /// - `dictionary`: The dictionary to search
    /// - `nfa`: The phonetic NFA pattern (e.g., compiled from "(ph|f)one")
    /// - `max_distance`: Maximum edit distance for matches
    ///
    /// # Returns
    ///
    /// A new `PhoneticWfst` ready for composition or traversal.
    pub fn new(dictionary: &D, nfa: NFAChar, max_distance: u8) -> Self {
        let state_source = PhoneticStateSource::new(dictionary, nfa, max_distance);
        Self::from_state_source(state_source, max_distance, 0.0, 1.0)
    }

    /// Create a new phonetic WFST with a custom phonetic weight.
    ///
    /// # Arguments
    ///
    /// - `dictionary`: The dictionary to search
    /// - `nfa`: The phonetic NFA pattern
    /// - `max_distance`: Maximum edit distance for matches
    /// - `phonetic_weight`: Cost added for phonetic transformations
    pub fn with_phonetic_weight(
        dictionary: &D,
        nfa: NFAChar,
        max_distance: u8,
        phonetic_weight: f64,
    ) -> Result<Self, crate::InvalidWeightError> {
        Self::with_weights(dictionary, nfa, max_distance, phonetic_weight, 1.0)
    }

    /// Create a new phonetic WFST with custom phonetic and edit weights.
    ///
    /// `phonetic_weight` is charged on each consumed dictionary/NFA edge. `edit_weight`
    /// scales the edit-distance component contributed by accepting final weights.
    ///
    /// # Arguments
    ///
    /// - `dictionary`: The dictionary to search
    /// - `nfa`: The phonetic NFA pattern
    /// - `max_distance`: Maximum edit distance for matches, before weighting
    /// - `phonetic_weight`: Cost added for consumed phonetic transitions
    /// - `edit_weight`: Multiplier applied to accepted edit distance
    pub fn with_weights(
        dictionary: &D,
        nfa: NFAChar,
        max_distance: u8,
        phonetic_weight: f64,
        edit_weight: f64,
    ) -> Result<Self, crate::InvalidWeightError> {
        let state_source = PhoneticStateSource::with_weights(
            dictionary,
            nfa,
            max_distance,
            phonetic_weight,
            edit_weight,
        )?;

        Ok(Self::from_state_source(
            state_source,
            max_distance,
            phonetic_weight,
            edit_weight,
        ))
    }

    fn from_state_source(
        state_source: PhoneticStateSource<D>,
        max_distance: u8,
        phonetic_weight: f64,
        edit_weight: f64,
    ) -> Self {
        Self {
            state_source,
            cache: LazyStateCache::new(DEFAULT_MAX_CACHE_SIZE),
            max_distance,
            phonetic_weight,
            edit_weight,
        }
    }

    /// Get the maximum edit distance.
    pub fn max_distance(&self) -> u8 {
        self.max_distance
    }

    /// Get the phonetic weight.
    pub fn phonetic_weight(&self) -> f64 {
        self.phonetic_weight
    }

    /// Get the edit distance weight multiplier.
    pub fn edit_weight(&self) -> f64 {
        self.edit_weight
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
            PhoneticStateSource::is_valid_product_state,
        );
    }
}

#[cfg(feature = "phonetic-rules")]
impl<D> Wfst<char, TropicalWeight> for PhoneticWfst<D>
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

    #[inline]
    fn is_empty(&self) -> bool {
        false
    }

    #[inline]
    fn is_valid_state(&self, state: StateId) -> bool {
        self.state_source.is_valid_product_state(state)
    }
}

#[cfg(feature = "phonetic-rules")]
impl<D> LazyWfst<char, TropicalWeight> for PhoneticWfst<D>
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

/// Builder for PhoneticWfst with pattern string support.
///
/// This provides a convenient API for creating phonetic WFSTs from
/// pattern strings without manually compiling the NFA.
#[cfg(feature = "phonetic-rules")]
pub struct PhoneticWfstBuilder<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
{
    dictionary: D,
    max_distance: u8,
    phonetic_weight: f64,
    edit_weight: f64,
}

#[cfg(feature = "phonetic-rules")]
impl<D> PhoneticWfstBuilder<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
{
    /// Create a new builder for the given dictionary.
    pub fn new(dictionary: D, max_distance: u8) -> Self {
        Self {
            dictionary,
            max_distance,
            phonetic_weight: 0.0,
            edit_weight: 1.0,
        }
    }

    /// Set the phonetic weight.
    pub fn phonetic_weight(mut self, weight: f64) -> Result<Self, crate::InvalidWeightError> {
        let weight = crate::validate_finite_nonnegative_weight("phonetic_weight", weight)?;

        self.phonetic_weight = weight;
        Ok(self)
    }

    /// Set the edit distance weight multiplier.
    pub fn edit_weight(mut self, weight: f64) -> Result<Self, crate::InvalidWeightError> {
        let weight = crate::validate_finite_nonnegative_weight("edit_weight", weight)?;

        self.edit_weight = weight;
        Ok(self)
    }

    /// Build a PhoneticWfst from a pattern string.
    ///
    /// # Arguments
    ///
    /// - `pattern`: A phonetic regex pattern (e.g., "(ph|f)one")
    ///
    /// # Returns
    ///
    /// A `Result` containing the `PhoneticWfst` or an error if parsing fails.
    pub fn build_from_pattern(self, pattern: &str) -> Result<PhoneticWfst<D>, String> {
        use liblevenshtein::phonetic::nfa::compiler::compile;
        use liblevenshtein::phonetic::regex::parse;

        let ast = parse(pattern).map_err(|e| format!("Parse error: {:?}", e))?;
        let nfa = compile(&ast).map_err(|e| format!("Compile error: {:?}", e))?;

        PhoneticWfst::with_weights(
            &self.dictionary,
            nfa,
            self.max_distance,
            self.phonetic_weight,
            self.edit_weight,
        )
        .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
#[cfg(feature = "phonetic-rules")]
mod tests {
    use super::*;
    use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
    use liblevenshtein::phonetic::nfa::compiler::compile;
    use liblevenshtein::phonetic::regex::parse;

    #[test]
    fn test_phonetic_wfst_creation() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["phone", "fone", "help"]);
        let nfa = compile(&parse("(ph|f)one").expect("parse")).expect("compile");
        let wfst = PhoneticWfst::new(&dict, nfa, 2);

        assert_eq!(wfst.max_distance(), 2);
        assert_eq!(wfst.phonetic_weight(), 0.0);
        assert_eq!(wfst.edit_weight(), 1.0);
    }

    #[test]
    fn test_phonetic_wfst_start_state() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["phone", "help"]);
        let nfa = compile(&parse("(ph|f)one").expect("parse")).expect("compile");
        let wfst = PhoneticWfst::new(&dict, nfa, 2);

        let start = wfst.start();
        assert_eq!(start, 0);
    }

    #[test]
    fn test_phonetic_wfst_expand_state() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["phone", "fone"]);
        let nfa = compile(&parse("(ph|f)one").expect("parse")).expect("compile");
        let mut wfst = PhoneticWfst::new(&dict, nfa, 2);

        let start = wfst.start();
        assert!(!wfst.is_expanded(start));

        wfst.expand(start);
        assert!(wfst.is_expanded(start));
        assert!(wfst.computed_states() >= 1);
    }

    #[test]
    fn test_phonetic_wfst_with_weight() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["phone"]);
        let nfa = compile(&parse("phone").expect("parse")).expect("compile");
        let wfst =
            PhoneticWfst::with_phonetic_weight(&dict, nfa, 2, 0.5).expect("valid phonetic weight");

        assert_eq!(wfst.phonetic_weight(), 0.5);
        assert_eq!(wfst.edit_weight(), 1.0);
    }

    #[test]
    fn test_phonetic_wfst_with_weights() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["phone"]);
        let nfa = compile(&parse("phone").expect("parse")).expect("compile");
        let wfst =
            PhoneticWfst::with_weights(&dict, nfa, 2, 0.25, 1.5).expect("valid phonetic weights");

        assert_eq!(wfst.phonetic_weight(), 0.25);
        assert_eq!(wfst.edit_weight(), 1.5);
    }

    #[test]
    fn test_phonetic_wfst_rejects_negative_edit_weight() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["phone"]);
        let nfa = compile(&parse("phone").expect("parse")).expect("compile");
        let error = match PhoneticWfst::with_weights(&dict, nfa, 2, 0.25, -1.0) {
            Ok(_) => std::panic::panic_any("negative edit weight should be rejected"),
            Err(error) => error,
        };

        assert_eq!(error.name(), "edit_weight");
        assert_eq!(error.value(), -1.0);
    }

    #[test]
    fn test_phonetic_wfst_builder_rejects_infinite_phonetic_weight() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["phone"]);
        let error = match PhoneticWfstBuilder::new(dict, 2).phonetic_weight(f64::INFINITY) {
            Ok(_) => std::panic::panic_any("infinite phonetic weight should be rejected"),
            Err(error) => error,
        };

        assert_eq!(error.name(), "phonetic_weight");
        assert!(error.value().is_infinite());
    }

    #[test]
    fn test_phonetic_wfst_cache_policy() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
        let nfa = compile(&parse("test").expect("parse")).expect("compile");
        let mut wfst = PhoneticWfst::new(&dict, nfa, 1);

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
    fn test_phonetic_wfst_rejects_invalid_state_without_caching() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
        let nfa = compile(&parse("test").expect("parse")).expect("compile");
        let mut wfst = PhoneticWfst::new(&dict, nfa, 1);
        let invalid_state = u32::MAX;

        assert!(!wfst.is_valid_state(invalid_state));
        wfst.expand(invalid_state);

        assert_eq!(wfst.computed_states(), 0);
        assert!(wfst.transitions_lazy(invalid_state).is_empty());
        assert_eq!(wfst.computed_states(), 0);
    }

    #[test]
    fn test_phonetic_wfst_no_cache_policy_uses_scratch_only() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
        let nfa = compile(&parse("test").expect("parse")).expect("compile");
        let mut wfst = PhoneticWfst::new(&dict, nfa, 1);
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
    fn test_phonetic_wfst_lru_policy_evicts_least_recently_used_state() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
        let nfa = compile(&parse("test").expect("parse")).expect("compile");
        let mut wfst = PhoneticWfst::new(&dict, nfa, 1);
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
    fn test_phonetic_wfst_builder() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["phone", "fone"]);
        let builder = PhoneticWfstBuilder::new(dict, 2)
            .phonetic_weight(0.1)
            .expect("valid phonetic weight")
            .edit_weight(1.5)
            .expect("valid edit weight");

        let wfst = builder.build_from_pattern("(ph|f)one").expect("build");
        assert_eq!(wfst.max_distance(), 2);
        assert_eq!(wfst.phonetic_weight(), 0.1);
        assert_eq!(wfst.edit_weight(), 1.5);
    }
}
