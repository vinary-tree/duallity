//! Integration tests for WFST functionality.
//!
//! These tests verify that liblevenshtein integrates correctly with
//! lling-llang's WFST infrastructure.

use duallity::{
    DictionaryBackend, GeneralizedWfst, LazyState, LevenshteinStateSource, LevenshteinWfst,
    Semiring, StateSource, TropicalWeight, UniversalLevenshteinStateSource,
    UniversalLevenshteinWfst, Wfst,
};
#[cfg(feature = "phonetic-rules")]
use duallity::{PhoneticStateSource, PhoneticWfst};
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use libdictenstein::{Dictionary, DictionaryNode};
use liblevenshtein::transducer::universal::Standard;
use liblevenshtein::transducer::{Algorithm, OperationSet};
use lling_llang::backend::LatticeBackend;
use lling_llang::wfst::{LazyWfst, LazyWfstWrapper};

fn pending_eager_state<T>(message: &str) -> T {
    std::panic::panic_any(message.to_owned())
}

#[derive(Clone)]
struct UnknownLenDict;

#[derive(Clone)]
struct UnknownLenNode;

impl Dictionary for UnknownLenDict {
    type Node = UnknownLenNode;

    fn root(&self) -> Self::Node {
        UnknownLenNode
    }

    fn len(&self) -> Option<usize> {
        None
    }
}

impl DictionaryNode for UnknownLenNode {
    type Unit = char;
    type SnapshotCursor = libdictenstein::SnapshotTraversalCursor;
    type SnapshotGraphValueHandle = libdictenstein::SnapshotTraversalCursor;

    fn is_final(&self) -> bool {
        false
    }

    fn transition(&self, _label: Self::Unit) -> Option<Self> {
        None
    }

    fn edges(&self) -> Box<dyn Iterator<Item = (Self::Unit, Self)> + '_> {
        Box::new(std::iter::empty())
    }
}

fn state_source_transition_labels(
    dict_terms: Vec<&str>,
    query: &str,
) -> Vec<(Option<char>, Option<char>, f64)> {
    let dict = DynamicDawgChar::<()>::from_terms(dict_terms);
    let source = LevenshteinStateSource::new(&dict, query, 1);
    match source.compute_state(source.start()) {
        LazyState::Computed { transitions, .. } => transitions
            .iter()
            .map(|transition| {
                (
                    transition.input,
                    transition.output,
                    transition.weight.value(),
                )
            })
            .collect(),
        LazyState::Pending => {
            pending_eager_state("Levenshtein state source should compute eagerly")
        }
    }
}

fn labels_contain(
    labels: &[(Option<char>, Option<char>, f64)],
    input: Option<char>,
    output: Option<char>,
    weight: f64,
) -> bool {
    labels
        .iter()
        .any(|(actual_input, actual_output, actual_weight)| {
            *actual_input == input
                && *actual_output == output
                && (*actual_weight - weight).abs() <= f64::EPSILON
        })
}

fn assert_start_is_below_num_states<W>(wfst: &W)
where
    W: Wfst<char, TropicalWeight>,
{
    let start = usize::try_from(wfst.start()).expect("start state ID should fit usize");
    assert!(
        start < wfst.num_states(),
        "start state {} must be below num_states {}",
        start,
        wfst.num_states()
    );
}

fn state_source_accepts_any_path(source: &LevenshteinStateSource<DynamicDawgChar<()>>) -> bool {
    let mut stack = vec![source.start()];
    let mut seen = std::collections::HashSet::new();

    while let Some(state_id) = stack.pop() {
        if !seen.insert(state_id) {
            continue;
        }

        match source.compute_state(state_id) {
            LazyState::Computed {
                is_final,
                transitions,
                ..
            } => {
                if is_final {
                    return true;
                }
                stack.extend(transitions.iter().map(|transition| transition.to));
            }
            LazyState::Pending => {
                pending_eager_state("Levenshtein state source should compute eagerly")
            }
        }
    }

    false
}

