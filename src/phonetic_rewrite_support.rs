use lling_llang::prelude::{StateId, TropicalWeight, WeightedTransition};
use smallvec::SmallVec;

use crate::fx_hash_map_with_capacity;
use crate::phonetic_rewrite_wfst::RewriteRule;

pub(crate) const HASH_PRUNE_THRESHOLD: usize = 16;

#[derive(Clone)]
pub(crate) struct PreparedRewriteRule {
    pub(crate) input: SmallVec<[char; 4]>,
    pub(crate) output: SmallVec<[char; 4]>,
    pub(crate) cost_weight: TropicalWeight,
    pub(crate) steps: usize,
}

impl PreparedRewriteRule {
    pub(crate) fn new(rule: &RewriteRule) -> Self {
        let input = rule.input.chars().collect::<SmallVec<[char; 4]>>();
        let output = rule.output.chars().collect::<SmallVec<[char; 4]>>();
        let cost_weight = TropicalWeight::new(rule.cost);
        let steps = input.len().max(output.len());

        Self {
            input,
            output,
            cost_weight,
            steps,
        }
    }

    pub(crate) fn continuation_steps(&self) -> usize {
        self.steps.saturating_sub(1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ContinuationState {
    pub(crate) rule_index: usize,
    pub(crate) step: usize,
}

pub(crate) struct PreparedRuleMetadata {
    pub(crate) prepared_rules: Vec<PreparedRewriteRule>,
    pub(crate) priority_order: Vec<usize>,
    pub(crate) continuation_end_offsets: Vec<usize>,
    pub(crate) continuation_lookup: Vec<ContinuationState>,
    pub(crate) continuation_states: usize,
}

impl PreparedRuleMetadata {
    pub(crate) fn from_rules(rules: &[RewriteRule]) -> Self {
        let mut prepared_rules = Vec::with_capacity(rules.len());
        let mut continuation_states = 0usize;

        for rule in rules {
            let prepared = PreparedRewriteRule::new(rule);
            continuation_states = continuation_states.saturating_add(prepared.continuation_steps());
            prepared_rules.push(prepared);
        }

        let mut continuation_end_offsets = Vec::with_capacity(rules.len());
        let mut continuation_lookup = Vec::with_capacity(continuation_states);

        for (rule_index, prepared) in prepared_rules.iter().enumerate() {
            for step in 1..prepared.steps {
                continuation_lookup.push(ContinuationState { rule_index, step });
            }
            continuation_end_offsets.push(continuation_lookup.len());
        }

        let mut priority_order = (0..rules.len()).collect::<Vec<_>>();
        priority_order.sort_by(|&left, &right| {
            rules[right]
                .priority
                .cmp(&rules[left].priority)
                .then_with(|| left.cmp(&right))
        });

        Self {
            prepared_rules,
            priority_order,
            continuation_end_offsets,
            continuation_lookup,
            continuation_states,
        }
    }
}

pub(crate) fn validate_rewrite_rule(rule: &RewriteRule) -> Result<(), crate::InvalidWeightError> {
    crate::validate_finite_nonnegative_weight("rewrite rule cost", rule.cost).map(|_| ())
}

pub(crate) fn validate_rewrite_rules(
    rules: &[RewriteRule],
) -> Result<(), crate::InvalidWeightError> {
    for rule in rules {
        validate_rewrite_rule(rule)?;
    }
    Ok(())
}

pub(crate) fn prune_dominated_transitions(
    transitions: &mut SmallVec<[WeightedTransition<char, TropicalWeight>; 4]>,
) {
    if transitions.len() < 2 {
        return;
    }

    if transitions.len() < HASH_PRUNE_THRESHOLD {
        prune_dominated_transitions_linear(transitions);
        return;
    }

    let mut pruned =
        SmallVec::<[WeightedTransition<char, TropicalWeight>; 4]>::with_capacity(transitions.len());
    let mut index_by_key = fx_hash_map_with_capacity(transitions.len());

    for transition in transitions.drain(..) {
        let key = RewriteTransitionKey::from(&transition);
        let Some(&existing_index) = index_by_key.get(&key) else {
            index_by_key.insert(key, pruned.len());
            pruned.push(transition);
            continue;
        };

        let existing = &mut pruned[existing_index];
        if transition.weight.value() < existing.weight.value() {
            *existing = transition;
        }
    }

    *transitions = pruned;
}

fn prune_dominated_transitions_linear(
    transitions: &mut SmallVec<[WeightedTransition<char, TropicalWeight>; 4]>,
) {
    let mut pruned =
        SmallVec::<[WeightedTransition<char, TropicalWeight>; 4]>::with_capacity(transitions.len());

    for transition in transitions.drain(..) {
        let key = RewriteTransitionKey::from(&transition);
        let Some(existing) = pruned
            .iter_mut()
            .find(|existing| RewriteTransitionKey::from(&**existing) == key)
        else {
            pruned.push(transition);
            continue;
        };

        if transition.weight.value() < existing.weight.value() {
            *existing = transition;
        }
    }

    *transitions = pruned;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct RewriteTransitionKey {
    from: StateId,
    input: Option<char>,
    output: Option<char>,
    to: StateId,
}

impl From<&WeightedTransition<char, TropicalWeight>> for RewriteTransitionKey {
    fn from(transition: &WeightedTransition<char, TropicalWeight>) -> Self {
        Self {
            from: transition.from,
            input: transition.input,
            output: transition.output,
            to: transition.to,
        }
    }
}

#[inline]
pub(crate) fn usize_from_state_id(id: StateId) -> usize {
    usize::try_from(id).unwrap_or(usize::MAX)
}

#[inline]
pub(crate) fn rewrite_state_count(continuation_states: usize) -> usize {
    let addressable_states = usize::try_from(StateId::MAX)
        .ok()
        .and_then(|max_state_id| max_state_id.checked_add(1))
        .unwrap_or(usize::MAX);

    continuation_states
        .saturating_add(1)
        .min(addressable_states)
}
