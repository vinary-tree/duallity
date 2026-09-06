use duallity::{
    DirectStateSource, GeneralizedWfst, GeneralizedWfstBuilder, LazyWfst, StateExpansion,
    TropicalWeight, WeightedTransition, Wfst,
};
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use liblevenshtein::transducer::{
    generalized::GeneralizedAutomaton, OperationApplicability, OperationSet, OperationSetBuilder,
    OperationType, SubstitutionSet,
};
use lling_llang::wfst::CachePolicy;
use std::collections::{HashMap, VecDeque};

type TestDict = DynamicDawgChar<()>;

#[derive(Clone)]
struct UnaryDictionary {
    length: usize,
    inspected_edges: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    endless: bool,
}

#[derive(Clone)]
struct UnaryNode {
    remaining: usize,
    inspected_edges: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    endless: bool,
}

impl libdictenstein::Dictionary for UnaryDictionary {
    type Node = UnaryNode;
    fn root(&self) -> UnaryNode {
        UnaryNode {
            remaining: self.length,
            inspected_edges: self.inspected_edges.clone(),
            endless: self.endless,
        }
    }
    fn len(&self) -> Option<usize> {
        Some(1)
    }
}

impl libdictenstein::DictionaryNode for UnaryNode {
    type Unit = char;
    type SnapshotCursor = ();
    type SnapshotGraphValueHandle = ();
    fn is_final(&self) -> bool {
        self.remaining == 0
    }
    fn transition(&self, label: char) -> Option<Self> {
        (label == 'a' && self.remaining > 0).then(|| Self {
            remaining: self.remaining - 1,
            ..self.clone()
        })
    }
    fn edges(&self) -> Box<dyn Iterator<Item = (char, Self)> + '_> {
        let next = (self.remaining > 0).then(|| Self {
            remaining: self.remaining - 1,
            ..self.clone()
        });
        let mut returned = false;
        Box::new(std::iter::from_fn(move || {
            self.inspected_edges
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if !self.endless && returned {
                return None;
            }
            returned = true;
            next.clone().map(|node| ('a', node))
        }))
    }
    fn edge_count(&self) -> Option<usize> {
        Some(usize::MAX)
    }
}

#[test]
fn maximum_width_traversal_and_emission_are_stack_safe() {
    std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(|| {
            let dictionary = UnaryDictionary {
                length: 4096,
                inspected_edges: Default::default(),
                endless: false,
            };
            let operations = OperationSetBuilder::new()
                .with_operation(OperationType::new(4096, 0, 1.0, "wide_delete"))
                .build();
            let mut wfst = GeneralizedWfst::new(&dictionary, "", 1, operations);
            let mut state = 0;
            for _ in 0..4096 {
                let arcs = wfst
                    .try_transitions(state)
                    .expect("bounded unary expansion");
                assert_eq!(arcs.len(), 1);
                assert_eq!(arcs[0].output, Some('a'));
                state = arcs[0].to;
            }
            assert!(wfst.is_final(state));
            assert_eq!(wfst.num_states(), 4097);
        })
        .expect("small-stack worker")
        .join()
        .expect("iterative traversal");
}

#[test]
fn work_limits_stop_endless_iterators_and_ignore_hostile_edge_hints() {
    use duallity::GeneralizedWfstLimits;
    let dictionary = UnaryDictionary {
        length: 1,
        inspected_edges: Default::default(),
        endless: true,
    };
    let operations = OperationSetBuilder::new()
        .with_operation(OperationType::new(1, 0, 1.0, "delete"))
        .build();
    let mut wfst = GeneralizedWfst::try_new_with_limits(
        &dictionary,
        "",
        1,
        operations,
        GeneralizedWfstLimits {
            max_work_units_per_expansion: 100,
            ..Default::default()
        },
    )
    .expect("bounded constructor");
    assert!(wfst.try_transitions(0).is_err());
    assert!(
        dictionary
            .inspected_edges
            .load(std::sync::atomic::Ordering::Relaxed)
            <= 100
    );
    assert_eq!(wfst.num_states(), 1);
    assert_eq!(wfst.computed_states(), 0);
}

