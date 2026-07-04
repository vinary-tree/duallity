use liblevenshtein::phonetic::nfa::{NFAChar, StateSet, TransitionLabelChar};
use smallvec::SmallVec;

/// Normalize zero-width anchors for full-string WFST traversal.
///
/// The upstream product automaton only follows epsilon and consuming NFA labels.
/// For full-string dictionary traversal, start anchors are true only before the
/// first dictionary character and end anchors are true only at dictionary-term
/// finality checks.
pub(crate) fn normalize_full_string_anchors(mut nfa: NFAChar) -> NFAChar {
    let start = nfa.start();
    let initial_closure = start_anchor_closure(&nfa);
    for state in initial_closure.iter() {
        if state != start {
            nfa.add_epsilon(start, state);
        }
    }

    for state_index in 0..nfa.num_states() {
        let Ok(state) = u32::try_from(state_index) else {
            break;
        };
        let mut singleton = StateSet::new();
        singleton.insert(state);
        if state_set_can_reach_final_through_end_anchors(&nfa, &singleton) {
            nfa.set_final(state, true);
        }
    }

    nfa.finalize();
    nfa
}

pub(crate) fn start_anchor_closure(nfa: &NFAChar) -> StateSet {
    position_anchor_closure(
        nfa,
        nfa.epsilon_closure_single(nfa.start()),
        is_start_anchor,
    )
}

pub(crate) fn state_set_can_reach_final_through_end_anchors(
    nfa: &NFAChar,
    state_set: &StateSet,
) -> bool {
    let terminal_closure = position_anchor_closure(nfa, state_set.clone(), is_end_anchor);
    terminal_closure.iter().any(|state| nfa.is_final(state))
}

fn position_anchor_closure(
    nfa: &NFAChar,
    initial: StateSet,
    anchor_matches: fn(&TransitionLabelChar) -> bool,
) -> StateSet {
    let mut closure = initial;
    let mut stack = SmallVec::<[u32; 8]>::with_capacity(closure.len());
    stack.extend(closure.iter());

    while let Some(state) = stack.pop() {
        for trans in nfa.transitions_from(state) {
            if (trans.label.is_epsilon() || anchor_matches(&trans.label))
                && closure.insert(trans.to)
            {
                stack.push(trans.to);
            }
        }
    }

    closure
}

#[inline]
fn is_start_anchor(label: &TransitionLabelChar) -> bool {
    matches!(
        label,
        TransitionLabelChar::StartOfLine | TransitionLabelChar::StartOfInput
    )
}

#[inline]
fn is_end_anchor(label: &TransitionLabelChar) -> bool {
    matches!(
        label,
        TransitionLabelChar::EndOfLine
            | TransitionLabelChar::EndOfInput
            | TransitionLabelChar::EndOfInputStrict
    )
}
