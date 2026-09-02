#![cfg(feature = "phonetic-rules")]

use duallity::{
    DirectStateSource, ExpansionFailureKind, LazyWfst, PhoneticNfaWfst, Semiring, StateExpansion,
    StateId, TropicalWeight, Wfst,
};
use liblevenshtein::phonetic::nfa::compiler::compile;
use liblevenshtein::phonetic::nfa::NFAChar;
use liblevenshtein::phonetic::regex::parse;
use lling_llang::wfst::CachePolicy;

fn compile_pattern(pattern: &str) -> NFAChar {
    let ast = parse(pattern).expect("phonetic pattern should parse");
    compile(&ast).expect("phonetic pattern should compile")
}

fn start_input_labels(wfst: &mut PhoneticNfaWfst) -> Vec<char> {
    wfst.transitions_lazy(Wfst::start(wfst))
        .iter()
        .filter_map(|transition| transition.input)
        .collect()
}

fn transition_for(
    wfst: &mut PhoneticNfaWfst,
    state: StateId,
    input: char,
) -> lling_llang::prelude::WeightedTransition<char, TropicalWeight> {
    wfst.transitions_lazy(state)
        .iter()
        .find(|transition| transition.input == Some(input))
        .cloned()
        .expect("expected transition for input")
}

#[test]
fn phonetic_nfa_wfst_creation_uses_default_weight_and_ascii_alphabet() {
    let nfa = compile_pattern("(ph|f)one");
    let wfst = PhoneticNfaWfst::new(nfa);

    assert_eq!(wfst.phonetic_weight(), 0.0);
    assert_eq!(wfst.alphabet().first(), Some(&' '));
    assert_eq!(wfst.alphabet().last(), Some(&'~'));
    assert!(!wfst.is_empty());
}

#[test]
fn phonetic_nfa_wfst_empty_language_still_has_start_state() {
    let nfa = NFAChar::new();
    let mut wfst = PhoneticNfaWfst::new(nfa);
    let start = Wfst::start(&wfst);

    assert_eq!(wfst.num_states(), 1);
    assert!(!wfst.is_empty());
    assert!(wfst.is_valid_state(start));
    assert!(!wfst.is_final(start));
    assert!(wfst.transitions_lazy(start).is_empty());
}

#[test]
fn phonetic_nfa_wfst_deduplicates_custom_alphabet() {
    let nfa = compile_pattern(".");
    let wfst = PhoneticNfaWfst::with_alphabet(nfa, ['b', 'a', 'b', 'c', 'a']);

    assert_eq!(wfst.alphabet(), ['b', 'a', 'c'].as_slice());
}

#[test]
fn phonetic_nfa_wfst_char_class_enumerates_full_small_range() {
    let nfa = compile_pattern("[a-c]");
    let mut wfst = PhoneticNfaWfst::with_alphabet(nfa, []);

    let labels = start_input_labels(&mut wfst);

    assert_eq!(labels, vec!['a', 'b', 'c']);
}

#[test]
fn phonetic_nfa_wfst_negated_class_uses_finite_alphabet() {
    let nfa = compile_pattern("[^a-c]");
    let mut wfst = PhoneticNfaWfst::with_alphabet(nfa, ['a', 'b', 'c', 'd', 'e']);

    let labels = start_input_labels(&mut wfst);

    assert_eq!(labels, vec!['d', 'e']);
}

#[test]
fn phonetic_nfa_wfst_any_uses_finite_alphabet() {
    let nfa = compile_pattern(".");
    let mut wfst = PhoneticNfaWfst::with_alphabet(nfa, ['x', 'y', 'x']);

    let labels = start_input_labels(&mut wfst);

    assert_eq!(labels, vec!['x', 'y']);
}

#[test]
fn phonetic_nfa_wfst_start_anchor_allows_initial_consumption() {
    let nfa = compile_pattern("^a");
    let mut wfst = PhoneticNfaWfst::new(nfa);

    assert_eq!(start_input_labels(&mut wfst), vec!['a']);
}

