//! Pure Phonetic NFA WFST wrapper for lling-llang.
//!
//! This module provides [`PhoneticNfaWfst`], a WFST wrapper for phonetic NFAs
//! that can be composed with other WFSTs in a pipeline. Unlike [`PhoneticWfst`],
//! this does not include dictionary integration - it's a pure phonetic transducer.
//!
//! # Use Cases
//!
//! - **Composition pipeline**: Chain phonetic matching with Levenshtein and language models
//! - **Standalone phonetic matching**: Use NFA for pattern matching without edit distance
//! - **Custom pipelines**: Build complex matching pipelines with explicit composition

use std::sync::{Arc, RwLock};

use lling_llang::prelude::{
    LazyState, LazyWfst, Semiring, StateId, StateSource, TropicalWeight, WeightedTransition, Wfst,
};
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use crate::fx_hash_map_with_capacity;
use crate::lazy_cache::{empty_char_transitions, CachedCharState, LazyStateCache};
use crate::phonetic_anchors::{
    start_anchor_closure, state_set_can_reach_final_through_end_anchors,
};
use crate::phonetic_nfa_support::{
    collect_label_successors, default_phonetic_nfa_alphabet, next_nfa_state_id, normalize_alphabet,
    phonetic_nfa_state_hint, push_bounded_char_range, state_set_to_key, usize_from_state_id,
    CandidateChars, LabelSuccessors, StateSetKey,
};
#[cfg(feature = "phonetic-rules")]
use liblevenshtein::phonetic::nfa::{CharClassChar, NFAChar, StateSet, TransitionLabelChar};

#[cfg(feature = "phonetic-rules")]
type PendingNfaTargets = SmallVec<[(char, StateSet, StateSetKey); 8]>;
#[cfg(feature = "phonetic-rules")]
type ResolvedNfaTargets = SmallVec<[(char, Option<StateId>); 8]>;
#[cfg(feature = "phonetic-rules")]
type MissingNfaTargets = SmallVec<[(usize, StateSet, StateSetKey); 8]>;
#[cfg(feature = "phonetic-rules")]
type UniqueMissingNfaTargets = SmallVec<[(StateSet, StateSetKey, bool); 8]>;
#[cfg(feature = "phonetic-rules")]
type MissingNfaTargetUses = SmallVec<[(usize, usize); 8]>;

/// A pure phonetic NFA exposed as a lling-llang WFST.
///
/// This wrapper exposes a phonetic NFA as a weighted transducer:
/// - **Input labels**: Characters from the input string
/// - **Output labels**: Same as input (identity transduction)
/// - **Weights**: Phonetic transformation costs as `TropicalWeight`
///
/// The NFA supports phonetic alternations like `(ph|f)` where `ph` and `f`
/// are phonetically equivalent with potentially different costs.
///
/// # Example
///
/// ```rust,no_run
/// use duallity::PhoneticNfaWfst;
/// use liblevenshtein::phonetic::nfa::compile;
/// use liblevenshtein::phonetic::regex::parse;
/// use lling_llang::prelude::*;
///
/// let nfa = compile(&parse("(ph|f)one").expect("valid pattern")).expect("compiles");
/// let wfst = PhoneticNfaWfst::new(nfa);
///
/// // Compose with other WFSTs
/// // let pipeline = compose(wfst, levenshtein_wfst);
/// ```
#[cfg(feature = "phonetic-rules")]
#[derive(Clone)]
pub struct PhoneticNfaWfst {
    /// The phonetic NFA
    nfa: NFAChar,
    /// Phonetic weight (cost for each NFA transition)
    phonetic_weight: f64,
    /// Finite alphabet used to concretize wide NFA labels such as `.` and character classes.
    alphabet: Arc<[char]>,
    /// Shared NFA state-set registry for lazy and StateSource views.
    state_registry: Arc<RwLock<NfaStateRegistry>>,
    /// Cached state information
    cache: LazyStateCache<CachedCharState>,
}

/// Registry for assigning stable WFST state IDs to NFA state sets.
#[cfg(feature = "phonetic-rules")]
struct NfaStateRegistry {
    state_to_id: FxHashMap<StateSetKey, StateId>,
    id_to_state: Vec<RegisteredNfaState>,
}