#[test]
fn test_levenshtein_wfst_basic() {
    // Create a simple dictionary
    let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help", "world", "hell"]);

    // Create WFST for query "helo" with max distance 2
    let wfst = LevenshteinWfst::new(&dict, "helo", 2);

    // Verify basic properties
    assert_eq!(wfst.max_distance(), 2);
    assert_eq!(wfst.query(), "helo");
    assert!(!wfst.is_empty());
}

#[test]
fn test_levenshtein_wfst_start_state() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
    let wfst = LevenshteinWfst::new(&dict, "tset", 2);

    // Start state should exist
    let start = wfst.start();
    assert!(wfst.is_valid_state(start));
}

#[test]
fn test_levenshtein_wfst_lazy_expansion() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help"]);
    let mut wfst = LevenshteinWfst::new(&dict, "helo", 2);

    // Initially nothing expanded
    let start = wfst.start();
    assert!(!wfst.is_expanded(start));
    assert_eq!(wfst.computed_states(), 0);

    // Expand start state
    wfst.expand(start);
    assert!(wfst.is_expanded(start));
    assert!(wfst.computed_states() >= 1);
}

#[test]
fn test_levenshtein_wfst_num_states_uses_source_hint_not_cache_len() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help"]);
    let wfst = LevenshteinWfst::new(&dict, "helo", 2);

    assert_eq!(wfst.computed_states(), 0);
    assert!(wfst.num_states() > wfst.computed_states());
}

#[test]
fn test_levenshtein_wfst_reports_registered_final_state_before_expansion() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["a"]);
    let mut wfst = LevenshteinWfst::new(&dict, "a", 0);

    let start = wfst.start();
    let target = wfst
        .transitions_lazy(start)
        .iter()
        .find(|transition| transition.input == Some('a') && transition.output == Some('a'))
        .map(|transition| transition.to)
        .expect("exact transition should exist");
    let computed_after_start = wfst.computed_states();

    assert!(wfst.is_valid_state(target));
    assert!(!wfst.is_expanded(target));
    assert!(wfst.is_final(target));
    assert_eq!(wfst.final_weight(target), TropicalWeight::new(0.0));
    assert!(!wfst.is_expanded(target));
    assert_eq!(wfst.computed_states(), computed_after_start);
}

#[test]
fn test_levenshtein_wfst_clone() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
    let wfst = LevenshteinWfst::new(&dict, "tset", 1);
    let cloned = wfst.clone();

    assert_eq!(wfst.start(), cloned.start());
    assert_eq!(wfst.max_distance(), cloned.max_distance());
}

#[test]
fn test_universal_wfst_tracks_exact_query_position() {
    use liblevenshtein::transducer::universal::Standard;

    let dict = DynamicDawgChar::<()>::from_terms(vec!["ab"]);
    let mut wfst = UniversalLevenshteinWfst::<Standard, _>::new(&dict, "xy", 2);

    let start = wfst.start();
    let first_target = wfst
        .transitions_lazy(start)
        .iter()
        .find(|transition| {
            transition.input == Some('x')
                && transition.output == Some('a')
                && transition.weight.value() == 0.0
        })
        .map(|transition| transition.to)
        .expect("expected first substitution transition");

    let second_transitions = wfst.transitions_lazy(first_target);
    assert!(
        second_transitions
            .iter()
            .any(|transition| transition.input == Some('y') && transition.output == Some('b')),
        "second dictionary edge must consume the second query character"
    );
    assert!(
        !second_transitions
            .iter()
            .any(|transition| transition.input == Some('x') && transition.output == Some('b')),
        "query position must not be recovered from abstract offsets"
    );
}

#[test]
fn test_universal_wfst_reports_registered_final_state_before_expansion() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["a"]);
    let mut wfst = UniversalLevenshteinWfst::<Standard, _>::new(&dict, "a", 0);

    let start = wfst.start();
    let target = wfst
        .transitions_lazy(start)
        .iter()
        .find(|transition| transition.input == Some('a') && transition.output == Some('a'))
        .map(|transition| transition.to)
        .expect("exact transition should exist");
    let computed_after_start = wfst.computed_states();

    assert!(wfst.is_valid_state(target));
    assert!(!wfst.is_expanded(target));
    assert!(wfst.is_final(target));
    assert_eq!(wfst.final_weight(target), TropicalWeight::new(0.0));
    assert!(!wfst.is_expanded(target));
    assert_eq!(wfst.computed_states(), computed_after_start);
}

