use duallity::{
    LazyState, StateSource, TropicalWeight, WallBreakerWfst, WallBreakerWfstBuilder, Wfst,
};
use libdictenstein::scdawg::Scdawg;
use liblevenshtein::transducer::Algorithm;
use lling_llang::prelude::LazyWfst;
use lling_llang::wfst::CachePolicy;

fn pending_eager_state<T>(message: &str) -> T {
    std::panic::panic_any(message.to_owned())
}

#[test]
fn wallbreaker_wfst_creation() {
    let dict = Scdawg::<()>::from_terms(vec!["hello", "help", "world"]);
    let wfst = WallBreakerWfst::new(&dict, "helo", 2);

    assert!(!wfst.is_empty());
    assert_eq!(wfst.query(), "helo");
    assert_eq!(wfst.max_distance(), 2);
}

#[test]
fn wallbreaker_wfst_start_state() {
    let dict = Scdawg::<()>::from_terms(vec!["test"]);
    let wfst = WallBreakerWfst::new(&dict, "tset", 2);

    let start = Wfst::start(&wfst);
    assert!(wfst.is_valid_state(start));
}

#[test]
fn wallbreaker_wfst_lazy_expansion() {
    let dict = Scdawg::<()>::from_terms(vec!["hello", "help"]);
    let mut wfst = WallBreakerWfst::new(&dict, "helo", 2);

    let start = Wfst::start(&wfst);
    assert!(!wfst.is_expanded(start));

    wfst.expand(start);
    assert!(wfst.is_expanded(start));
}

#[test]
fn wallbreaker_wfst_reports_registered_final_state_before_expansion() {
    let dict = Scdawg::<()>::from_terms(vec!["a"]);
    let mut wfst = WallBreakerWfst::new(&dict, "a", 0);

    let start = Wfst::start(&wfst);
    let terminal = wfst
        .transitions_lazy(start)
        .iter()
        .find(|transition| transition.input == Some('a') && transition.output == Some('a'))
        .map(|transition| transition.to)
        .expect("exact result transition should exist");
    let computed_after_start = wfst.computed_states();

    assert!(wfst.is_valid_state(terminal));
    assert!(!wfst.is_expanded(terminal));
    assert!(wfst.is_final(terminal));
    assert_eq!(wfst.final_weight(terminal), TropicalWeight::new(0.0));
    assert!(!wfst.is_expanded(terminal));
    assert_eq!(wfst.computed_states(), computed_after_start);
}

#[test]
fn wallbreaker_statesource_computes_start_state() {
    let dict = Scdawg::<()>::from_terms(vec!["hello", "help"]);
    let wfst = WallBreakerWfst::new(&dict, "helo", 2);

    match StateSource::<char, TropicalWeight>::compute_state(&wfst, Wfst::start(&wfst)) {
        LazyState::Computed { transitions, .. } => {
            assert!(!transitions.is_empty());
            assert!(transitions
                .iter()
                .all(|transition| wfst.is_valid_state(transition.to)));
        }
        LazyState::Pending => pending_eager_state("WallBreaker StateSource should compute eagerly"),
    }
}

#[test]
fn wallbreaker_wfst_transitions_after_expansion() {
    let dict = Scdawg::<()>::from_terms(vec!["hello", "help"]);
    let mut wfst = WallBreakerWfst::new(&dict, "helo", 2);

    let start = Wfst::start(&wfst);
    wfst.expand(start);

    assert!(!wfst.transitions(start).is_empty() || wfst.num_results() == 0);
}

#[test]
fn wallbreaker_wfst_with_transposition() {
    let dict = Scdawg::<()>::from_terms(vec!["test", "tset"]);
    let wfst = WallBreakerWfst::with_algorithm(&dict, "tset", 1, Algorithm::Transposition);

    assert!(matches!(wfst.algorithm(), Algorithm::Transposition));
}

#[test]
fn wallbreaker_wfst_builder() {
    let dict = Scdawg::<()>::from_terms(vec!["hello", "world"]);
    let result = WallBreakerWfstBuilder::new(&dict)
        .query("helo")
        .max_distance(2)
        .standard()
        .build();

    assert!(result.is_ok());
    let wfst = result.unwrap();
    assert_eq!(wfst.query(), "helo");
}

#[test]
fn wallbreaker_wfst_builder_requires_query() {
    let dict = Scdawg::<()>::from_terms(vec!["test"]);
    let result = WallBreakerWfstBuilder::new(&dict).build();

    assert!(result.is_err());
}

