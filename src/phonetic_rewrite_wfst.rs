//! Rule-based Phonetic Rewrite WFST.
//!
//! This module provides [`RewriteWfst`], a WFST that applies phonetic rewrite
//! rules with optional context conditions. Unlike the NFA-based phonetic WFST,
//! this provides explicit control over individual rewrite rules.
//!
//! # Rewrite Rules
//!
//! Phonetic rewrite rules follow the format:
//! - `input -> output` (unconditional)
//! - `input -> output / left_context _ right_context` (contextual)
//!
//! Examples:
//! - `ph -> f` (ph always becomes f)
//! - `c -> s / _[ei]` (c becomes s before e or i)
//! - `e -> / _#` (e is deleted at word end)
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
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

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
    pub fn with_cost(input: &str, output: &str, cost: f64) -> Self {
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
/// This WFST applies a set of rewrite rules to transform input strings.
/// Rules are applied greedily, with higher priority rules matched first.
///
/// # State Encoding
///
/// States encode the current position in the input buffer plus any
/// partial rule matches in progress.
///
/// # Example
///
/// ```rust,ignore
/// use liblevenshtein::wfst::RewriteWfst;
///
/// let mut wfst = RewriteWfst::new();
/// wfst.add_rule("ph", "f", 0.1);  // ph -> f with cost 0.1
/// wfst.add_rule("c", "s", 0.2);   // c -> s with cost 0.2
///
/// // Compose with Levenshtein WFST
/// // let pipeline = compose(wfst, levenshtein_wfst);
/// ```
#[derive(Clone)]
pub struct RewriteWfst {
    /// Rewrite rules
    rules: Vec<RewriteRule>,
    /// Number of continuation states used to emit multi-symbol rewrites.
    continuation_states: usize,
    /// State cache
    cache: FxHashMap<StateId, CachedRewriteState>,
    /// Cache policy
    cache_policy: lling_llang::wfst::CachePolicy,
    /// Maximum cache size
    max_cache_size: usize,
    /// Allow identity transitions (passthrough without rewrite)
    allow_identity: bool,
}

/// Cached state information.
#[derive(Clone)]
struct CachedRewriteState {
    is_final: bool,
    final_weight: TropicalWeight,
    transitions: SmallVec<[WeightedTransition<char, TropicalWeight>; 4]>,
}

/// Default maximum cache size
const DEFAULT_MAX_CACHE_SIZE: usize = 10_000;

impl RewriteWfst {
    /// Create a new empty rewrite WFST.
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            continuation_states: 0,
            cache: FxHashMap::default(),
            cache_policy: lling_llang::wfst::CachePolicy::CacheAll,
            max_cache_size: DEFAULT_MAX_CACHE_SIZE,
            allow_identity: true,
        }
    }

    /// Create a rewrite WFST with the given rules.
    pub fn with_rules(rules: Vec<RewriteRule>) -> Self {
        let continuation_states = continuation_state_count(&rules);
        Self {
            rules,
            continuation_states,
            cache: FxHashMap::default(),
            cache_policy: lling_llang::wfst::CachePolicy::CacheAll,
            max_cache_size: DEFAULT_MAX_CACHE_SIZE,
            allow_identity: true,
        }
    }

    /// Add a rewrite rule.
    pub fn add_rule(&mut self, input: &str, output: &str, cost: f64) {
        self.rules.push(RewriteRule::with_cost(input, output, cost));
        self.continuation_states = continuation_state_count(&self.rules);
        self.cache.clear(); // Invalidate cache
    }

    /// Add a pre-built rewrite rule.
    pub fn add_rewrite_rule(&mut self, rule: RewriteRule) {
        self.rules.push(rule);
        self.continuation_states = continuation_state_count(&self.rules);
        self.cache.clear();
    }

    /// Set whether identity transitions are allowed.
    ///
    /// When true, characters can pass through without rewriting.
    /// When false, characters must match a rule to be accepted.
    pub fn set_allow_identity(&mut self, allow: bool) {
        self.allow_identity = allow;
        self.cache.clear();
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
        let mut transitions = SmallVec::new();

        // State 0 is the initial/accepting state
        let is_final = state_id == 0;
        let final_weight = if is_final {
            TropicalWeight::one()
        } else {
            TropicalWeight::zero()
        };

        // Sort rules by priority (descending)
        let mut sorted_rules: Vec<(usize, &RewriteRule)> = self.rules.iter().enumerate().collect();
        sorted_rules.sort_by_key(|(_, r)| std::cmp::Reverse(r.priority));

        if state_id == 0 {
            for (rule_index, rule) in &sorted_rules {
                self.push_rule_step_transition(&mut transitions, *rule_index, rule, 0);
            }
        } else if let Some((rule_index, step)) = self.decode_continuation_state(state_id) {
            if let Some(rule) = self.rules.get(rule_index) {
                self.push_rule_step_transition(&mut transitions, rule_index, rule, step);
            }
        }

        // Add identity transitions if allowed
        if self.allow_identity && state_id == 0 {
            // Add wildcard identity for any printable character
            for c in ('a'..='z').chain('A'..='Z').chain('0'..='9') {
                transitions.push(WeightedTransition::new(
                    state_id,
                    Some(c),
                    Some(c),
                    0,
                    TropicalWeight::one(), // Zero cost for identity
                ));
            }
        }

        (is_final, final_weight, transitions)
    }

    fn push_rule_step_transition(
        &self,
        transitions: &mut SmallVec<[WeightedTransition<char, TropicalWeight>; 4]>,
        rule_index: usize,
        rule: &RewriteRule,
        step: usize,
    ) {
        let input_chars: Vec<char> = rule.input.chars().collect();
        let output_chars: Vec<char> = rule.output.chars().collect();
        let steps = input_chars.len().max(output_chars.len());

        if steps == 0 || step >= steps {
            return;
        }

        let from = if step == 0 {
            0
        } else {
            self.continuation_state(rule_index, step)
        };
        let to = if step + 1 == steps {
            0
        } else {
            self.continuation_state(rule_index, step + 1)
        };
        let input = input_chars.get(step).copied();
        let output = output_chars.get(step).copied();
        let weight = if step == 0 {
            TropicalWeight::new(rule.cost)
        } else {
            TropicalWeight::one()
        };

        transitions.push(WeightedTransition::new(from, input, output, to, weight));
    }

    fn continuation_state(&self, rule_index: usize, step: usize) -> StateId {
        debug_assert!(step > 0);
        let prior: usize = self
            .rules
            .iter()
            .take(rule_index)
            .map(|rule| rule_steps(rule).saturating_sub(1))
            .sum();
        1 + (prior + step - 1) as StateId
    }

    fn decode_continuation_state(&self, state_id: StateId) -> Option<(usize, usize)> {
        if state_id == 0 {
            return None;
        }

        let mut remaining = (state_id - 1) as usize;
        for (rule_index, rule) in self.rules.iter().enumerate() {
            let states = rule_steps(rule).saturating_sub(1);
            if remaining < states {
                return Some((rule_index, remaining + 1));
            }
            remaining = remaining.saturating_sub(states);
        }
        None
    }

    /// Ensure a state is computed and cached.
    fn ensure_state(&mut self, state: StateId) {
        if self.cache.contains_key(&state) {
            return;
        }

        let (is_final, final_weight, transitions) = self.compute_transitions(state);

        let cached = CachedRewriteState {
            is_final,
            final_weight,
            transitions,
        };

        if let lling_llang::wfst::CachePolicy::Lru { max_states } = self.cache_policy {
            let limit = if max_states > 0 {
                max_states
            } else {
                self.max_cache_size
            };
            if self.cache.len() >= limit {
                let to_remove = (self.cache.len() / 10).max(1);
                let keys: Vec<_> = self.cache.keys().take(to_remove).copied().collect();
                for key in keys {
                    self.cache.remove(&key);
                }
            }
        }

        self.cache.insert(state, cached);
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
        self.cache
            .get(&state)
            .map(|s| s.is_final)
            .unwrap_or(state == 0)
    }

    fn final_weight(&self, state: StateId) -> TropicalWeight {
        self.cache
            .get(&state)
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
        static EMPTY: &[WeightedTransition<char, TropicalWeight>] = &[];
        self.cache
            .get(&state)
            .map(|s| s.transitions.as_slice())
            .unwrap_or(EMPTY)
    }

    fn num_states(&self) -> usize {
        1 + self.continuation_states
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.rules.is_empty() && !self.allow_identity
    }

    #[inline]
    fn is_valid_state(&self, state: StateId) -> bool {
        (state as usize) < self.num_states()
    }
}