#[test]
fn test_universal_wfst_num_states_uses_source_hint_not_cache_len() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help"]);
    let wfst = UniversalLevenshteinWfst::<Standard, _>::new(&dict, "helo", 2);

    assert_eq!(wfst.computed_states(), 0);
    assert!(wfst.num_states() > wfst.computed_states());
}

#[test]
fn test_universal_wfst_num_states_covers_registered_transition_targets() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["ab"]);
    let mut wfst = UniversalLevenshteinWfst::<Standard, _>::new(&dict, "ab", 1);

    let start = wfst.start();
    let target = wfst
        .transitions_lazy(start)
        .iter()
        .find(|transition| transition.output.is_some())
        .map(|transition| transition.to)
        .expect("dictionary transition should be registered");
    let target = usize::try_from(target).expect("target state ID should fit usize");

    assert!(
        target < wfst.num_states(),
        "target state {} must be below num_states {}",
        target,
        wfst.num_states()
    );
}

#[test]
fn test_levenshtein_state_source_basic() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help", "world"]);
    let source = LevenshteinStateSource::new(&dict, "helo", 2);

    // Start state should be 0
    let start = source.start();
    assert_eq!(start, 0);

    // Should have a state hint
    assert!(source.num_states_hint().is_some());
}

#[test]
fn test_levenshtein_state_source_oversized_distance_does_not_panic() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["a"]);
    let source = LevenshteinStateSource::new(&dict, "a", usize::MAX);

    assert!(source.compute_state(source.start()).is_computed());
}

#[test]
fn test_unknown_dictionary_length_propagates_to_state_hints() {
    let dict = UnknownLenDict;

    let levenshtein = LevenshteinStateSource::new(&dict, "helo", 2);
    assert_eq!(levenshtein.num_states_hint(), None);

    let universal = UniversalLevenshteinStateSource::<Standard, _>::new(&dict, "helo", 2);
    assert_eq!(universal.num_states_hint(), None);

    let generalized = GeneralizedWfst::new(&dict, "helo", 2, OperationSet::standard());
    assert_eq!(generalized.num_states_hint(), None);
}

#[test]
fn test_unknown_dictionary_length_lazy_wfsts_keep_start_below_num_states() {
    let dict = UnknownLenDict;

    let levenshtein = LevenshteinWfst::new(&dict, "helo", 2);
    assert_start_is_below_num_states(&levenshtein);

    let universal = UniversalLevenshteinWfst::<Standard, _>::new(&dict, "helo", 2);
    assert_start_is_below_num_states(&universal);

    let generalized = GeneralizedWfst::new(&dict, "helo", 2, OperationSet::standard());
    assert_start_is_below_num_states(&generalized);
}

#[cfg(feature = "phonetic-rules")]
#[test]
fn test_unknown_dictionary_length_propagates_to_phonetic_state_hint() {
    use liblevenshtein::phonetic::nfa::compile;
    use liblevenshtein::phonetic::regex::parse;

    let dict = UnknownLenDict;
    let nfa = compile(&parse("helo").expect("parse")).expect("compile");
    let phonetic = PhoneticStateSource::new(&dict, nfa, 2);

    assert_eq!(phonetic.num_states_hint(), None);
}

#[cfg(feature = "phonetic-rules")]
#[test]
fn test_unknown_dictionary_length_phonetic_wfst_keeps_start_below_num_states() {
    use liblevenshtein::phonetic::nfa::compile;
    use liblevenshtein::phonetic::regex::parse;

    let dict = UnknownLenDict;
    let nfa = compile(&parse("helo").expect("parse")).expect("compile");
    let phonetic = PhoneticWfst::new(&dict, nfa, 2);

    assert_start_is_below_num_states(&phonetic);
}

