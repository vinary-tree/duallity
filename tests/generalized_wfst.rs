use duallity::{
    GeneralizedWfst, GeneralizedWfstBuilder, LazyState, LazyWfst, StateSource, TropicalWeight,
    WeightedTransition, Wfst,
};
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use liblevenshtein::transducer::{
    OperationSet, OperationSetBuilder, OperationType, SubstitutionSet,
};
use lling_llang::wfst::CachePolicy;

fn pending_eager_state<T>(message: &str) -> T {
    std::panic::panic_any(message.to_owned())
}

type TestDict = DynamicDawgChar<()>;

fn transitions_for(
    wfst: &mut GeneralizedWfst<TestDict>,
    state: lling_llang::prelude::StateId,
) -> Vec<WeightedTransition<char, TropicalWeight>> {
    wfst.transitions_lazy(state).to_vec()
}

fn find_transition(
    transitions: &[WeightedTransition<char, TropicalWeight>],
    input: Option<char>,
    output: Option<char>,
    weight: Option<f64>,
) -> WeightedTransition<char, TropicalWeight> {
    transitions
        .iter()
        .find(|transition| {
            transition.input == input
                && transition.output == output
                && weight.is_none_or(|expected| {
                    (transition.weight.value() - expected).abs() <= 1e-9
                })
        })
        .cloned()
        .expect("expected transition not found")
}

#[test]
fn generalized_wfst_creation() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help", "world"]);
    let wfst = GeneralizedWfst::new(&dict, "helo", 2, OperationSet::standard());

    assert!(!wfst.is_empty());
    assert_eq!(wfst.query(), "helo");
    assert_eq!(wfst.max_distance(), 2);
}

#[test]
fn generalized_wfst_start_state_is_registered() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
    let wfst = GeneralizedWfst::new(&dict, "tset", 2, OperationSet::standard());

    let start = Wfst::start(&wfst);
    assert!(wfst.is_valid_state(start));
}

#[test]
fn generalized_wfst_deduplicates_equivalent_runtime_operations() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["a"]);
    let operations = OperationSetBuilder::new()
        .with_match()
        .with_operation(OperationType::new(1, 1, 0.0, "match_alias"))
        .build();
    let mut wfst = GeneralizedWfst::new(&dict, "a", 0, operations);

    let start = Wfst::start(&wfst);
    let transitions = transitions_for(&mut wfst, start);

    assert_eq!(
        transitions
            .iter()
            .filter(|transition| {
                transition.input == Some('a')
                    && transition.output == Some('a')
                    && transition.weight == TropicalWeight::new(0.0)
            })
            .count(),
        1
    );
}

#[test]
fn generalized_wfst_lazy_expansion_materializes_start_state() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help"]);
    let mut wfst = GeneralizedWfst::new(&dict, "helo", 2, OperationSet::standard());

    let start = Wfst::start(&wfst);
    assert!(!wfst.is_expanded(start));

    wfst.expand(start);
    assert!(wfst.is_expanded(start));
}

#[test]
fn generalized_wfst_reports_registered_final_state_before_expansion() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["a"]);
    let mut wfst = GeneralizedWfst::new(&dict, "a", 0, OperationSet::standard());

    let start = Wfst::start(&wfst);
    let exact = transitions_for(&mut wfst, start)
        .iter()
        .find(|transition| transition.input == Some('a') && transition.output == Some('a'))
        .map(|transition| transition.to)
        .expect("exact transition should exist");
    let computed_after_start = wfst.computed_states();

    assert!(wfst.is_valid_state(exact));
    assert!(!wfst.is_expanded(exact));
    assert!(wfst.is_final(exact));
    assert_eq!(wfst.final_weight(exact), TropicalWeight::new(0.0));
    assert!(!wfst.is_expanded(exact));
    assert_eq!(wfst.computed_states(), computed_after_start);
}

#[test]
fn generalized_wfst_no_cache_policy_uses_scratch_only() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help"]);
    let mut wfst = GeneralizedWfst::new(&dict, "helo", 2, OperationSet::standard());
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
fn generalized_state_source_computes_eagerly() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["ab"]);
    let wfst = GeneralizedWfst::new(&dict, "ab", 1, OperationSet::standard());

    match StateSource::compute_state(&wfst, Wfst::start(&wfst)) {
        LazyState::Computed { transitions, .. } => assert!(!transitions.is_empty()),
        LazyState::Pending => {
            pending_eager_state("generalized WFST state source should compute eagerly")
        }
    }
}

#[test]
fn generalized_wfst_expands_non_start_product_states() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["ab"]);
    let mut wfst = GeneralizedWfst::new(&dict, "ab", 1, OperationSet::standard());

    let start = Wfst::start(&wfst);
    let start_transitions = transitions_for(&mut wfst, start);
    let first = find_transition(&start_transitions, Some('a'), Some('a'), Some(0.0));
    assert!(wfst.is_valid_state(first.to));

    let second_transitions = transitions_for(&mut wfst, first.to);
    let second = find_transition(&second_transitions, Some('b'), Some('b'), Some(0.0));

    wfst.expand(second.to);
    assert!(wfst.is_valid_state(second.to));
    assert!(wfst.is_final(second.to));
    assert_eq!(wfst.final_weight(second.to).value(), 0.0);
}

#[test]
fn generalized_wfst_standard_match_counts_unicode_chars() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["é"]);
    let mut wfst = GeneralizedWfst::new(&dict, "é", 0, OperationSet::standard());

    let start = Wfst::start(&wfst);
    let start_transitions = transitions_for(&mut wfst, start);
    let exact = find_transition(&start_transitions, Some('é'), Some('é'), Some(0.0));

    wfst.expand(exact.to);

    assert!(wfst.is_final(exact.to));
    assert_eq!(wfst.final_weight(exact.to).value(), 0.0);
}

