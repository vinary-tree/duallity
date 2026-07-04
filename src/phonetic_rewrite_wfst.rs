//! Rule-based Phonetic Rewrite WFST.
//!
//! This module provides [`RewriteWfst`], a WFST that applies phonetic rewrite
//! rules as explicit character/epsilon transition chains. Unlike the
//! NFA-based phonetic WFST, this provides direct control over individual
//! unconditional rewrite rules.
//!
//! # Rewrite Rules
//!
//! Phonetic rewrite rules follow the unconditional format:
//! - `input -> output`
//!
//! Examples:
//! - `ph -> f` (ph always becomes f)
//! - `gh -> f` (as in rough -> ruff)
//! - `e -> ` (e is deleted)
//!
//! Left/right context or word-boundary conditioning should be expanded into
//! explicit unconditional rules before constructing [`RewriteWfst`].
//!
//! # Integration with WFST Pipelines
//!
//! RewriteWfst can be composed with other WFSTs:
//!
//! ```text
//! Input -> [RewriteWfst] -> [LevenshteinWfst] -> [LM WFST] -> Candidates
//! ```

use lling_llang::prelude::{
    LazyState, LazyWfst, Semiring, StateId, StateSource, TropicalWeight, WeightedTransition, Wfst,
};
use smallvec::SmallVec;

use crate::lazy_cache::{empty_char_transitions, CachedCharState, LazyStateCache};
use crate::phonetic_rewrite_support::{
    prune_dominated_transitions, rewrite_state_count, usize_from_state_id, validate_rewrite_rule,
    validate_rewrite_rules, ContinuationState, PreparedRewriteRule, PreparedRuleMetadata,
};

/// A phonetic rewrite rule.
#[derive(Debug, Clone)]
pub struct RewriteRule {
    /// Input pattern (characters to match)
    pub input: String,
    /// Output pattern (replacement characters)
    pub output: String,
    /// Cost of applying this rule
    pub cost: f64,
    /// Priority (higher = applied first when multiple rules match)
    pub priority: i32,
}

impl RewriteRule {
    /// Create a simple unconditional rewrite rule.
    pub fn new(input: &str, output: &str) -> Self {
        Self {
            input: input.to_string(),
            output: output.to_string(),
            cost: 0.0,
            priority: 0,
        }
    }

    /// Create a rewrite rule with cost.
    pub fn with_cost(
        input: &str,
        output: &str,
        cost: f64,
    ) -> Result<Self, crate::InvalidWeightError> {
        let cost = crate::validate_finite_nonnegative_weight("rewrite rule cost", cost)?;

        Ok(Self::from_validated_cost(input, output, cost))
    }

    fn from_validated_cost(input: &str, output: &str, cost: f64) -> Self {
        Self {
            input: input.to_string(),
            output: output.to_string(),
            cost,
            priority: 0,
        }
    }

    /// Set the priority for rule ordering.
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }
}

/// A rule-based phonetic rewrite WFST.
///
/// This WFST exposes transitions for the configured rewrite rules. Higher
/// priority rules are enumerated first; equal-priority rules keep insertion
/// order.
///
/// # State Encoding
///
/// State 0 is the home state. Non-zero states encode continuation positions in
/// multi-symbol rewrite chains.
///
/// # Example
///
/// ```rust,no_run
/// use duallity::RewriteWfst;
///
/// let mut wfst = RewriteWfst::new();
/// wfst.add_rule("ph", "f", 0.1).expect("valid rewrite rule"); // ph -> f
/// wfst.add_rule("c", "s", 0.2).expect("valid rewrite rule");  // c -> s
///
/// // Compose with Levenshtein WFST
/// // let pipeline = compose(wfst, levenshtein_wfst);
/// ```
#[derive(Clone)]
pub struct RewriteWfst {
    /// Rewrite rules
    rules: Vec<RewriteRule>,
    /// Pre-tokenized rule input/output strings, aligned with `rules`.
    prepared_rules: Vec<PreparedRewriteRule>,
    /// Rule indices ordered by descending priority, with insertion order as a tie-breaker.
    priority_order: Vec<usize>,
    /// Exclusive cumulative continuation-state count after each rule.
    continuation_end_offsets: Vec<usize>,
    /// Dense continuation-state lookup indexed by `state_id - 1`.
    continuation_lookup: Vec<ContinuationState>,
    /// Number of continuation states used to emit multi-symbol rewrites.
    continuation_states: usize,
    /// State cache
    cache: LazyStateCache<CachedCharState>,
    /// Allow identity transitions (passthrough without rewrite)
    allow_identity: bool,
}