#[cfg(feature = "phonetic-rules")]
#[test]
fn test_phonetic_wfst_num_states_covers_registered_transition_targets() {
    use liblevenshtein::phonetic::nfa::compile;
    use liblevenshtein::phonetic::regex::parse;

    let dict = DynamicDawgChar::<()>::from_terms(vec!["phone"]);
    let nfa = compile(&parse("phone").expect("parse")).expect("compile");
    let mut wfst = PhoneticWfst::new(&dict, nfa, 1);

    let start = wfst.start();
    let target = wfst
        .transitions_lazy(start)
        .iter()
        .find(|transition| transition.output.is_some())
        .map(|transition| transition.to)
        .expect("dictionary transition should be registered");
    let target = usize::try_from(target).expect("target state ID should fit usize");

    assert!(
        target < wfst.num_states(),
        "target state {} must be below num_states {}",
        target,
        wfst.num_states()
    );
}

#[test]
fn test_levenshtein_state_source_compute_state() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help"]);
    let source = LevenshteinStateSource::new(&dict, "helo", 2);

    // Compute start state
    let state = source.compute_state(source.start());
    assert!(state.is_computed());
}

#[test]
fn test_levenshtein_state_source_clone_preserves_start_state() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
    let source = LevenshteinStateSource::new(&dict, "tset", 2);
    let cloned = source.clone();

    assert_eq!(source.start(), cloned.start());
}

#[test]
fn test_levenshtein_state_source_transition_labels_preserve_transducer_sides() {
    let substitution = state_source_transition_labels(vec!["cat"], "bat");
    assert!(labels_contain(&substitution, Some('b'), Some('c'), 1.0));

    let insertion = state_source_transition_labels(vec!["cat"], "at");
    assert!(labels_contain(&insertion, None, Some('c'), 1.0));

    let deletion = state_source_transition_labels(vec!["at"], "cat");
    assert!(labels_contain(&deletion, Some('c'), None, 1.0));

    let identity = state_source_transition_labels(vec!["cat"], "cat");
    assert!(labels_contain(&identity, Some('c'), Some('c'), 0.0));
}

#[test]
fn test_levenshtein_state_source_prunes_paths_above_max_distance() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["bbb"]);
    let source = LevenshteinStateSource::new(&dict, "a", 1);

    let mut stack = vec![source.start()];
    let mut seen = std::collections::HashSet::new();

    while let Some(state_id) = stack.pop() {
        if !seen.insert(state_id) {
            continue;
        }

        match source.compute_state(state_id) {
            LazyState::Computed {
                is_final,
                transitions,
                ..
            } => {
                assert!(!is_final, "distance-3 term must not be accepted with k=1");
                stack.extend(transitions.iter().map(|transition| transition.to));
            }
            LazyState::Pending => {
                pending_eager_state("Levenshtein state source should compute eagerly")
            }
        }
    }

    assert!(!seen.is_empty());
}

#[test]
fn test_levenshtein_state_source_accepts_paths_at_max_distance() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["b"]);
    let source = LevenshteinStateSource::new(&dict, "a", 1);

    let substitution_target = match source.compute_state(source.start()) {
        LazyState::Computed { transitions, .. } => transitions
            .iter()
            .find(|transition| {
                transition.input == Some('a')
                    && transition.output == Some('b')
                    && (transition.weight.value() - 1.0).abs() <= f64::EPSILON
            })
            .map(|transition| transition.to)
            .expect("expected substitution transition"),
        LazyState::Pending => {
            pending_eager_state("Levenshtein state source should compute eagerly")
        }
    };

    match source.compute_state(substitution_target) {
        LazyState::Computed {
            is_final,
            final_weight,
            ..
        } => {
            assert!(is_final);
            assert_eq!(final_weight.value(), 0.0);
        }
        LazyState::Pending => {
            pending_eager_state("Levenshtein state source should compute eagerly")
        }
    }
}