/// Metadata for a registered WFST state backed by an NFA state set.
#[cfg(feature = "phonetic-rules")]
#[derive(Clone)]
struct RegisteredNfaState {
    state_set: Arc<StateSet>,
    is_final: bool,
}

#[cfg(feature = "phonetic-rules")]
impl NfaStateRegistry {
    fn new(initial_closure: StateSet, initial_is_final: bool) -> Self {
        let mut state_to_id = fx_hash_map_with_capacity(1);
        let initial_key = state_set_to_key(&initial_closure);
        state_to_id.insert(initial_key, 0);
        Self {
            state_to_id,
            id_to_state: vec![RegisteredNfaState {
                state_set: Arc::new(initial_closure),
                is_final: initial_is_final,
            }],
        }
    }

    fn get(&self, state_id: StateId) -> Option<RegisteredNfaState> {
        self.id_to_state.get(usize_from_state_id(state_id)).cloned()
    }

    fn id_for_key(&self, key: &StateSetKey) -> Option<StateId> {
        self.state_to_id.get(key).copied()
    }

    fn get_or_create_with_key(
        &mut self,
        state_set: StateSet,
        key: StateSetKey,
        is_final: bool,
    ) -> Option<StateId> {
        if let Some(id) = self.id_for_key(&key) {
            return Some(id);
        }

        let id = next_nfa_state_id(self.id_to_state.len())?;
        self.state_to_id.insert(key, id);
        self.id_to_state.push(RegisteredNfaState {
            state_set: Arc::new(state_set),
            is_final,
        });
        Some(id)
    }

    fn len(&self) -> usize {
        self.id_to_state.len()
    }
}

/// Default maximum cache size (50,000 states)
const DEFAULT_MAX_CACHE_SIZE: usize = 50_000;

#[cfg(feature = "phonetic-rules")]
impl PhoneticNfaWfst {
    /// Create a new phonetic NFA WFST with default settings.
    ///
    /// # Arguments
    ///
    /// - `nfa`: The phonetic NFA to wrap
    pub fn new(nfa: NFAChar) -> Self {
        Self::from_validated_weight_and_alphabet(nfa, 0.0, default_phonetic_nfa_alphabet())
    }

    /// Create a new phonetic NFA WFST with custom phonetic weight.
    ///
    /// # Arguments
    ///
    /// - `nfa`: The phonetic NFA to wrap
    /// - `phonetic_weight`: Cost added for each NFA transition
    pub fn with_phonetic_weight(
        nfa: NFAChar,
        phonetic_weight: f64,
    ) -> Result<Self, crate::InvalidWeightError> {
        Self::with_phonetic_weight_and_alphabet(
            nfa,
            phonetic_weight,
            default_phonetic_nfa_alphabet(),
        )
    }

    /// Create a new phonetic NFA WFST with a custom finite alphabet.
    ///
    /// The alphabet is used to concretize wide NFA labels (`.` and negated or large
    /// character classes) into ordinary `char` WFST transitions. Literal `Char(c)` labels
    /// are always exact, even when `c` is not in the alphabet.
    pub fn with_alphabet<I>(nfa: NFAChar, alphabet: I) -> Self
    where
        I: IntoIterator<Item = char>,
    {
        Self::from_validated_weight_and_alphabet(nfa, 0.0, alphabet)
    }

    /// Create a new phonetic NFA WFST with custom phonetic weight and finite alphabet.
    ///
    /// Duplicate alphabet symbols are ignored after their first occurrence so transition
    /// order is deterministic and bounded by the caller-provided alphabet order.
    pub fn with_phonetic_weight_and_alphabet<I>(
        nfa: NFAChar,
        phonetic_weight: f64,
        alphabet: I,
    ) -> Result<Self, crate::InvalidWeightError>
    where
        I: IntoIterator<Item = char>,
    {
        let phonetic_weight =
            crate::validate_finite_nonnegative_weight("phonetic_weight", phonetic_weight)?;

        Ok(Self::from_validated_weight_and_alphabet(
            nfa,
            phonetic_weight,
            alphabet,
        ))
    }