#[test]
fn phonetic_nfa_wfst_end_anchor_marks_terminal_state_final() {
    let nfa = compile_pattern("a$");
    let mut wfst = PhoneticNfaWfst::new(nfa);

    let start = Wfst::start(&wfst);
    let transition = transition_for(&mut wfst, start, 'a');
    wfst.expand(transition.to).expect("valid state expands");

    assert!(wfst.is_final(transition.to));
    assert_eq!(wfst.final_weight(transition.to), TropicalWeight::one());
}

#[test]
fn phonetic_nfa_wfst_reports_registered_final_state_before_expansion() {
    let nfa = compile_pattern("a$");
    let mut wfst = PhoneticNfaWfst::new(nfa);

    let start = Wfst::start(&wfst);
    let transition = transition_for(&mut wfst, start, 'a');

    assert!(wfst.is_valid_state(transition.to));
    assert!(!wfst.is_expanded(transition.to));
    assert!(wfst.is_final(transition.to));
    assert_eq!(wfst.final_weight(transition.to), TropicalWeight::one());
    assert!(!wfst.is_expanded(transition.to));
}

#[test]
fn phonetic_nfa_wfst_end_anchor_does_not_enable_following_consumption() {
    let nfa = compile_pattern("a\\z[b]");
    let mut wfst = PhoneticNfaWfst::new(nfa);

    let start = Wfst::start(&wfst);
    let transition = transition_for(&mut wfst, start, 'a');
    wfst.expand(transition.to).expect("valid state expands");

    assert!(!wfst.is_final(transition.to));
    assert!(wfst.transitions(transition.to).is_empty());
}

#[test]
fn phonetic_nfa_wfst_start_state_is_registered() {
    let nfa = compile_pattern("test");
    let wfst = PhoneticNfaWfst::new(nfa);

    assert_eq!(Wfst::start(&wfst), 0);
    assert!(wfst.is_valid_state(0));
}

#[test]
fn phonetic_nfa_statesource_classifies_invalid_state() {
    let nfa = compile_pattern("test");
    let wfst = PhoneticNfaWfst::new(nfa);

    match wfst.expand_state(StateId::MAX) {
        StateExpansion::Failed(failure) => {
            assert_eq!(failure.kind(), ExpansionFailureKind::InvalidState);
        }
        StateExpansion::Expanded { .. } => panic!("invalid state unexpectedly expanded"),
        StateExpansion::Cancelled(reason) => panic!("state expansion cancelled: {reason:?}"),
    }
}

#[test]
fn phonetic_nfa_wfst_expand_state_materializes_transitions() {
    let nfa = compile_pattern("(a|b)c");
    let mut wfst = PhoneticNfaWfst::new(nfa);

    let start = Wfst::start(&wfst);
    assert!(!wfst.is_expanded(start));

    wfst.expand(start).expect("start state expands");
    assert!(wfst.is_expanded(start));
    assert!(!wfst.transitions(start).is_empty());
}

#[test]
fn phonetic_nfa_wfst_uses_custom_transition_weight() {
    let nfa = compile_pattern("test");
    let wfst = PhoneticNfaWfst::with_phonetic_weight(nfa, 0.5).expect("valid phonetic weight");

    assert_eq!(wfst.phonetic_weight(), 0.5);
}

#[test]
fn phonetic_nfa_wfst_rejects_nan_weight() {
    let nfa = compile_pattern("test");
    let error = match PhoneticNfaWfst::with_phonetic_weight(nfa, f64::NAN) {
        Ok(_) => std::panic::panic_any("NaN phonetic weight should be rejected"),
        Err(error) => error,
    };

    assert_eq!(error.name(), "phonetic_weight");
    assert!(error.value().is_nan());
}