#[test]
fn exact_fractional_language_agrees_with_an_independent_integer_grid() {
    // Independent alignment grid in integer tenths: free equality, substitution
    // 3, deletion 2, insertion 1, and a two-source/one-query rule costing 4.
    let operations = OperationSetBuilder::new()
        .with_match()
        .with_operation(OperationType::new(1, 1, 0.3, "replace"))
        .with_operation(OperationType::new(1, 0, 0.2, "delete"))
        .with_operation(OperationType::new(0, 1, 0.1, "insert"))
        .with_operation(OperationType::new(2, 1, 0.4, "merge"))
        .build();
    let words = binary_words(3);
    for source in &words {
        for query in &words {
            let x: Vec<_> = source.chars().collect();
            let y: Vec<_> = query.chars().collect();
            let mut grid = vec![vec![usize::MAX; y.len() + 1]; x.len() + 1];
            grid[0][0] = 0;
            for i in 0..=x.len() {
                for j in 0..=y.len() {
                    let current = grid[i][j];
                    if current == usize::MAX {
                        continue;
                    }
                    if i < x.len() {
                        grid[i + 1][j] = grid[i + 1][j].min(current + 2);
                    }
                    if j < y.len() {
                        grid[i][j + 1] = grid[i][j + 1].min(current + 1);
                    }
                    if i < x.len() && j < y.len() {
                        let cost = if x[i] == y[j] { 0 } else { 3 };
                        grid[i + 1][j + 1] = grid[i + 1][j + 1].min(current + cost);
                    }
                    if i + 2 <= x.len() && j < y.len() {
                        grid[i + 2][j + 1] = grid[i + 2][j + 1].min(current + 4);
                    }
                }
            }
            let exact = grid[x.len()][y.len()];
            for budget in 0..=1 {
                let result = relation_cost(source, query, budget, operations.clone());
                if exact <= usize::from(budget) * 10 {
                    assert!(
                        result.is_some_and(|value| (value - exact as f64 / 10.0).abs() < 1e-12),
                        "{source:?} -> {query:?}: expected {exact} tenths, got {result:?}"
                    );
                } else {
                    assert_eq!(result, None, "{source:?} -> {query:?}");
                }
            }
        }
    }
}

#[test]
fn decimal_tenths_accept_thirty_edits_and_reject_thirty_one() {
    for (source_width, query_width) in [(1, 1), (1, 0), (0, 1)] {
        let operations = OperationSetBuilder::new()
            .with_operation(OperationType::new(source_width, query_width, 0.1, "tenth"))
            .build();
        for length in [30, 31] {
            let source = "a".repeat(source_width * length);
            let query = "b".repeat(query_width * length);
            let result = relation_cost(&source, &query, 3, operations.clone());
            if length == 30 {
                assert!(
                    result.is_some_and(|value| (value - 3.0).abs() < 1e-12),
                    "{result:?}"
                );
            } else {
                assert_eq!(result, None);
            }
        }
    }
}

#[test]
fn an_unemitted_query_suffix_never_creates_a_final_state() {
    assert_eq!(
        relation_cost_for_input("a", "ab", "a", 1, OperationSet::standard()),
        None
    );
    assert_eq!(
        relation_cost_for_input("a", "ab", "ab", 1, OperationSet::standard()),
        Some(1.0)
    );
    assert_eq!(
        relation_cost_for_input("", "a", "", 1, OperationSet::standard()),
        None
    );
}