    fn from_validated_weight_and_alphabet<I>(
        nfa: NFAChar,
        phonetic_weight: f64,
        alphabet: I,
    ) -> Self
    where
        I: IntoIterator<Item = char>,
    {
        // Register initial state. Start anchors are valid only at the initial WFST
        // position, so they are folded into state 0 rather than followed later.
        let initial_closure = start_anchor_closure(&nfa);
        let initial_is_final =
            state_set_can_reach_final_through_end_anchors(&nfa, &initial_closure);
        let state_registry = Arc::new(RwLock::new(NfaStateRegistry::new(
            initial_closure,
            initial_is_final,
        )));
        let alphabet = normalize_alphabet(alphabet);

        Self {
            nfa,
            phonetic_weight,
            alphabet,
            state_registry,
            cache: LazyStateCache::new(DEFAULT_MAX_CACHE_SIZE),
        }
    }

    /// Get the phonetic weight.
    pub fn phonetic_weight(&self) -> f64 {
        self.phonetic_weight
    }

    /// Finite alphabet used for concretizing wide NFA labels.
    pub fn alphabet(&self) -> &[char] {
        &self.alphabet
    }

    /// Set the maximum cache size for LRU eviction.
    pub fn set_max_cache_size(&mut self, size: usize) {
        self.cache.set_max_lru_states(size);
    }

    fn computed_state(&self, state: StateId) -> Option<&CachedCharState> {
        self.cache.get(state)
    }

    fn registered_state(&self, state: StateId) -> Option<RegisteredNfaState> {
        crate::read_lock(&self.state_registry).get(state)
    }

    fn registered_state_is_final(&self, state: StateId) -> bool {
        self.registered_state(state)
            .is_some_and(|registered| registered.is_final)
    }

    /// Compute transitions for an NFA state set.
    fn compute_nfa_transitions(
        &self,
        state_id: StateId,
    ) -> (
        bool,
        TropicalWeight,
        SmallVec<[WeightedTransition<char, TropicalWeight>; 4]>,
    ) {
        let registered_state = match crate::read_lock(&self.state_registry).get(state_id) {
            Some(registered) => registered,
            None => return (false, TropicalWeight::zero(), SmallVec::new()),
        };
        let state_set = registered_state.state_set;

        // Collect all concrete input characters available from this NFA state set.
        let mut candidates = CandidateChars::with_capacity(self.alphabet.len());
        for nfa_state in state_set.iter() {
            for trans in self.nfa.transitions_from(nfa_state) {
                if trans.label.consumes_input() {
                    self.collect_label_candidates(&trans.label, &mut candidates);
                }
            }
        }
        let chars = candidates.as_slice();

        let mut successors_by_char = LabelSuccessors::new(chars);
        for nfa_state in state_set.iter() {
            for trans in self.nfa.transitions_from(nfa_state) {
                collect_label_successors(&trans.label, trans.to, chars, &mut successors_by_char);
            }
        }

        let mut pending_targets = PendingNfaTargets::with_capacity(successors_by_char.len());
        for (c, next_states) in successors_by_char.into_slots() {
            if next_states.is_empty() {
                continue;
            }

            // Apply epsilon closure
            let next_closure = self.nfa.epsilon_closure(&next_states);
            let key = state_set_to_key(&next_closure);
            pending_targets.push((c, next_closure, key));
        }

        let resolved_targets = self.resolve_transition_targets(pending_targets);
        let mut transitions = SmallVec::with_capacity(resolved_targets.len());
        for (c, target_id) in resolved_targets {
            let Some(target_id) = target_id else {
                continue;
            };
            transitions.push(WeightedTransition::new(
                state_id,
                Some(c),
                Some(c),
                target_id,
                TropicalWeight::new(self.phonetic_weight),
            ));
        }

        // Check if this state set is final, including terminal zero-width anchors.
        let is_final = registered_state.is_final;
        let final_weight = if is_final {
            TropicalWeight::one()
        } else {
            TropicalWeight::zero()
        };

        (is_final, final_weight, transitions)
    }

