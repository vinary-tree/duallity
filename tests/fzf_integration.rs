use duallity::{FzfConfig, FzfScorer, FzfStateSource, FzfWfst};
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use libdictenstein::Dictionary;
use liblevenshtein::transducer::SubsequenceQueryIterator;
use lling_llang::prelude::{ArcticWeight, LazyState, Semiring, StateSource};

fn sorted(mut values: Vec<(String, i32)>) -> Vec<(String, i32)> {
    values.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    values
}

#[test]
fn upstream_path_corpus_agrees_across_scorer_trie_and_state_source() {
    let terms = [
        "/AutomatorDocument.icns",
        "/man1/zshcompctl.1",
        "/.oh-my-zsh/cache",
        "foo/bar/baz",
        "Foo/Bar/Baz",
        "unrelated",
    ];
    let dictionary = DynamicDawgChar::<()>::from_terms(terms);
    let scorer = FzfScorer::with_config(
        "zshc",
        FzfConfig {
            top_k: 3,
            ..FzfConfig::default()
        },
    )
    .expect("fixture query is bounded");

    let mut traversal = SubsequenceQueryIterator::with_pruner(
        dictionary.root(),
        scorer.query_units(),
        scorer.clone(),
    );
    let trie = sorted(
        traversal
            .by_ref()
            .map(|matched| {
                (
                    matched.units.iter().collect(),
                    matched.score.expect("fzf score is present") as i32,
                )
            })
            .collect(),
    );
    let flat = sorted(
        terms
            .iter()
            .filter_map(|term| {
                scorer
                    .score(term)
                    .expect("fixture candidate is bounded")
                    .map(|matched| ((*term).to_owned(), matched.score))
            })
            .collect(),
    );
    assert_eq!(trie, flat);

    let source = FzfStateSource::new(&dictionary, "zshc").expect("fixture query is bounded");
    let LazyState::Computed { transitions, .. } = source.compute_state(source.start()) else {
        panic!("fzf source computes reachable states");
    };
    assert!(!transitions.is_empty());

    let wfst = FzfWfst::new(&dictionary, "zshc").expect("fixture query is bounded");
    assert_eq!(wfst.query(), ['z', 's', 'h', 'c']);
    assert_eq!(ArcticWeight::one().value(), 0.0);
}

#[test]
fn resource_limits_are_enforced_at_public_boundaries() {
    let config = FzfConfig {
        max_query_chars: 2,
        max_candidate_chars: 3,
        ..FzfConfig::default()
    };
    assert!(FzfScorer::with_config("long", config).is_err());
    let scorer = FzfScorer::with_config("ok", config).expect("two characters fit");
    assert!(scorer.score("toolong").is_err());
}