#[test]
fn equivalent_decimal_decompositions_reuse_one_exact_product_identity() {
    let dictionary = DynamicDawgChar::<()>::from_terms(["ab"]);
    let operations = OperationSetBuilder::new()
        .with_operation(OperationType::new(1, 1, 0.1, "tenth"))
        .with_operation(OperationType::new(1, 1, 0.2, "two_tenths"))
        .with_operation(OperationType::new(2, 2, 0.3, "three_tenths"))
        .build();
    let mut wfst = GeneralizedWfst::new(&dictionary, "ab", 1, operations);
    let arcs = wfst.try_transitions(0).expect("root").to_vec();
    let tenth = arcs
        .iter()
        .find(|arc| arc.weight.value() == 0.1)
        .expect("tenth")
        .to;
    let third = arcs
        .iter()
        .find(|arc| arc.weight.value() == 0.3)
        .expect("third")
        .to;
    let via_two = wfst
        .try_transitions(tenth)
        .expect("second scalar")
        .iter()
        .find(|arc| arc.weight.value() == 0.2)
        .expect("two tenths")
        .to;
    let direct = wfst.try_transitions(third).expect("continuation")[0].to;
    assert_eq!(via_two, direct);
    assert!(wfst.is_final(direct));
}

#[test]
fn construction_limits_count_utf8_bytes_and_scalars_independently() {
    use duallity::{
        GeneralizedWfstError, GeneralizedWfstLimits, GeneralizedWfstResource as Resource,
    };
    let dictionary = DynamicDawgChar::<()>::from_terms(["éa"]);
    let exact = GeneralizedWfstLimits {
        max_query_bytes: 3,
        max_query_scalars: 2,
        max_operation_source_scalars: 1,
        max_operation_query_scalars: 1,
        max_retained_dictionary_nodes: 1,
        max_retained_wfst_states: 1,
        ..Default::default()
    };
    assert!(GeneralizedWfst::try_new_with_limits(
        &dictionary,
        "éa",
        1,
        OperationSet::standard(),
        exact
    )
    .is_ok());
    for (limits, expected) in [
        (
            GeneralizedWfstLimits {
                max_query_bytes: 2,
                ..exact
            },
            Resource::QueryBytes,
        ),
        (
            GeneralizedWfstLimits {
                max_query_scalars: 1,
                ..exact
            },
            Resource::QueryScalars,
        ),
        (
            GeneralizedWfstLimits {
                max_operation_source_scalars: 0,
                ..exact
            },
            Resource::OperationSourceScalars,
        ),
        (
            GeneralizedWfstLimits {
                max_operation_query_scalars: 0,
                ..exact
            },
            Resource::OperationQueryScalars,
        ),
        (
            GeneralizedWfstLimits {
                max_retained_dictionary_nodes: 0,
                ..exact
            },
            Resource::RetainedDictionaryNodes,
        ),
        (
            GeneralizedWfstLimits {
                max_retained_wfst_states: 0,
                ..exact
            },
            Resource::RetainedWfstStates,
        ),
    ] {
        assert!(matches!(GeneralizedWfst::try_new_with_limits(
            &dictionary, "éa", 1, OperationSet::standard(), limits),
            Err(GeneralizedWfstError::LimitExceeded { resource, .. }) if resource == expected));
    }
}

#[test]
fn exact_work_boundary_succeeds_and_one_less_fails() {
    use duallity::GeneralizedWfstLimits;
    let dictionary = DynamicDawgChar::<()>::from_terms(["a", "b"]);
    let operations = OperationSetBuilder::new()
        .with_operation(OperationType::new(1, 1, 1.0, "any"))
        .build();
    let mut required = None;
    for work in 0..200 {
        let mut wfst = GeneralizedWfst::try_new_with_limits(
            &dictionary,
            "a",
            1,
            operations.clone(),
            GeneralizedWfstLimits {
                max_work_units_per_expansion: work,
                max_paths_per_expansion: 2,
                max_retained_dictionary_nodes: 3,
                max_retained_wfst_states: 3,
                ..Default::default()
            },
        )
        .expect("construction");
        match wfst.try_transitions(0) {
            Ok(arcs) => {
                assert_eq!(arcs.len(), 2);
                required = Some(work);
                break;
            }
            Err(_) => assert_eq!(wfst.num_states(), 1),
        }
    }
    assert!(required.is_some_and(|work| work > 1));
}

fn consume_label(labels: &[char], position: usize, label: Option<char>) -> Option<usize> {
    match label {
        None => Some(position),
        Some(label) if labels.get(position) == Some(&label) => Some(position + 1),
        Some(_) => None,
    }
}