    fn resolve_transition_targets(&self, pending_targets: PendingNfaTargets) -> ResolvedNfaTargets {
        let mut resolved_targets = ResolvedNfaTargets::with_capacity(pending_targets.len());
        let mut missing_targets = MissingNfaTargets::new();

        {
            let registry = crate::read_lock(&self.state_registry);
            for (candidate, state_set, key) in pending_targets {
                if let Some(target_id) = registry.id_for_key(&key) {
                    resolved_targets.push((candidate, Some(target_id)));
                } else {
                    let index = resolved_targets.len();
                    resolved_targets.push((candidate, None));
                    missing_targets.push((index, state_set, key));
                }
            }
        }

        if missing_targets.is_empty() {
            return resolved_targets;
        }

        let mut unique_index_by_key = fx_hash_map_with_capacity(missing_targets.len());
        let mut unique_missing = UniqueMissingNfaTargets::with_capacity(missing_targets.len());
        let mut missing_target_uses = MissingNfaTargetUses::with_capacity(missing_targets.len());
        for (resolved_index, state_set, key) in missing_targets {
            let unique_index = if let Some(&unique_index) = unique_index_by_key.get(&key) {
                unique_index
            } else {
                let unique_index = unique_missing.len();
                unique_index_by_key.insert(key.clone(), unique_index);
                let is_final = self.state_set_is_final(&state_set);
                unique_missing.push((state_set, key, is_final));
                unique_index
            };
            missing_target_uses.push((resolved_index, unique_index));
        }

        let mut unique_target_ids =
            SmallVec::<[Option<StateId>; 8]>::with_capacity(unique_missing.len());
        let mut registry = crate::write_lock(&self.state_registry);
        for (state_set, key, is_final) in unique_missing {
            unique_target_ids.push(registry.get_or_create_with_key(state_set, key, is_final));
        }

        for (resolved_index, unique_index) in missing_target_uses {
            if let Some(target_id) = unique_target_ids.get(unique_index).copied().flatten() {
                if let Some((_, resolved_target)) = resolved_targets.get_mut(resolved_index) {
                    *resolved_target = Some(target_id);
                }
            }
        }

        resolved_targets
    }

    /// Ensure a state is computed and cached.
    fn ensure_state(&mut self, state: StateId) {
        if self.cache.touch_if_cached(state) {
            return;
        }

        if !self.is_registered_state(state) {
            return;
        }

        let (is_final, final_weight, transitions) = self.compute_nfa_transitions(state);

        self.cache.insert(
            state,
            CachedCharState::new(is_final, final_weight, transitions),
        );
    }

    fn is_registered_state(&self, state: StateId) -> bool {
        usize_from_state_id(state) < crate::read_lock(&self.state_registry).len()
    }

    fn collect_label_candidates(&self, label: &TransitionLabelChar, chars: &mut CandidateChars) {
        match label {
            TransitionLabelChar::Char(c) => chars.push_unique(*c),
            TransitionLabelChar::CharClass(class) => {
                self.collect_char_class_candidates(class, chars);
            }
            TransitionLabelChar::Any => {
                for &candidate in self.alphabet.iter() {
                    chars.push_unique(candidate);
                }
            }
            _ => {}
        }
    }

    fn state_set_is_final(&self, state_set: &StateSet) -> bool {
        state_set_can_reach_final_through_end_anchors(&self.nfa, state_set)
    }

    fn collect_char_class_candidates(&self, class: &CharClassChar, chars: &mut CandidateChars) {
        for &candidate in self.alphabet.iter() {
            if class.matches(candidate) {
                chars.push_unique(candidate);
            }
        }

        if !class.negated {
            for &(start, end) in &class.ranges {
                push_bounded_char_range(chars, start, end);
            }
        }
    }
}

#[cfg(feature = "phonetic-rules")]
impl Wfst<char, TropicalWeight> for PhoneticNfaWfst {
    fn start(&self) -> StateId {
        0 // Initial state is always ID 0
    }

    fn is_final(&self, state: StateId) -> bool {
        self.computed_state(state)
            .map(|s| s.is_final)
            .unwrap_or_else(|| self.registered_state_is_final(state))
    }

    fn final_weight(&self, state: StateId) -> TropicalWeight {
        if let Some(cached) = self.computed_state(state) {
            return cached.final_weight;
        }

        if self.registered_state_is_final(state) {
            TropicalWeight::one()
        } else {
            TropicalWeight::zero()
        }
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
        crate::read_lock(&self.state_registry).len()
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.num_states() == 0
    }