#[test]
fn phonetic_nfa_wfst_cache_policy_roundtrips() {
    let nfa = compile_pattern("test");
    let mut wfst = PhoneticNfaWfst::new(nfa);

    assert!(matches!(wfst.cache_policy(), CachePolicy::CacheAll));

    wfst.set_cache_policy(CachePolicy::Lru { max_states: 500 });
    assert!(matches!(wfst.cache_policy(), CachePolicy::Lru { .. }));
}

#[test]
fn phonetic_nfa_wfst_rejects_invalid_state_without_caching() {
    let nfa = compile_pattern("test");
    let mut wfst = PhoneticNfaWfst::new(nfa);
    let invalid_state = 1;

    assert!(!wfst.is_valid_state(invalid_state));
    assert!(wfst.expand(invalid_state).is_err());

    assert_eq!(wfst.computed_states(), 0);
    assert!(wfst.transitions_lazy(invalid_state).is_empty());
    assert_eq!(wfst.computed_states(), 0);
}

#[test]
fn phonetic_nfa_wfst_no_cache_policy_uses_scratch_only() {
    let nfa = compile_pattern("(a|b)c");
    let mut wfst = PhoneticNfaWfst::new(nfa);
    wfst.set_cache_policy(CachePolicy::NoCache);

    let start = Wfst::start(&wfst);
    let transition_count = wfst.transitions_lazy(start).len();

    assert!(transition_count > 0);
    assert_eq!(wfst.computed_states(), 0);
    assert!(!wfst.is_expanded(start));
    assert_eq!(wfst.transitions(start).len(), transition_count);
    assert_eq!(wfst.total_transitions(), transition_count);
}

#[test]
fn phonetic_nfa_wfst_lru_policy_evicts_least_recently_used_state() {
    let nfa = compile_pattern("(a|b)c");
    let mut wfst = PhoneticNfaWfst::new(nfa);
    wfst.set_cache_policy(CachePolicy::Lru { max_states: 1 });

    let start = Wfst::start(&wfst);
    let next = wfst
        .transitions_lazy(start)
        .first()
        .expect("expected start transition")
        .to;

    assert!(wfst.is_expanded(start));
    assert_eq!(wfst.computed_states(), 1);

    wfst.expand(next).expect("valid state expands");

    assert_eq!(wfst.computed_states(), 1);
    assert!(!wfst.is_expanded(start));
    assert!(wfst.is_expanded(next));
}

#[test]
fn phonetic_nfa_wfst_state_count_grows_from_registry() {
    let nfa = compile_pattern("abc");
    let mut wfst = PhoneticNfaWfst::new(nfa);

    assert_eq!(wfst.num_states(), 1);

    wfst.expand(0).expect("start state expands");
    assert!(wfst.num_states() >= 1);
}

#[test]
fn phonetic_nfa_wfst_statesource_computes_start() {
    let nfa = compile_pattern("(a|b)c");
    let wfst = PhoneticNfaWfst::new(nfa);

    let StateExpansion::Expanded { transitions, .. } = wfst.expand_state(0) else {
        panic!("expected computed transitions");
    };
    assert!(transitions
        .iter()
        .any(|transition| transition.input == Some('a')));
    assert!(transitions
        .iter()
        .any(|transition| transition.input == Some('b')));
}

#[test]
fn phonetic_nfa_wfst_statesource_matches_lazy_expansion() {
    let nfa = compile_pattern("(a|b)c");
    let mut lazy = PhoneticNfaWfst::new(nfa.clone());
    let source = PhoneticNfaWfst::new(nfa);

    lazy.expand(0).expect("start state expands");
    let lazy_transitions = lazy.transitions(0).to_vec();
    let StateExpansion::Expanded {
        transitions: source_transitions,
        ..
    } = source.expand_state(0)
    else {
        panic!("expected computed transitions");
    };

    assert_eq!(source_transitions.as_slice(), lazy_transitions.as_slice());
}