#[test]
fn wallbreaker_wfst_builder_sets_transposition_algorithm() {
    let dict = Scdawg::<()>::from_terms(vec!["test"]);
    let result = WallBreakerWfstBuilder::new(&dict)
        .query("tset")
        .transposition()
        .build();

    assert!(matches!(
        result.expect("test fixture: build must be Ok").algorithm(),
        Algorithm::Transposition
    ));
}

#[test]
fn wallbreaker_wfst_builder_sets_merge_and_split_algorithm() {
    let dict = Scdawg::<()>::from_terms(vec!["test"]);
    let result = WallBreakerWfstBuilder::new(&dict)
        .query("test")
        .merge_and_split()
        .build();

    assert!(matches!(
        result.expect("test fixture: build must be Ok").algorithm(),
        Algorithm::MergeAndSplit
    ));
}

#[test]
fn wallbreaker_wfst_num_results() {
    let dict = Scdawg::<()>::from_terms(vec!["hello", "help", "world"]);
    let wfst = WallBreakerWfst::new(&dict, "helo", 2);

    assert!(wfst.num_results() > 0);
}

#[test]
fn wallbreaker_wfst_cache_operations() {
    let dict = Scdawg::<()>::from_terms(vec!["test"]);
    let mut wfst = WallBreakerWfst::new(&dict, "test", 1);

    wfst.expand(0);
    let before = wfst.computed_states();

    wfst.clear_cache();
    assert_eq!(wfst.computed_states(), 0);
    assert!(before > 0);
}

#[test]
fn wallbreaker_wfst_no_cache_policy_uses_scratch_only() {
    let dict = Scdawg::<()>::from_terms(vec!["hello", "help"]);
    let mut wfst = WallBreakerWfst::new(&dict, "helo", 2);
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
fn wallbreaker_wfst_lru_policy_evicts_least_recently_used_state() {
    let dict = Scdawg::<()>::from_terms(vec!["hello", "help"]);
    let mut wfst = WallBreakerWfst::new(&dict, "helo", 2);
    wfst.set_cache_policy(CachePolicy::Lru { max_states: 1 });

    let start = Wfst::start(&wfst);
    let next = wfst
        .transitions_lazy(start)
        .first()
        .expect("expected start transition")
        .to;

    assert!(wfst.is_expanded(start));
    assert_eq!(wfst.computed_states(), 1);

    wfst.expand(next);

    assert_eq!(wfst.computed_states(), 1);
    assert!(!wfst.is_expanded(start));
    assert!(wfst.is_expanded(next));
}

#[test]
fn wallbreaker_wfst_state_hint() {
    let dict = Scdawg::<()>::from_terms(vec!["hello", "world"]);
    let wfst = WallBreakerWfst::new(&dict, "helo", 2);

    let hint = StateSource::num_states_hint(&wfst);
    assert!(hint.is_some());
    assert!(hint.unwrap() > 0);
}

#[test]
fn wallbreaker_wfst_empty_results() {
    let dict = Scdawg::<()>::from_terms(vec!["hello", "world"]);
    let mut wfst = WallBreakerWfst::new(&dict, "zzzzz", 1);

    let start = Wfst::start(&wfst);

    assert_eq!(wfst.num_results(), 0);
    assert_eq!(wfst.num_states(), 1);
    assert!(!wfst.is_empty());
    assert!(wfst.is_valid_state(start));
    assert!(!wfst.is_final(start));
    assert!(wfst.transitions_lazy(start).is_empty());
}

#[test]
fn wallbreaker_wfst_exact_match() {
    let dict = Scdawg::<()>::from_terms(vec!["hello", "world"]);
    let wfst = WallBreakerWfst::new(&dict, "hello", 0);

    assert_eq!(wfst.num_results(), 1);
}

#[test]
fn wallbreaker_path_distance_counted_once() {
    let dict = Scdawg::<()>::from_terms(vec!["hello"]);
    let mut wfst = WallBreakerWfst::new(&dict, "helo", 2);

    assert!(wfst.num_results() > 0);

    let mut current = Wfst::start(&wfst);
    let mut transition_cost = 0.0;

    for _ in 0..10 {
        wfst.expand(current);
        if wfst.is_final(current) {
            break;
        }

        let (from, to, weight) = {
            let transition = wfst
                .transitions(current)
                .first()
                .expect("expected a transition along the result term");
            (transition.from, transition.to, transition.weight.value())
        };

        assert_eq!(from, current);
        transition_cost += weight;
        current = to;
    }

    wfst.expand(current);
    assert!(wfst.is_final(current));
    assert_eq!(transition_cost, 0.0);
    assert!(wfst.final_weight(current).value() > 0.0);
}