    #[inline]
    fn is_valid_state(&self, state: StateId) -> bool {
        self.is_registered_state(state)
    }
}

#[cfg(feature = "phonetic-rules")]
impl LazyWfst<char, TropicalWeight> for PhoneticNfaWfst {
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

#[cfg(feature = "phonetic-rules")]
impl StateSource<char, TropicalWeight> for PhoneticNfaWfst {
    fn compute_state(&self, state: StateId) -> LazyState<char, TropicalWeight> {
        if !self.is_valid_state(state) {
            return LazyState::non_final(SmallVec::new());
        }

        let (is_final, final_weight, transitions) = self.compute_nfa_transitions(state);
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
        Some(phonetic_nfa_state_hint(
            self.nfa.num_states(),
            crate::read_lock(&self.state_registry).len(),
        ))
    }
}

#[cfg(test)]
#[cfg(feature = "phonetic-rules")]
mod tests {
    use super::*;

    #[test]
    fn test_phonetic_nfa_registry_preallocates_and_reuses_start_state() {
        let mut initial = StateSet::new();
        initial.insert(0);
        let mut registry = NfaStateRegistry::new(initial.clone(), true);

        assert!(registry.state_to_id.capacity() >= 1);
        assert!(registry.id_to_state.capacity() >= 1);
        assert!(registry.get(0).is_some());
        assert_eq!(
            registry.get_or_create_with_key(initial.clone(), state_set_to_key(&initial), false),
            Some(0)
        );

        let first_lookup = registry.get(0).expect("start state should be registered");
        let second_lookup = registry
            .get(0)
            .expect("start state should remain registered");
        assert!(Arc::ptr_eq(
            &first_lookup.state_set,
            &second_lookup.state_set
        ));
        assert!(first_lookup.is_final);

        let mut non_final = StateSet::new();
        non_final.insert(1);
        let non_final_id = registry
            .get_or_create_with_key(non_final.clone(), state_set_to_key(&non_final), false)
            .expect("representable registry state");
        assert!(
            !registry
                .get(non_final_id)
                .expect("registered state")
                .is_final
        );
    }

    #[test]
    fn test_phonetic_nfa_state_hint_is_saturating_and_never_underreports_registered_states() {
        assert_eq!(phonetic_nfa_state_hint(usize::MAX, 1), usize::MAX);

        let nfa = NFAChar::new();
        let wfst = PhoneticNfaWfst::new(nfa);
        let estimated_hint = phonetic_nfa_state_hint(wfst.nfa.num_states(), 0);

        {
            let mut registry = crate::write_lock(&wfst.state_registry);
            for state_index in 0..=estimated_hint {
                let Some(nfa_state) = u32::try_from(state_index.saturating_add(1)).ok() else {
                    break;
                };
                let mut state_set = StateSet::new();
                state_set.insert(nfa_state);
                let key = state_set_to_key(&state_set);
                assert!(registry
                    .get_or_create_with_key(state_set, key, false)
                    .is_some());
            }
        }

        assert!(Wfst::num_states(&wfst) > estimated_hint);
        assert_eq!(
            StateSource::<char, TropicalWeight>::num_states_hint(&wfst),
            Some(Wfst::num_states(&wfst))
        );
    }

    #[test]
    fn test_phonetic_nfa_wildcard_transitions_share_registered_successor() {
        let ast = liblevenshtein::phonetic::regex::parse(".").expect("wildcard parses");
        let nfa =
            liblevenshtein::phonetic::nfa::compiler::compile(&ast).expect("wildcard compiles");
        let mut wfst = PhoneticNfaWfst::with_alphabet(nfa, ['x', 'y', 'z']);

        wfst.expand(0);

        let transitions = wfst.transitions(0);
        assert_eq!(transitions.len(), 3);
        let target = transitions
            .first()
            .map(|transition| transition.to)
            .expect("wildcard should create transitions");

        assert!(transitions.iter().all(|transition| transition.to == target));
        assert_eq!(Wfst::num_states(&wfst), 2);
    }
}