impl LazyWfst<char, TropicalWeight> for RewriteWfst {
    fn is_expanded(&self, state: StateId) -> bool {
        self.cache.contains_key(&state)
    }

    fn expand(&mut self, state: StateId) {
        self.ensure_state(state);
    }

    fn transitions_lazy(&mut self, state: StateId) -> &[WeightedTransition<char, TropicalWeight>] {
        self.ensure_state(state);
        self.transitions(state)
    }

    fn cache_policy(&self) -> lling_llang::wfst::CachePolicy {
        self.cache_policy
    }

    fn set_cache_policy(&mut self, policy: lling_llang::wfst::CachePolicy) {
        self.cache_policy = policy;
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

fn rule_steps(rule: &RewriteRule) -> usize {
    rule.input.chars().count().max(rule.output.chars().count())
}

fn continuation_state_count(rules: &[RewriteRule]) -> usize {
    rules
        .iter()
        .map(|rule| rule_steps(rule).saturating_sub(1))
        .sum()
}

/// Builder for common phonetic rewrite rules.
pub struct CommonPhoneticRules;

impl CommonPhoneticRules {
    /// English phonetic rules.
    pub fn english() -> Vec<RewriteRule> {
        vec![
            RewriteRule::with_cost("ph", "f", 0.1),
            RewriteRule::with_cost("gh", "f", 0.2), // rough -> ruff
            RewriteRule::with_cost("ck", "k", 0.1),
            RewriteRule::with_cost("qu", "kw", 0.1),
            RewriteRule::with_cost("x", "ks", 0.1),
            RewriteRule::with_cost("c", "k", 0.2), // Before a, o, u
            RewriteRule::with_cost("c", "s", 0.2), // Before e, i
        ]
    }

    /// German phonetic rules.
    pub fn german() -> Vec<RewriteRule> {
        vec![
            RewriteRule::with_cost("sch", "sh", 0.1),
            RewriteRule::with_cost("ch", "x", 0.1), // IPA [x] or [ç]
            RewriteRule::with_cost("ß", "ss", 0.1),
            RewriteRule::with_cost("ä", "ae", 0.1),
            RewriteRule::with_cost("ö", "oe", 0.1),
            RewriteRule::with_cost("ü", "ue", 0.1),
        ]
    }

    /// French phonetic rules.
    pub fn french() -> Vec<RewriteRule> {
        vec![
            RewriteRule::with_cost("eau", "o", 0.1),
            RewriteRule::with_cost("aux", "o", 0.1),
            RewriteRule::with_cost("ai", "e", 0.1),
            RewriteRule::with_cost("ph", "f", 0.1),
            RewriteRule::with_cost("qu", "k", 0.1),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rewrite_wfst_creation() {
        let wfst = RewriteWfst::new();
        assert_eq!(wfst.num_rules(), 0);
    }

    #[test]
    fn test_rewrite_wfst_add_rule() {
        let mut wfst = RewriteWfst::new();
        wfst.add_rule("ph", "f", 0.1);
        wfst.add_rule("c", "s", 0.2);

        assert_eq!(wfst.num_rules(), 2);
    }

    #[test]
    fn test_rewrite_wfst_with_rules() {
        let rules = vec![
            RewriteRule::with_cost("ph", "f", 0.1),
            RewriteRule::with_cost("ck", "k", 0.1),
        ];
        let wfst = RewriteWfst::with_rules(rules);

        assert_eq!(wfst.num_rules(), 2);
    }

    #[test]
    fn test_rewrite_wfst_start_state() {
        let wfst = RewriteWfst::new();
        assert_eq!(Wfst::start(&wfst), 0);
    }

    #[test]
    fn test_rewrite_wfst_expand() {
        let mut wfst = RewriteWfst::new();
        wfst.add_rule("a", "b", 0.1);

        assert!(!wfst.is_expanded(0));
        wfst.expand(0);
        assert!(wfst.is_expanded(0));
    }

    #[test]
    fn test_rewrite_wfst_transitions() {
        let mut wfst = RewriteWfst::new();
        wfst.add_rule("a", "b", 0.1);
        wfst.expand(0);

        let transitions = wfst.transitions(0);
        assert!(!transitions.is_empty());

        // Should have transition for 'a' -> 'b'
        let a_trans = transitions.iter().find(|t| t.input == Some('a'));
        assert!(a_trans.is_some());
        assert_eq!(
            a_trans.expect("expected Some a_trans in test").output,
            Some('b')
        );
    }

    #[test]
    fn test_rewrite_wfst_one_to_many_output_chain() {
        let mut wfst = RewriteWfst::new();
        wfst.set_allow_identity(false);
        wfst.add_rule("f", "ph", 0.1);
        wfst.expand(0);

        let first = wfst
            .transitions(0)
            .iter()
            .find(|t| t.input == Some('f') && t.output == Some('p'))
            .expect("expected f -> p first transition")
            .clone();
        assert_ne!(first.to, 0);
        assert_eq!(first.weight.value(), 0.1);

        wfst.expand(first.to);
        let continuation = wfst
            .transitions(first.to)
            .iter()
            .find(|t| t.input.is_none() && t.output == Some('h'))
            .expect("expected epsilon -> h continuation");
        assert_eq!(continuation.to, 0);
        assert_eq!(continuation.weight, TropicalWeight::one());
    }

    #[test]
    fn test_rewrite_wfst_many_to_one_input_chain() {
        let mut wfst = RewriteWfst::new();
        wfst.set_allow_identity(false);
        wfst.add_rule("ph", "f", 0.1);
        wfst.expand(0);

        let first = wfst
            .transitions(0)
            .iter()
            .find(|t| t.input == Some('p') && t.output == Some('f'))
            .expect("expected p -> f first transition")
            .clone();
        assert_ne!(first.to, 0);

        wfst.expand(first.to);
        let continuation = wfst
            .transitions(first.to)
            .iter()
            .find(|t| t.input == Some('h') && t.output.is_none())
            .expect("expected h -> epsilon continuation");
        assert_eq!(continuation.to, 0);
    }

    #[test]
    fn test_rewrite_wfst_identity() {
        let mut wfst = RewriteWfst::new();
        wfst.set_allow_identity(true);
        wfst.expand(0);

        let transitions = wfst.transitions(0);
        // Should have identity transitions for unrewritten characters
        let z_trans = transitions.iter().find(|t| t.input == Some('z'));
        assert!(z_trans.is_some());
        assert_eq!(
            z_trans.expect("expected Some z_trans in test").output,
            Some('z')
        );
    }

    #[test]
    fn test_common_english_rules() {
        let rules = CommonPhoneticRules::english();
        assert!(!rules.is_empty());

        let ph_rule = rules.iter().find(|r| r.input == "ph");
        assert!(ph_rule.is_some());
        assert_eq!(ph_rule.expect("expected Some ph_rule in test").output, "f");
    }

    #[test]
    fn test_common_german_rules() {
        let rules = CommonPhoneticRules::german();
        assert!(!rules.is_empty());

        let sch_rule = rules.iter().find(|r| r.input == "sch");
        assert!(sch_rule.is_some());
        assert_eq!(
            sch_rule.expect("expected Some sch_rule in test").output,
            "sh"
        );
    }

    #[test]
    fn test_rewrite_rule_priority() {
        let rule = RewriteRule::new("ph", "f").with_priority(10);
        assert_eq!(rule.priority, 10);
    }
}