#[test]
fn test_levenshtein_state_source_transposition_accepts_adjacent_swap() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["ab"]);
    let source = LevenshteinStateSource::with_algorithm(&dict, "ba", 1, Algorithm::Transposition);

    let first_targets = match source.compute_state(source.start()) {
        LazyState::Computed { transitions, .. } => transitions
            .iter()
            .filter(|transition| {
                transition.input == Some('b')
                    && transition.output == Some('a')
                    && (transition.weight.value() - 1.0).abs() <= f64::EPSILON
            })
            .map(|transition| transition.to)
            .collect::<Vec<_>>(),
        LazyState::Pending => {
            pending_eager_state("Levenshtein state source should compute eagerly")
        }
    };

    assert!(
        !first_targets.is_empty(),
        "expected at least one first-half transposition transition"
    );

    let mut reaches_final = false;

    for first_target in first_targets {
        let second_targets = match source.compute_state(first_target) {
            LazyState::Computed { transitions, .. } => transitions
                .iter()
                .filter(|transition| {
                    transition.input == Some('a')
                        && transition.output == Some('b')
                        && transition.weight.value() == 0.0
                })
                .map(|transition| transition.to)
                .collect::<Vec<_>>(),
            LazyState::Pending => {
                pending_eager_state("Levenshtein state source should compute eagerly")
            }
        };

        for second_target in second_targets {
            if let LazyState::Computed {
                is_final,
                final_weight,
                ..
            } = source.compute_state(second_target)
            {
                reaches_final |= is_final && final_weight.value() == 0.0;
            }
        }
    }

    assert!(reaches_final, "adjacent swap should accept at distance 1");
}

#[test]
fn test_levenshtein_state_source_standard_rejects_adjacent_swap_at_one_edit() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["ab"]);
    let source = LevenshteinStateSource::new(&dict, "ba", 1);

    assert!(
        !state_source_accepts_any_path(&source),
        "standard Levenshtein should require two substitutions for an adjacent swap"
    );
}

#[test]
fn test_levenshtein_state_source_merge_and_split_accepts_merge() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["m"]);
    let source = LevenshteinStateSource::with_algorithm(&dict, "rn", 1, Algorithm::MergeAndSplit);

    let first_targets = match source.compute_state(source.start()) {
        LazyState::Computed { transitions, .. } => transitions
            .iter()
            .filter(|transition| {
                transition.input == Some('r')
                    && transition.output == Some('m')
                    && (transition.weight.value() - 1.0).abs() <= f64::EPSILON
            })
            .map(|transition| transition.to)
            .collect::<Vec<_>>(),
        LazyState::Pending => {
            pending_eager_state("Levenshtein state source should compute eagerly")
        }
    };

    let mut reaches_final = false;

    for first_target in first_targets {
        let second_targets = match source.compute_state(first_target) {
            LazyState::Computed { transitions, .. } => transitions
                .iter()
                .filter(|transition| {
                    transition.input == Some('n')
                        && transition.output.is_none()
                        && transition.weight.value() == 0.0
                })
                .map(|transition| transition.to)
                .collect::<Vec<_>>(),
            LazyState::Pending => {
                pending_eager_state("Levenshtein state source should compute eagerly")
            }
        };

        for second_target in second_targets {
            if let LazyState::Computed {
                is_final,
                final_weight,
                ..
            } = source.compute_state(second_target)
            {
                reaches_final |= is_final && final_weight.value() == 0.0;
            }
        }
    }

    assert!(reaches_final, "merge should accept at distance 1");
}

#[test]
fn test_levenshtein_state_source_merge_and_split_accepts_split() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["rn"]);
    let source = LevenshteinStateSource::with_algorithm(&dict, "m", 1, Algorithm::MergeAndSplit);

    let first_targets = match source.compute_state(source.start()) {
        LazyState::Computed { transitions, .. } => transitions
            .iter()
            .filter(|transition| {
                transition.input == Some('m')
                    && transition.output == Some('r')
                    && (transition.weight.value() - 1.0).abs() <= f64::EPSILON
            })
            .map(|transition| transition.to)
            .collect::<Vec<_>>(),
        LazyState::Pending => {
            pending_eager_state("Levenshtein state source should compute eagerly")
        }
    };

    let mut reaches_final = false;

    for first_target in first_targets {
        let second_targets = match source.compute_state(first_target) {
            LazyState::Computed { transitions, .. } => transitions
                .iter()
                .filter(|transition| {
                    transition.input.is_none()
                        && transition.output == Some('n')
                        && transition.weight.value() == 0.0
                })
                .map(|transition| transition.to)
                .collect::<Vec<_>>(),
            LazyState::Pending => {
                pending_eager_state("Levenshtein state source should compute eagerly")
            }
        };

        for second_target in second_targets {
            if let LazyState::Computed {
                is_final,
                final_weight,
                ..
            } = source.compute_state(second_target)
            {
                reaches_final |= is_final && final_weight.value() == 0.0;
            }
        }
    }

    assert!(reaches_final, "split should accept at distance 1");
}

