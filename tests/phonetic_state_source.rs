#![cfg(feature = "phonetic-rules")]

use duallity::{
    DirectStateSource, PhoneticStateSource, StateExpansion, StateSource, TropicalWeight,
};
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use liblevenshtein::phonetic::nfa::compiler::compile;
use liblevenshtein::phonetic::nfa::NFAChar;
use liblevenshtein::phonetic::regex::parse;

fn compile_pattern(pattern: &str) -> NFAChar {
    let ast = parse(pattern).expect("phonetic pattern should parse");
    compile(&ast).expect("phonetic pattern should compile")
}

fn transition_outputs(state: &StateExpansion<char, TropicalWeight>) -> Vec<char> {
    match state {
        StateExpansion::Expanded { transitions, .. } => transitions
            .iter()
            .filter_map(|transition| transition.output)
            .collect(),
        StateExpansion::Failed(failure) => panic!("state expansion failed: {failure}"),
        StateExpansion::Cancelled(reason) => panic!("state expansion cancelled: {reason:?}"),
    }
}

fn follow_term(
    source: &PhoneticStateSource<DynamicDawgChar<()>>,
    term: &str,
) -> StateExpansion<char, TropicalWeight> {
    let mut state_id = source.start();

    for ch in term.chars() {
        state_id = match source.expand_state(state_id) {
            StateExpansion::Expanded { transitions, .. } => transitions
                .iter()
                .find(|transition| transition.output == Some(ch))
                .map(|transition| transition.to)
                .expect("expected dictionary transition while following test term"),
            StateExpansion::Failed(failure) => panic!("state expansion failed: {failure}"),
            StateExpansion::Cancelled(reason) => panic!("state expansion cancelled: {reason:?}"),
        };
    }

    source.expand_state(state_id)
}

#[test]
fn phonetic_state_source_start_anchor_allows_initial_dictionary_edge() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["a"]);
    let source = PhoneticStateSource::new(&dict, compile_pattern("^a"), 0);

    let start_state = source.expand_state(source.start());

    assert_eq!(transition_outputs(&start_state), vec!['a']);
}

#[test]
fn phonetic_state_source_end_anchor_marks_dictionary_terminal_final() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["a"]);
    let source = PhoneticStateSource::new(&dict, compile_pattern("a$"), 0);

    match follow_term(&source, "a") {
        StateExpansion::Expanded {
            is_final,
            final_weight,
            ..
        } => {
            assert!(is_final);
            assert_eq!(final_weight.value(), 0.0);
        }
        StateExpansion::Failed(failure) => panic!("state expansion failed: {failure}"),
        StateExpansion::Cancelled(reason) => panic!("state expansion cancelled: {reason:?}"),
    }
}

#[test]
fn phonetic_state_source_end_anchor_does_not_enable_following_dictionary_edge() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["ab"]);
    let source = PhoneticStateSource::new(&dict, compile_pattern("a\\z[b]"), 0);

    let after_a = match source.expand_state(source.start()) {
        StateExpansion::Expanded { transitions, .. } => transitions
            .iter()
            .find(|transition| transition.output == Some('a'))
            .map(|transition| transition.to)
            .expect("expected first dictionary edge"),
        StateExpansion::Failed(failure) => panic!("state expansion failed: {failure}"),
        StateExpansion::Cancelled(reason) => panic!("state expansion cancelled: {reason:?}"),
    };

    match source.expand_state(after_a) {
        StateExpansion::Expanded {
            is_final,
            transitions,
            ..
        } => {
            assert!(!is_final);
            assert!(!transitions
                .iter()
                .any(|transition| transition.output == Some('b')));
        }
        StateExpansion::Failed(failure) => panic!("state expansion failed: {failure}"),
        StateExpansion::Cancelled(reason) => panic!("state expansion cancelled: {reason:?}"),
    }
}

#[test]
fn phonetic_state_source_creation_uses_default_weights() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["phone", "fone", "help"]);
    let source = PhoneticStateSource::new(&dict, compile_pattern("(ph|f)one"), 2);

    assert_eq!(source.max_distance(), 2);
    assert_eq!(source.phonetic_weight(), 0.0);
    assert_eq!(source.edit_weight(), 1.0);
}

#[test]
fn phonetic_state_source_start_state_computes_eagerly() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
    let source = PhoneticStateSource::new(&dict, compile_pattern("test"), 1);

    assert_eq!(source.start(), 0);
    assert!(matches!(
        source.expand_state(source.start()),
        StateExpansion::Expanded { .. }
    ));
}

#[test]
fn phonetic_state_source_uses_custom_phonetic_weight() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["phone"]);
    let source =
        PhoneticStateSource::with_phonetic_weight(&dict, compile_pattern("(ph|f)one"), 2, 0.5)
            .expect("valid phonetic weight");

    assert_eq!(source.phonetic_weight(), 0.5);
    assert_eq!(source.edit_weight(), 1.0);
}

#[test]
fn phonetic_state_source_rejects_invalid_weights() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["phone"]);
    let error = match PhoneticStateSource::with_weights(
        &dict,
        compile_pattern("phone"),
        2,
        0.25,
        f64::INFINITY,
    ) {
        Ok(_) => std::panic::panic_any("infinite edit weight should be rejected"),
        Err(error) => error,
    };

    assert_eq!(error.name(), "edit_weight");
    assert!(error.value().is_infinite());
}

#[test]
fn phonetic_state_source_applies_phonetic_transition_weight() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["cat"]);
    let source = PhoneticStateSource::with_weights(&dict, compile_pattern("cat"), 1, 0.25, 1.0)
        .expect("valid phonetic weights");

    match source.expand_state(source.start()) {
        StateExpansion::Expanded { transitions, .. } => {
            let transition = transitions
                .iter()
                .find(|transition| transition.output == Some('c'))
                .expect("expected first dictionary transition");
            assert_eq!(transition.weight.value(), 0.25);
        }
        StateExpansion::Failed(failure) => panic!("state expansion failed: {failure}"),
        StateExpansion::Cancelled(reason) => panic!("state expansion cancelled: {reason:?}"),
    }
}

#[test]
fn phonetic_state_source_applies_edit_final_weight() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["bat"]);
    let source = PhoneticStateSource::with_weights(&dict, compile_pattern("cat"), 1, 0.0, 2.5)
        .expect("valid phonetic weights");

    match follow_term(&source, "bat") {
        StateExpansion::Expanded {
            is_final,
            final_weight,
            ..
        } => {
            assert!(is_final);
            assert_eq!(final_weight.value(), 2.5);
        }
        StateExpansion::Failed(failure) => panic!("state expansion failed: {failure}"),
        StateExpansion::Cancelled(reason) => panic!("state expansion cancelled: {reason:?}"),
    }
}

#[test]
fn phonetic_state_source_computes_start_state() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["phone", "help"]);
    let source = PhoneticStateSource::new(&dict, compile_pattern("phone"), 1);

    assert!(matches!(
        source.expand_state(source.start()),
        StateExpansion::Expanded { .. }
    ));
}

#[test]
fn phonetic_state_source_num_states_hint_is_nonzero() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["test", "rest", "best"]);
    let source = PhoneticStateSource::new(&dict, compile_pattern("test"), 1);

    let hint = source.num_states_hint();
    assert!(hint.is_some());
    assert!(hint.expect("expected state hint") > 0);
}