/// Default maximum cache size
const DEFAULT_MAX_CACHE_SIZE: usize = 10_000;
const PRINTABLE_ASCII_IDENTITY_TRANSITIONS: usize = (b'~' - b' ' + 1) as usize;

impl RewriteWfst {
    /// Create a new empty rewrite WFST.
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            prepared_rules: Vec::new(),
            priority_order: Vec::new(),
            continuation_end_offsets: Vec::new(),
            continuation_lookup: Vec::new(),
            continuation_states: 0,
            cache: LazyStateCache::new(DEFAULT_MAX_CACHE_SIZE),
            allow_identity: true,
        }
    }

    /// Create a rewrite WFST with the given rules.
    pub fn with_rules(rules: Vec<RewriteRule>) -> Result<Self, crate::InvalidWeightError> {
        validate_rewrite_rules(&rules)?;

        Ok(Self::from_validated_rules(rules))
    }

    fn from_validated_rules(rules: Vec<RewriteRule>) -> Self {
        let metadata = PreparedRuleMetadata::from_rules(&rules);
        Self {
            rules,
            prepared_rules: metadata.prepared_rules,
            priority_order: metadata.priority_order,
            continuation_end_offsets: metadata.continuation_end_offsets,
            continuation_lookup: metadata.continuation_lookup,
            continuation_states: metadata.continuation_states,
            cache: LazyStateCache::new(DEFAULT_MAX_CACHE_SIZE),
            allow_identity: true,
        }
    }

    /// Add a rewrite rule.
    pub fn add_rule(
        &mut self,
        input: &str,
        output: &str,
        cost: f64,
    ) -> Result<(), crate::InvalidWeightError> {
        let rule = RewriteRule::with_cost(input, output, cost)?;
        self.append_validated_rule(rule);
        self.clear_cache();
        Ok(())
    }

    /// Add a pre-built rewrite rule.
    pub fn add_rewrite_rule(&mut self, rule: RewriteRule) -> Result<(), crate::InvalidWeightError> {
        validate_rewrite_rule(&rule)?;

        self.append_validated_rule(rule);
        self.clear_cache();
        Ok(())
    }

    /// Set whether identity transitions are allowed.
    ///
    /// When true, characters can pass through without rewriting.
    /// When false, characters must match a rule to be accepted.
    pub fn set_allow_identity(&mut self, allow: bool) {
        self.allow_identity = allow;
        self.clear_cache();
    }

    /// Get the number of rules.
    pub fn num_rules(&self) -> usize {
        self.rules.len()
    }

    /// Compute transitions from a state.
    ///
    /// State encoding:
    /// - State 0: Initial state (empty buffer)
    /// - State N: Buffer contains partial input match
    fn compute_transitions(
        &self,
        state_id: StateId,
    ) -> (
        bool,
        TropicalWeight,
        SmallVec<[WeightedTransition<char, TropicalWeight>; 4]>,
    ) {
        if !self.is_known_state(state_id) {
            return (false, TropicalWeight::zero(), SmallVec::new());
        }

        let transition_capacity = if state_id == 0 {
            let identity_capacity = if self.allow_identity {
                PRINTABLE_ASCII_IDENTITY_TRANSITIONS
            } else {
                0
            };
            self.priority_order.len().saturating_add(identity_capacity)
        } else {
            1
        };
        let mut transitions = SmallVec::with_capacity(transition_capacity);

        // State 0 is the initial/accepting state
        let is_final = state_id == 0;
        let final_weight = if is_final {
            TropicalWeight::one()
        } else {
            TropicalWeight::zero()
        };

        if state_id == 0 {
            for &rule_index in &self.priority_order {
                self.push_rule_step_transition(&mut transitions, rule_index, 0);
            }
        } else if let Some((rule_index, step)) = self.decode_continuation_state(state_id) {
            self.push_rule_step_transition(&mut transitions, rule_index, step);
        }

        // Add identity transitions if allowed
        if self.allow_identity && state_id == 0 {
            // The fixed rewrite alphabet is printable ASCII; regex-backed phonetic
            // WFSTs handle alphabet-aware NFA expansion separately.
            for c in ' '..='~' {
                transitions.push(WeightedTransition::new(
                    state_id,
                    Some(c),
                    Some(c),
                    0,
                    TropicalWeight::one(), // Zero cost for identity
                ));
            }
        }

        prune_dominated_transitions(&mut transitions);

        (is_final, final_weight, transitions)
    }

    fn push_rule_step_transition(
        &self,
        transitions: &mut SmallVec<[WeightedTransition<char, TropicalWeight>; 4]>,
        rule_index: usize,
        step: usize,
    ) {
        let Some(prepared) = self.prepared_rules.get(rule_index) else {
            return;
        };
        let steps = prepared.steps;

        if steps == 0 || step >= steps {
            return;
        }

        let Some(from) = (if step == 0 {
            Some(0)
        } else {
            self.continuation_state(rule_index, step)
        }) else {
            return;
        };
        let Some(to) = (if step + 1 == steps {
            Some(0)
        } else {
            self.continuation_state(rule_index, step + 1)
        }) else {
            return;
        };
        let input = prepared.input.get(step).copied();
        let output = prepared.output.get(step).copied();
        let weight = if step == 0 {
            prepared.cost_weight
        } else {
            TropicalWeight::one()
        };

        transitions.push(WeightedTransition::new(from, input, output, to, weight));
    }

    fn continuation_state(&self, rule_index: usize, step: usize) -> Option<StateId> {
        debug_assert!(step > 0);
        let raw_state = self.continuation_start(rule_index).saturating_add(step);
        raw_state.try_into().ok()
    }

    fn decode_continuation_state(&self, state_id: StateId) -> Option<(usize, usize)> {
        if state_id == 0 {
            return None;
        }

        let zero_based = usize_from_state_id(state_id.checked_sub(1)?);
        let continuation = self.continuation_lookup.get(zero_based)?;
        let prepared = self.prepared_rules.get(continuation.rule_index)?;

        (continuation.step < prepared.steps).then_some((continuation.rule_index, continuation.step))
    }

    fn continuation_start(&self, rule_index: usize) -> usize {
        rule_index
            .checked_sub(1)
            .and_then(|previous| self.continuation_end_offsets.get(previous))
            .copied()
            .unwrap_or(0)
    }

    fn append_validated_rule(&mut self, rule: RewriteRule) {
        let rule_index = self.rules.len();
        let prepared = PreparedRewriteRule::new(&rule);

        for step in 1..prepared.steps {
            self.continuation_lookup
                .push(ContinuationState { rule_index, step });
        }
        self.continuation_states = self.continuation_lookup.len();
        self.continuation_end_offsets.push(self.continuation_states);
        self.prepared_rules.push(prepared);

        let insertion_index = self.priority_order.partition_point(|&existing_index| {
            let existing_priority = self.rules[existing_index].priority;
            existing_priority > rule.priority
                || (existing_priority == rule.priority && existing_index < rule_index)
        });
        self.priority_order.insert(insertion_index, rule_index);
        self.rules.push(rule);
    }

    fn computed_state(&self, state: StateId) -> Option<&CachedCharState> {
        self.cache.get(state)
    }

    /// Ensure a state is computed and cached.
    fn ensure_state(&mut self, state: StateId) {
        if self.cache.touch_if_cached(state) {
            return;
        }

        if !self.is_known_state(state) {
            return;
        }

        let (is_final, final_weight, transitions) = self.compute_transitions(state);

        self.cache.insert(
            state,
            CachedCharState::new(is_final, final_weight, transitions),
        );
    }

    fn is_known_state(&self, state: StateId) -> bool {
        usize_from_state_id(state) < self.num_states()
    }
}