fn relation_cost(
    dictionary_term: &str,
    query: &str,
    max_distance: u8,
    operations: OperationSet,
) -> Option<f64> {
    relation_cost_for_input(dictionary_term, query, query, max_distance, operations)
}

fn relation_cost_for_input(
    dictionary_term: &str,
    query: &str,
    actual_input: &str,
    max_distance: u8,
    operations: OperationSet,
) -> Option<f64> {
    let dictionary = DynamicDawgChar::<()>::from_terms(vec![dictionary_term]);
    let mut wfst = GeneralizedWfst::try_new(&dictionary, query, max_distance, operations)
        .expect("test operation set must validate");
    let input = actual_input.chars().collect::<Vec<_>>();
    let output = dictionary_term.chars().collect::<Vec<_>>();
    let start = Wfst::start(&wfst);
    let mut queue = VecDeque::from([(start, 0usize, 0usize, 0.0f64)]);
    let mut best_by_configuration = HashMap::from([((start, 0usize, 0usize), 0.0f64)]);
    let mut accepted: Option<f64> = None;

    while let Some((state, input_position, output_position, cost)) = queue.pop_front() {
        if cost
            > best_by_configuration
                .get(&(state, input_position, output_position))
                .copied()
                .unwrap_or(f64::INFINITY)
        {
            continue;
        }

        if input_position == input.len() && output_position == output.len() && wfst.is_final(state)
        {
            let candidate = cost + wfst.final_weight(state).value();
            accepted = Some(accepted.map_or(candidate, |current| current.min(candidate)));
        }

        for transition in wfst
            .try_transitions(state)
            .expect("complete expansion")
            .to_vec()
        {
            let Some(next_input) = consume_label(&input, input_position, transition.input) else {
                continue;
            };
            let Some(next_output) = consume_label(&output, output_position, transition.output)
            else {
                continue;
            };
            let next_cost = cost + transition.weight.value();
            let configuration = (transition.to, next_input, next_output);
            let previous = best_by_configuration
                .get(&configuration)
                .copied()
                .unwrap_or(f64::INFINITY);
            if next_cost < previous {
                best_by_configuration.insert(configuration, next_cost);
                queue.push_back((transition.to, next_input, next_output, next_cost));
            }
        }
    }

    accepted
}

fn binary_words(max_len: usize) -> Vec<String> {
    let mut words = vec![String::new()];
    for len in 1..=max_len {
        for bits in 0..(1usize << len) {
            words.push(
                (0..len)
                    .map(|index| if bits & (1 << index) == 0 { 'a' } else { 'b' })
                    .collect(),
            );
        }
    }
    words
}

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
                && weight
                    .is_none_or(|expected| (transition.weight.value() - expected).abs() <= 1e-9)
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
fn generalized_wfst_honors_isolated_native_applicability_predicates() {
    let any = OperationSetBuilder::new()
        .with_operation(OperationType::new(1, 1, 1.0, "transpose"))
        .build();
    assert_eq!(relation_cost("a", "a", 1, any.clone()), Some(1.0));
    assert_eq!(relation_cost("a", "b", 1, any), Some(1.0));

    let equal = OperationSetBuilder::new()
        .with_operation(OperationType::with_applicability(
            1,
            1,
            1.0,
            OperationApplicability::Equal,
            "positive_equal",
        ))
        .build();
    assert_eq!(relation_cost("é", "é", 1, equal.clone()), Some(1.0));
    assert_eq!(relation_cost("é", "e", 1, equal), None);

    let transpose = OperationSetBuilder::new()
        .with_operation(OperationType::adjacent_transposition(1.0, "renamed"))
        .build();
    assert_eq!(relation_cost("ab", "ba", 1, transpose.clone()), Some(1.0));
    assert_eq!(relation_cost("aa", "aa", 1, transpose.clone()), Some(1.0));
    assert_eq!(relation_cost("ab", "cd", 1, transpose), None);
}

