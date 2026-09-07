use duallity::{
    FzfWfst, GeneralizedWfst, LazyWfst, LevenshteinWfst, UniversalLevenshteinWfst, WallBreakerWfst,
    Wfst,
};
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use libdictenstein::scdawg::Scdawg;
use liblevenshtein::transducer::universal::{MergeAndSplit, Standard, Transposition};
use liblevenshtein::transducer::{Algorithm, OperationSet};
use lling_llang::prelude::{Semiring, StateId, TropicalWeight, WeightedTransition};
use lling_llang::wfst::CachePolicy;
use std::collections::{HashSet, VecDeque};

#[derive(Debug, PartialEq)]
struct StateSnapshot<S: Semiring> {
    is_final: bool,
    final_weight: S,
    arcs: Vec<WeightedTransition<char, S>>,
}

fn read_state<T, S>(wfst: &mut T, state: StateId) -> StateSnapshot<S>
where
    T: LazyWfst<char, S>,
    S: Semiring,
{
    assert!(wfst.is_valid_state(state));
    wfst.expand(state).expect("reachable state expands");
    StateSnapshot {
        is_final: wfst.is_final(state),
        final_weight: wfst.final_weight(state),
        arcs: wfst.transitions(state).to_vec(),
    }
}

fn three_reachable_states<T, S>(wfst: &mut T) -> Vec<(StateId, StateSnapshot<S>)>
where
    T: LazyWfst<char, S>,
    S: Semiring,
{
    let start = wfst.start();
    let mut seen = HashSet::from([start]);
    let mut frontier = VecDeque::from([start]);
    let mut states = Vec::with_capacity(3);
    while states.len() < 3 {
        let state = frontier.pop_front().expect("fixture has three states");
        let snapshot = read_state(wfst, state);
        for arc in &snapshot.arcs {
            if seen.insert(arc.to) {
                frontier.push_back(arc.to);
            }
        }
        states.push((state, snapshot));
    }
    states
}

fn assert_zero_lru_uses_fallback<T>(mut wfst: T, set_bound: impl Fn(&mut T, usize))
where
    T: LazyWfst<char, TropicalWeight>,
{
    let states = three_reachable_states(&mut wfst);
    set_bound(&mut wfst, 2);
    wfst.set_cache_policy(CachePolicy::Lru { max_states: 0 });
    assert_eq!(
        wfst.computed_states(),
        0,
        "policy replacement clears residency"
    );
    for (index, (state, expected)) in states.iter().enumerate() {
        assert_eq!(&read_state(&mut wfst, *state), expected);
        assert_eq!(wfst.computed_states(), (index + 1).min(2));
    }
    assert!(!wfst.is_expanded(states[0].0), "oldest state was evicted");
    assert!(wfst.is_expanded(states[1].0));
    assert!(wfst.is_expanded(states[2].0));

    // A hit changes recency; shrinking retains that most recently used state.
    assert_eq!(read_state(&mut wfst, states[1].0), states[1].1);
    set_bound(&mut wfst, 1);
    assert_eq!(wfst.computed_states(), 1);
    assert!(wfst.is_expanded(states[1].0));
    assert!(!wfst.is_expanded(states[2].0));

    // A zero configured fallback is clamped to one, never NoCache.
    set_bound(&mut wfst, 0);
    assert_eq!(read_state(&mut wfst, states[0].0), states[0].1);
    assert_eq!(wfst.computed_states(), 1);
    assert!(wfst.is_expanded(states[0].0));
    assert!(!wfst.is_expanded(states[1].0));
    assert_eq!(wfst.cache_policy(), CachePolicy::Lru { max_states: 0 });
    wfst.clear_cache();
    assert_eq!(wfst.computed_states(), 0);
    assert_eq!(read_state(&mut wfst, states[0].0), states[0].1);
    assert_eq!(wfst.computed_states(), 1);
}