#[test]
fn generalized_wfst_unrestricted_substitution_counts_unicode_chars() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["é"]);
    let mut wfst = GeneralizedWfst::new(&dict, "e", 1, OperationSet::standard());

    let start = Wfst::start(&wfst);
    let start_transitions = transitions_for(&mut wfst, start);
    let substitution = find_transition(&start_transitions, Some('e'), Some('é'), Some(1.0));

    wfst.expand(substitution.to);

    assert!(wfst.is_final(substitution.to));
    assert_eq!(wfst.final_weight(substitution.to).value(), 0.0);
}

#[test]
fn generalized_wfst_restricted_substitution_counts_unicode_chars() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["é"]);
    let mut substitutions = SubstitutionSet::new();
    substitutions.allow_str("é", "e");
    let operations = OperationSetBuilder::new()
        .with_operation(OperationType::with_restriction(
            1,
            1,
            0.25,
            substitutions,
            "accent_fold",
        ))
        .build();
    let mut wfst = GeneralizedWfst::new(&dict, "e", 1, operations);

    let start = Wfst::start(&wfst);
    let start_transitions = transitions_for(&mut wfst, start);
    let substitution = find_transition(&start_transitions, Some('e'), Some('é'), Some(0.25));

    wfst.expand(substitution.to);

    assert!(wfst.is_final(substitution.to));
}

#[test]
fn generalized_wfst_transposition_reaches_final() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["ab"]);
    let mut wfst = GeneralizedWfst::new(&dict, "ba", 1, OperationSet::with_transposition());

    let start = Wfst::start(&wfst);
    let first_hops = transitions_for(&mut wfst, start);
    let mut found = false;

    for first in first_hops
        .iter()
        .filter(|transition| {
            transition.input == Some('b')
                && transition.output == Some('a')
                && (transition.weight.value() - 1.0).abs() <= 1e-9
        })
        .cloned()
    {
        let second_hops = transitions_for(&mut wfst, first.to);
        if let Some(second) = second_hops
            .iter()
            .find(|transition| transition.input == Some('a') && transition.output == Some('b'))
            .cloned()
        {
            wfst.expand(second.to);
            if wfst.is_final(second.to) {
                found = true;
                break;
            }
        }
    }

    assert!(found, "expected transposition path ba -> ab");
}

#[test]
fn generalized_wfst_restricted_digraph_uses_continuation_state() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["phone"]);
    let mut wfst = GeneralizedWfstBuilder::new(&dict)
        .query("fone")
        .max_distance(1)
        .with_phonetic_digraphs()
        .build()
        .expect("builder should produce WFST");

    let start = Wfst::start(&wfst);
    let start_transitions = transitions_for(&mut wfst, start);
    let phonetic = find_transition(&start_transitions, Some('f'), Some('p'), Some(0.15));

    let continuation_transitions = transitions_for(&mut wfst, phonetic.to);
    let continuation = find_transition(&continuation_transitions, None, Some('h'), Some(0.0));

    let o_transitions = transitions_for(&mut wfst, continuation.to);
    let o = find_transition(&o_transitions, Some('o'), Some('o'), Some(0.0));
    let n_transitions = transitions_for(&mut wfst, o.to);
    let n = find_transition(&n_transitions, Some('n'), Some('n'), Some(0.0));
    let e_transitions = transitions_for(&mut wfst, n.to);
    let e = find_transition(&e_transitions, Some('e'), Some('e'), Some(0.0));

    wfst.expand(e.to);
    assert!(wfst.is_final(e.to));
    assert_eq!(wfst.final_weight(e.to).value(), 0.0);
}

#[test]
fn generalized_wfst_builder_enables_transposition() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["test", "tset"]);
    let result = GeneralizedWfstBuilder::new(&dict)
        .query("tset")
        .max_distance(1)
        .with_transposition()
        .build();

    assert!(result.is_ok());
}

#[test]
fn generalized_wfst_builder_builds_standard_wfst() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "world"]);
    let result = GeneralizedWfstBuilder::new(&dict)
        .query("helo")
        .max_distance(2)
        .with_standard_ops()
        .build();

    assert!(result.is_ok());
    let wfst = result.unwrap();
    assert_eq!(wfst.query(), "helo");
}

#[test]
fn generalized_wfst_builder_requires_query() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
    let result = GeneralizedWfstBuilder::new(&dict).build();

    assert!(result.is_err());
}

#[test]
fn generalized_wfst_builder_accepts_phonetic_operations() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["phone", "graph"]);
    let result = GeneralizedWfstBuilder::new(&dict)
        .query("fone")
        .max_distance(2)
        .with_phonetic_digraphs()
        .build();

    assert!(result.is_ok());
}

#[test]
fn generalized_wfst_transitions_after_expansion() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["ab", "abc"]);
    let mut wfst = GeneralizedWfst::new(&dict, "ab", 1, OperationSet::standard());

    let start = Wfst::start(&wfst);
    wfst.expand(start);

    assert!(!wfst.transitions(start).is_empty());
}

#[test]
fn generalized_wfst_cache_operations_clear_cached_states() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
    let mut wfst = GeneralizedWfst::new(&dict, "test", 1, OperationSet::standard());

    wfst.expand(0);
    let before = wfst.computed_states();

    wfst.clear_cache();
    assert_eq!(wfst.computed_states(), 0);
    assert!(before > 0);
}