impl Default for RewriteWfst {
    fn default() -> Self {
        Self::new()
    }
}

impl Wfst<char, TropicalWeight> for RewriteWfst {
    fn start(&self) -> StateId {
        0
    }

    fn is_final(&self, state: StateId) -> bool {
        self.computed_state(state)
            .map(|s| s.is_final)
            .unwrap_or(state == 0)
    }

    fn final_weight(&self, state: StateId) -> TropicalWeight {
        self.computed_state(state)
            .map(|s| s.final_weight)
            .unwrap_or_else(|| {
                if state == 0 {
                    TropicalWeight::one()
                } else {
                    TropicalWeight::zero()
                }
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
        rewrite_state_count(self.continuation_states)
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.num_states() == 0
    }

    #[inline]
    fn is_valid_state(&self, state: StateId) -> bool {
        self.is_known_state(state)
    }
}

impl LazyWfst<char, TropicalWeight> for RewriteWfst {
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

impl StateSource<char, TropicalWeight> for RewriteWfst {
    fn compute_state(&self, state: StateId) -> LazyState<char, TropicalWeight> {
        let (is_final, final_weight, transitions) = self.compute_transitions(state);

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
        Some(self.num_states())
    }
}

/// Builder for common phonetic rewrite rules.
pub struct CommonPhoneticRules;

impl CommonPhoneticRules {
    /// English phonetic rules.
    pub fn english() -> Vec<RewriteRule> {
        vec![
            RewriteRule::from_validated_cost("ph", "f", 0.1),
            RewriteRule::from_validated_cost("gh", "f", 0.2), // rough -> ruff
            RewriteRule::from_validated_cost("ck", "k", 0.1),
            RewriteRule::from_validated_cost("qu", "kw", 0.1),
            RewriteRule::from_validated_cost("x", "ks", 0.1),
            RewriteRule::from_validated_cost("c", "k", 0.2), // coarse hard-c alternative
            RewriteRule::from_validated_cost("c", "s", 0.2), // coarse soft-c alternative
        ]
    }

    /// German phonetic rules.
    pub fn german() -> Vec<RewriteRule> {
        vec![
            RewriteRule::from_validated_cost("sch", "sh", 0.1),
            RewriteRule::from_validated_cost("ch", "x", 0.1), // IPA [x] or [ç]
            RewriteRule::from_validated_cost("ß", "ss", 0.1),
            RewriteRule::from_validated_cost("ä", "ae", 0.1),
            RewriteRule::from_validated_cost("ö", "oe", 0.1),
            RewriteRule::from_validated_cost("ü", "ue", 0.1),
        ]
    }

    /// French phonetic rules.
    pub fn french() -> Vec<RewriteRule> {
        vec![
            RewriteRule::from_validated_cost("eau", "o", 0.1),
            RewriteRule::from_validated_cost("aux", "o", 0.1),
            RewriteRule::from_validated_cost("ai", "e", 0.1),
            RewriteRule::from_validated_cost("ph", "f", 0.1),
            RewriteRule::from_validated_cost("qu", "k", 0.1),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phonetic_rewrite_support::HASH_PRUNE_THRESHOLD;

    #[test]
    fn test_rewrite_wfst_incremental_metadata_matches_bulk_rules() {
        let rules = vec![
            RewriteRule::with_cost("ab", "x", 0.1).expect("valid rewrite rule"),
            RewriteRule::with_cost("c", "de", 0.2)
                .expect("valid rewrite rule")
                .with_priority(10),
            RewriteRule::with_cost("pq", "rs", 0.3).expect("valid rewrite rule"),
        ];

        let mut bulk = RewriteWfst::with_rules(rules.clone()).expect("valid rewrite rules");
        let mut incremental = RewriteWfst::new();
        for rule in rules {
            incremental
                .add_rewrite_rule(rule)
                .expect("valid rewrite rule");
        }

        bulk.set_allow_identity(false);
        incremental.set_allow_identity(false);
        bulk.expand(0);
        incremental.expand(0);

        assert_eq!(incremental.prepared_rules.len(), bulk.prepared_rules.len());
        assert_eq!(
            incremental.continuation_end_offsets,
            bulk.continuation_end_offsets
        );
        assert_eq!(incremental.continuation_lookup, bulk.continuation_lookup);
        assert_eq!(incremental.priority_order, bulk.priority_order);
        assert_eq!(incremental.num_states(), bulk.num_states());

        let bulk_start = bulk
            .transitions(0)
            .iter()
            .map(|transition| (transition.input, transition.output, transition.to))
            .collect::<Vec<_>>();
        let incremental_start = incremental
            .transitions(0)
            .iter()
            .map(|transition| (transition.input, transition.output, transition.to))
            .collect::<Vec<_>>();
        assert_eq!(incremental_start, bulk_start);

        assert_eq!(bulk.decode_continuation_state(1), Some((0, 1)));
        assert_eq!(bulk.decode_continuation_state(2), Some((1, 1)));
        assert_eq!(bulk.decode_continuation_state(3), Some((2, 1)));
        assert_eq!(bulk.decode_continuation_state(4), None);
    }

    #[test]
    fn test_rewrite_metadata_preallocates_exact_continuation_lookup_capacity() {
        let rules = vec![
            RewriteRule::with_cost("abcd", "x", 0.1).expect("valid rewrite rule"),
            RewriteRule::with_cost("e", "fghi", 0.2).expect("valid rewrite rule"),
            RewriteRule::with_cost("", "", 0.3).expect("valid rewrite rule"),
        ];

        let metadata = PreparedRuleMetadata::from_rules(&rules);

        assert_eq!(metadata.continuation_states, 6);
        assert_eq!(metadata.continuation_lookup.len(), 6);
        assert_eq!(metadata.continuation_lookup.capacity(), 6);
        assert_eq!(metadata.continuation_end_offsets, vec![3, 6, 6]);
        assert_eq!(
            metadata
                .prepared_rules
                .iter()
                .map(PreparedRewriteRule::continuation_steps)
                .sum::<usize>(),
            metadata.continuation_states
        );
    }

    #[test]
    fn test_rewrite_num_states_caps_to_representable_state_ids() {
        assert_eq!(rewrite_state_count(0), 1);

        let Ok(max_state_id) = usize::try_from(StateId::MAX) else {
            return;
        };
        let Some(addressable_states) = max_state_id.checked_add(1) else {
            assert_eq!(rewrite_state_count(usize::MAX), usize::MAX);
            return;
        };

        assert_eq!(
            rewrite_state_count(addressable_states - 1),
            addressable_states
        );
        assert_eq!(rewrite_state_count(addressable_states), addressable_states);

        let Some(unaddressable_continuations) = addressable_states.checked_add(10) else {
            return;
        };

        let mut wfst = RewriteWfst::new();
        wfst.continuation_states = unaddressable_continuations;

        assert_eq!(wfst.num_states(), addressable_states);
    }

    #[test]
    fn test_rewrite_transition_pruning_keeps_singleton_unchanged() {
        let mut transitions = SmallVec::<[WeightedTransition<char, TropicalWeight>; 4]>::new();
        transitions.push(WeightedTransition::new(
            0,
            Some('a'),
            Some('b'),
            0,
            TropicalWeight::new(0.25),
        ));

        prune_dominated_transitions(&mut transitions);

        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].input, Some('a'));
        assert_eq!(transitions[0].output, Some('b'));
        assert_eq!(transitions[0].weight.value(), 0.25);
    }

    #[test]
    fn test_rewrite_transition_pruning_hash_path_keeps_best_weight_in_first_position() {
        let mut transitions = SmallVec::<[WeightedTransition<char, TropicalWeight>; 4]>::new();
        transitions.push(WeightedTransition::new(
            0,
            Some('a'),
            Some('a'),
            0,
            TropicalWeight::new(0.7),
        ));

        for offset in 0..HASH_PRUNE_THRESHOLD {
            let label = char::from(b'b' + u8::try_from(offset).unwrap_or(0));
            transitions.push(WeightedTransition::new(
                0,
                Some(label),
                Some(label),
                0,
                TropicalWeight::one(),
            ));
        }

        transitions.push(WeightedTransition::new(
            0,
            Some('a'),
            Some('a'),
            0,
            TropicalWeight::new(0.1),
        ));

        assert!(transitions.len() > HASH_PRUNE_THRESHOLD);

        prune_dominated_transitions(&mut transitions);

        assert_eq!(transitions.len(), HASH_PRUNE_THRESHOLD + 1);
        assert_eq!(transitions[0].input, Some('a'));
        assert_eq!(transitions[0].weight.value(), 0.1);
        assert_eq!(transitions[1].input, Some('b'));
    }
}