#[test]
fn classic_lru_zero_uses_tunable_default_bound_for_every_edit_algorithm() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["abc", "abd", "cab"]);
    for algorithm in [
        Algorithm::Standard,
        Algorithm::Transposition,
        Algorithm::MergeAndSplit,
    ] {
        assert_zero_lru_uses_fallback(
            LevenshteinWfst::with_algorithm(&dict, "abc", 1, algorithm),
            LevenshteinWfst::set_max_cache_size,
        );
    }
}

#[test]
fn universal_standard_lru_zero_uses_tunable_default_bound() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["abc", "abd", "cab"]);
    assert_zero_lru_uses_fallback(
        UniversalLevenshteinWfst::<Standard, _>::new(&dict, "abc", 1),
        UniversalLevenshteinWfst::set_max_cache_size,
    );
}

#[test]
fn universal_transposition_lru_zero_uses_tunable_default_bound() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["abc", "abd", "cab"]);
    assert_zero_lru_uses_fallback(
        UniversalLevenshteinWfst::<Transposition, _>::new(&dict, "abc", 1),
        UniversalLevenshteinWfst::set_max_cache_size,
    );
}

#[test]
fn universal_merge_split_lru_zero_uses_tunable_default_bound() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["abc", "abd", "cab"]);
    assert_zero_lru_uses_fallback(
        UniversalLevenshteinWfst::<MergeAndSplit, _>::new(&dict, "abc", 1),
        UniversalLevenshteinWfst::set_max_cache_size,
    );
}

#[test]
fn generalized_lru_zero_uses_tunable_default_bound() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["abc", "abd", "cab"]);
    assert_zero_lru_uses_fallback(
        GeneralizedWfst::new(&dict, "abc", 1, OperationSet::standard()),
        GeneralizedWfst::set_max_cache_size,
    );
}

#[test]
fn fzf_lru_zero_keeps_only_the_current_transient_expansion() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["abc", "abd", "cab"]);
    let mut wfst = FzfWfst::new(&dict, "ab").expect("valid fzf query");
    let states = three_reachable_states(&mut wfst);
    wfst.set_cache_policy(CachePolicy::Lru { max_states: 0 });
    assert!(states.iter().all(|(state, _)| !wfst.is_expanded(*state)));
    // Unlike the other native wrappers, this count is cumulative computations,
    // not residency. Distinct accesses recompute; the same transient is reused.
    let mut computations = wfst.computed_states();
    for index in [0, 1, 2, 1, 0] {
        let (state, expected) = &states[index];
        assert_eq!(&read_state(&mut wfst, *state), expected);
        computations += 1;
        assert_eq!(wfst.computed_states(), computations);
        for (candidate, _) in &states {
            assert_eq!(wfst.is_expanded(*candidate), candidate == state);
        }
        assert_eq!(&read_state(&mut wfst, *state), expected);
        assert_eq!(
            wfst.computed_states(),
            computations,
            "same transient reused"
        );
    }
    wfst.clear_cache();
    assert!(states.iter().all(|(state, _)| !wfst.is_expanded(*state)));
    assert_eq!(
        wfst.computed_states(),
        computations,
        "clear retains lifetime count"
    );
    assert_eq!(read_state(&mut wfst, states[0].0), states[0].1);
    assert_eq!(wfst.computed_states(), computations + 1);
}

#[test]
fn wallbreaker_lru_zero_uses_tunable_default_bound() {
    let dict = Scdawg::<()>::from_terms(vec!["ab"]);
    let mut wfst = WallBreakerWfst::new(&dict, "ab", 0);
    wfst.set_max_cache_size(2);
    wfst.set_cache_policy(CachePolicy::Lru { max_states: 0 });

    let start = Wfst::start(&wfst);
    let next = wfst
        .transitions_lazy(start)
        .first()
        .expect("expected WallBreaker start transition")
        .to;
    wfst.expand(next).expect("valid state expands");

    assert_eq!(wfst.computed_states(), 2);

    wfst.set_max_cache_size(1);

    assert_eq!(wfst.computed_states(), 1);
}