#[test]
fn test_levenshtein_state_source_standard_rejects_merge_split_at_one_edit() {
    let merge_dict = DynamicDawgChar::<()>::from_terms(vec!["m"]);
    let merge_source = LevenshteinStateSource::new(&merge_dict, "rn", 1);
    assert!(
        !state_source_accepts_any_path(&merge_source),
        "standard Levenshtein should not merge two query chars at distance 1"
    );

    let split_dict = DynamicDawgChar::<()>::from_terms(vec!["rn"]);
    let split_source = LevenshteinStateSource::new(&split_dict, "m", 1);
    assert!(
        !state_source_accepts_any_path(&split_source),
        "standard Levenshtein should not split one query char at distance 1"
    );
}

#[test]
fn test_levenshtein_state_source_with_lazy_wrapper() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help"]);
    let source = LevenshteinStateSource::new(&dict, "helo", 2);

    // Wrap in LazyWfstWrapper
    let mut lazy_wfst = LazyWfstWrapper::new(source);

    // Initially no states computed
    assert_eq!(lazy_wfst.computed_states(), 0);

    // Access start state
    let start = lazy_wfst.start();
    let _transitions = lazy_wfst.transitions_lazy(start);

    // Now should have computed at least 1 state
    assert!(lazy_wfst.computed_states() >= 1);
}

#[test]
fn test_dictionary_backend_basic() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "world"]);
    let mut backend = DictionaryBackend::new(dict);

    // Initially empty vocabulary
    assert_eq!(backend.vocab_size(), 0);

    // Intern some words
    let id1 = backend.intern("hello");
    let id2 = backend.intern("world");

    assert_eq!(backend.vocab_size(), 2);
    assert_ne!(id1, id2);

    // Same word should return same ID
    let id3 = backend.intern("hello");
    assert_eq!(id1, id3);
}

#[test]
fn test_dictionary_backend_lookup() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
    let mut backend = DictionaryBackend::new(dict);

    let id = backend.intern("test");
    assert_eq!(backend.lookup(id), Some("test"));
    assert_eq!(backend.lookup(999), None);
}

#[test]
fn test_dictionary_backend_contains() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "world"]);
    let backend = DictionaryBackend::new(dict);

    // Contains checks the underlying dictionary
    assert!(backend.contains("hello"));
    assert!(backend.contains("world"));
    assert!(!backend.contains("missing"));
}

#[test]
fn test_dictionary_backend_iter() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["a", "b", "c"]);
    let mut backend = DictionaryBackend::new(dict);

    backend.intern("a");
    backend.intern("b");
    backend.intern("c");

    let entries: Vec<_> = backend.iter().collect();
    assert_eq!(entries.len(), 3);
}

#[test]
fn test_tropical_weight_semantics() {
    // Verify TropicalWeight behaves as expected
    let w1 = TropicalWeight::new(1.0);
    let w2 = TropicalWeight::new(2.0);

    // Plus is min
    let sum = w1.plus(&w2);
    assert_eq!(sum.value(), 1.0);

    // Times is add
    let prod = w1.times(&w2);
    assert_eq!(prod.value(), 3.0);

    // Zero is infinity
    let zero = TropicalWeight::zero();
    assert!(zero.is_infinite());

    // One is 0.0
    let one = TropicalWeight::one();
    assert_eq!(one.value(), 0.0);
}