#[test]
fn generalized_wfst_honors_directed_multi_scalar_listed_rules() {
    let mut pairs = SubstitutionSet::new();
    pairs.allow_str("ph", "f");
    let operations = OperationSetBuilder::new()
        .with_operation(OperationType::with_restriction(
            2, 1, 0.25, pairs, "digraph",
        ))
        .build();

    assert_eq!(relation_cost("ph", "f", 1, operations.clone()), Some(0.25));
    assert_eq!(relation_cost("f", "ph", 1, operations), None);
}

#[test]
fn generalized_wfst_acceptance_matches_native_generalized_automaton_exhaustively() {
    let mut listed_pairs = SubstitutionSet::new();
    listed_pairs.allow('a', 'b');
    let operation_sets = [
        OperationSet::standard(),
        OperationSet::with_transposition(),
        OperationSet::with_merge_split(),
        OperationSetBuilder::new()
            .with_operation(OperationType::new(1, 1, 1.0, "any"))
            .with_operation(OperationType::with_applicability(
                1,
                1,
                1.0,
                OperationApplicability::Equal,
                "equal",
            ))
            .build(),
        OperationSetBuilder::new()
            .with_match()
            .with_operation(OperationType::with_restriction(
                1,
                1,
                1.0,
                listed_pairs,
                "a_to_b",
            ))
            .build(),
    ];
    let words = binary_words(3);

    for operations in operation_sets {
        for max_distance in 0..=2 {
            let native = GeneralizedAutomaton::with_operations(max_distance, operations.clone());
            for dictionary_term in &words {
                for query in &words {
                    let native_accepts = native
                        .try_accepts(dictionary_term, query)
                        .expect("fixed operation sets validate");
                    let wfst_accepts =
                        relation_cost(dictionary_term, query, max_distance, operations.clone())
                            .is_some();
                    assert_eq!(
                        wfst_accepts, native_accepts,
                        "set={operations:?}, distance={max_distance}, dictionary={dictionary_term:?}, query={query:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn generalized_wfst_lazy_expansion_materializes_start_state() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help"]);
    let mut wfst = GeneralizedWfst::new(&dict, "helo", 2, OperationSet::standard());

    let start = Wfst::start(&wfst);
    assert!(!wfst.is_expanded(start));

    wfst.expand(start).expect("start state expands");
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

    match wfst.expand_state(Wfst::start(&wfst)) {
        StateExpansion::Expanded { transitions, .. } => assert!(!transitions.is_empty()),
        StateExpansion::Failed(failure) => panic!("state expansion failed: {failure}"),
        StateExpansion::Cancelled(reason) => panic!("state expansion cancelled: {reason:?}"),
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

    wfst.expand(second.to).expect("valid state expands");
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

    wfst.expand(exact.to).expect("exact state expands");

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

    wfst.expand(substitution.to)
        .expect("substitution state expands");

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

    wfst.expand(substitution.to)
        .expect("substitution state expands");

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
            wfst.expand(second.to).expect("valid state expands");
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

    wfst.expand(e.to).expect("valid state expands");
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
fn generalized_wfst_builder_reports_invalid_operations_before_filtering() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
    let operations = OperationSetBuilder::new()
        .with_operation(OperationType::new(0, 0, 2.0, "no_progress"))
        .build();

    let result = GeneralizedWfstBuilder::new(&dict)
        .query("test")
        .max_distance(0)
        .with_operations(operations)
        .build();
    let error = match result {
        Ok(_) => panic!("invalid over-budget operation must not disappear during filtering"),
        Err(error) => error,
    };

    assert!(
        error.contains("consumes neither input"),
        "unexpected error: {error}"
    );
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
    wfst.expand(start).expect("start state expands");

    assert!(!wfst.transitions(start).is_empty());
}

#[test]
fn generalized_wfst_cache_operations_clear_cached_states() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
    let mut wfst = GeneralizedWfst::new(&dict, "test", 1, OperationSet::standard());

    wfst.expand(0).expect("start state expands");
    let before = wfst.computed_states();

    wfst.clear_cache();
    assert_eq!(wfst.computed_states(), 0);
    assert!(before > 0);
}
