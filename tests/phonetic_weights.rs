#![cfg(feature = "phonetic-rules")]

use duallity::{PhoneticPipelineBuilder, PhoneticWfst, TropicalWeight, Wfst};
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use liblevenshtein::phonetic::nfa::compiler::compile;
use liblevenshtein::phonetic::nfa::NFAChar;
use liblevenshtein::phonetic::regex::parse;
use lling_llang::prelude::{LazyWfst, StateId};

fn compile_pattern(pattern: &str) -> NFAChar {
    let ast = parse(pattern).expect("phonetic pattern should parse");
    compile(&ast).expect("phonetic pattern should compile")
}

fn follow_term(wfst: &mut PhoneticWfst<DynamicDawgChar<()>>, term: &str) -> (StateId, f64) {
    let mut state = wfst.start();
    let mut path_weight = 0.0;

    for ch in term.chars() {
        let (next, weight) = {
            let transitions = wfst.transitions_lazy(state);
            let transition = transitions
                .iter()
                .find(|transition| transition.input == Some(ch) && transition.output == Some(ch))
                .expect("term should be traversable in the phonetic WFST");

            (transition.to, transition.weight.value())
        };

        path_weight += weight;
        state = next;
    }

    wfst.expand(state).expect("valid state expands");
    (state, path_weight)
}

#[test]
fn phonetic_wfst_reports_registered_final_state_before_expansion() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["a"]);
    let nfa = compile_pattern("a");
    let mut wfst = PhoneticWfst::new(&dict, nfa, 0);

    let start = wfst.start();
    let target = wfst
        .transitions_lazy(start)
        .iter()
        .find(|transition| transition.input == Some('a') && transition.output == Some('a'))
        .map(|transition| transition.to)
        .expect("exact phonetic transition should exist");
    let computed_after_start = wfst.computed_states();

    assert!(wfst.is_valid_state(target));
    assert!(!wfst.is_expanded(target));
    assert!(wfst.is_final(target));
    assert_eq!(wfst.final_weight(target), TropicalWeight::new(0.0));
    assert!(!wfst.is_expanded(target));
    assert_eq!(wfst.computed_states(), computed_after_start);
}

#[test]
fn phonetic_wfst_num_states_uses_source_hint_not_cache_len() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help"]);
    let nfa = compile_pattern("helo");
    let wfst = PhoneticWfst::new(&dict, nfa, 2);

    assert_eq!(wfst.computed_states(), 0);
    assert!(wfst.num_states() > wfst.computed_states());
}

#[test]
fn phonetic_wfst_nonfinal_dictionary_prefix_has_infinite_final_weight() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["ab"]);
    let nfa = compile_pattern("a");
    let mut wfst = PhoneticWfst::new(&dict, nfa, 0);

    let start = wfst.start();
    let prefix = wfst
        .transitions_lazy(start)
        .iter()
        .find(|transition| transition.input == Some('a') && transition.output == Some('a'))
        .map(|transition| transition.to)
        .expect("prefix transition should exist");

    assert!(!wfst.is_final(prefix));
    assert!(wfst.final_weight(prefix).value().is_infinite());

    wfst.expand(prefix).expect("valid prefix state expands");

    assert!(!wfst.is_final(prefix));
    assert!(wfst.final_weight(prefix).value().is_infinite());
}

#[test]
fn phonetic_wfst_applies_phonetic_edges_and_weighted_edit_final_cost() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["bat"]);
    let nfa = compile_pattern("cat");
    let mut wfst =
        PhoneticWfst::with_weights(&dict, nfa, 1, 0.25, 2.5).expect("valid phonetic weights");

    let (state, transition_cost) = follow_term(&mut wfst, "bat");

    assert!((transition_cost - 0.75).abs() <= f64::EPSILON);
    assert!(wfst.is_final(state));
    assert!((wfst.final_weight(state).value() - 2.5).abs() <= f64::EPSILON);
}

#[test]
fn pipeline_builder_passes_weight_config_to_dictionary_backed_wfst() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["bat"]);
    let mut wfst = PhoneticPipelineBuilder::new()
        .phonetic_pattern("cat")
        .max_edit_distance(1)
        .phonetic_weight(0.4)
        .expect("valid phonetic weight")
        .edit_weight(3.0)
        .expect("valid edit weight")
        .dictionary(&dict)
        .build()
        .expect("pipeline should build");

    assert_eq!(wfst.phonetic_weight(), 0.4);
    assert_eq!(wfst.edit_weight(), 3.0);

    let (state, transition_cost) = follow_term(&mut wfst, "bat");

    assert!((transition_cost - 1.2).abs() <= f64::EPSILON);
    assert!(wfst.is_final(state));
    assert!((wfst.final_weight(state).value() - 3.0).abs() <= f64::EPSILON);
}