#[test]
fn test_wfst_transposition_algorithm() {
    use liblevenshtein::transducer::Algorithm;

    let dict = DynamicDawgChar::<()>::from_terms(vec!["ab"]);
    let mut wfst = LevenshteinWfst::with_algorithm(&dict, "ba", 1, Algorithm::Transposition);

    assert_eq!(wfst.algorithm(), Algorithm::Transposition);

    let start = wfst.start();
    let first_targets = wfst
        .transitions_lazy(start)
        .iter()
        .filter(|transition| {
            transition.input == Some('b')
                && transition.output == Some('a')
                && (transition.weight.value() - 1.0).abs() <= f64::EPSILON
        })
        .map(|transition| transition.to)
        .collect::<Vec<_>>();

    let mut reaches_final = false;

    for first_target in first_targets {
        let second_targets = wfst
            .transitions_lazy(first_target)
            .iter()
            .filter(|transition| {
                transition.input == Some('a')
                    && transition.output == Some('b')
                    && transition.weight.value() == 0.0
            })
            .map(|transition| transition.to)
            .collect::<Vec<_>>();

        for second_target in second_targets {
            wfst.expand(second_target);
            reaches_final |=
                wfst.is_final(second_target) && wfst.final_weight(second_target).value() == 0.0;
        }
    }

    assert!(reaches_final, "adjacent swap should accept at distance 1");
}

#[test]
fn test_wfst_merge_and_split_algorithm() {
    use liblevenshtein::transducer::Algorithm;

    let merge_dict = DynamicDawgChar::<()>::from_terms(vec!["m"]);
    let mut merge_wfst =
        LevenshteinWfst::with_algorithm(&merge_dict, "rn", 1, Algorithm::MergeAndSplit);

    assert_eq!(merge_wfst.algorithm(), Algorithm::MergeAndSplit);

    let start = merge_wfst.start();
    let merge_first_targets = merge_wfst
        .transitions_lazy(start)
        .iter()
        .filter(|transition| {
            transition.input == Some('r')
                && transition.output == Some('m')
                && (transition.weight.value() - 1.0).abs() <= f64::EPSILON
        })
        .map(|transition| transition.to)
        .collect::<Vec<_>>();

    let mut merge_reaches_final = false;

    for first_target in merge_first_targets {
        let second_targets = merge_wfst
            .transitions_lazy(first_target)
            .iter()
            .filter(|transition| {
                transition.input == Some('n')
                    && transition.output.is_none()
                    && transition.weight.value() == 0.0
            })
            .map(|transition| transition.to)
            .collect::<Vec<_>>();

        for second_target in second_targets {
            merge_wfst.expand(second_target);
            merge_reaches_final |= merge_wfst.is_final(second_target)
                && merge_wfst.final_weight(second_target).value() == 0.0;
        }
    }

    assert!(
        merge_reaches_final,
        "merge should accept two query chars as one dictionary char at distance 1"
    );

    let split_dict = DynamicDawgChar::<()>::from_terms(vec!["rn"]);
    let mut split_wfst =
        LevenshteinWfst::with_algorithm(&split_dict, "m", 1, Algorithm::MergeAndSplit);

    let start = split_wfst.start();
    let split_first_targets = split_wfst
        .transitions_lazy(start)
        .iter()
        .filter(|transition| {
            transition.input == Some('m')
                && transition.output == Some('r')
                && (transition.weight.value() - 1.0).abs() <= f64::EPSILON
        })
        .map(|transition| transition.to)
        .collect::<Vec<_>>();

    let mut split_reaches_final = false;

    for first_target in split_first_targets {
        let second_targets = split_wfst
            .transitions_lazy(first_target)
            .iter()
            .filter(|transition| {
                transition.input.is_none()
                    && transition.output == Some('n')
                    && transition.weight.value() == 0.0
            })
            .map(|transition| transition.to)
            .collect::<Vec<_>>();

        for second_target in second_targets {
            split_wfst.expand(second_target);
            split_reaches_final |= split_wfst.is_final(second_target)
                && split_wfst.final_weight(second_target).value() == 0.0;
        }
    }

    assert!(
        split_reaches_final,
        "split should accept one query char as two dictionary chars at distance 1"
    );
}

#[test]
fn test_dictionary_backend_with_vocabulary() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "world", "test"]);
    let terms = vec!["hello".to_string(), "world".to_string()];
    let backend = DictionaryBackend::with_vocabulary(dict, terms);

    assert_eq!(backend.vocab_size(), 2);
    assert!(backend.get_id("hello").is_some());
    assert!(backend.get_id("world").is_some());
    assert!(backend.get_id("test").is_none()); // Not pre-populated
}
