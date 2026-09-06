//! Semantics of the constructed WFST, checked through the exported ABI.
//!
//! A duallity WFST is a transducer whose accepting paths spell dictionary terms
//! on the output tape with path weight equal to the edit distance. This suite
//! constructs the WFST through `duallity_wfst_new`, walks the exported
//! `vt.scalar-wfst.1` surface, and compares the transduced language against
//! liblevenshtein's OWN distance functions (a direct dependency, so this oracle
//! is placement-clean and is the same oracle liblevenshtein cross-validates its
//! automaton against).
//!
//! The parameterized Levenshtein kind (0), Universal kinds (1..=3), and
//! Generalized kinds (4..=6) are exact: their transduced language equals the
//! oracle ball for every corresponding algorithm.
//!
//! Correspondence:
//! - DUAL-WFST-LANG-1: accepted language = { t in dict : lev(query,t) <= d } for parameterized kind 0 and generalized kinds 4..=6.
//! - DUAL-WFST-LANG-2: the accepting-path weight of t equals lev(query,t).
//! - DUAL-FZF-1: the Fzf kind exports the Arctic (max-plus) semiring and its accepting language is the subsequence set.
//! - DUAL-UNIV-1: Universal finality consumes the complete fixed query before
//!   accepting an output term, and the automaton state depends only on the
//!   dictionary input processed by the universal transition algebra.

#![cfg(feature = "ffi")]

mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::ptr;

use duallity::ffi::{
    duallity_resource_release, duallity_wfst_free, duallity_wfst_new, duallity_wfst_resource,
    DuallityStatus, DuallityWfst,
};
use libdictenstein::bindings::{BindingUnitDomain, DynamicDawgBinding};
use liblevenshtein::distance::{
    create_memo_cache, merge_and_split_distance, standard_distance, transposition_distance,
};
use proptest::prelude::*;
use support::wfst_walk::WfstView;
use vinary_tree_interop::{VtResource, VtWeightDomain};

// Kind / algorithm encodings mirrored from src/ffi.rs and src/bindings.rs.
const KIND_LEVENSHTEIN: u32 = 0;
const KIND_UNIVERSAL_STANDARD: u32 = 1;
const KIND_UNIVERSAL_TRANSPOSITION: u32 = 2;
const KIND_UNIVERSAL_MERGE_AND_SPLIT: u32 = 3;
const KIND_GENERALIZED_STANDARD: u32 = 4;
const KIND_GENERALIZED_TRANSPOSITION: u32 = 5;
const KIND_GENERALIZED_MERGE_AND_SPLIT: u32 = 6;
const KIND_FZF: u32 = 8;

const ALGORITHM_STANDARD: u32 = 0;
const ALGORITHM_TRANSPOSITION: u32 = 1;
const ALGORITHM_MERGE_AND_SPLIT: u32 = 2;

/// Build a UnicodeScalar dictionary over `terms` and keep it alive for the test.
fn dictionary(terms: &BTreeSet<String>) -> DynamicDawgBinding {
    let dictionary = DynamicDawgBinding::new(BindingUnitDomain::UnicodeScalar);
    for term in terms {
        dictionary.insert_text(term.as_bytes(), None).unwrap();
    }
    dictionary
}

/// Transduced language of a constructed WFST: `output term -> best weight`.
/// `minimize` selects tropical (min, = distance) or arctic (max, = score).
fn wfst_language(
    source: VtResource,
    query: &str,
    distance: usize,
    algorithm: u32,
    kind: u32,
    minimize: bool,
) -> BTreeMap<String, f64> {
    let mut wfst: *mut DuallityWfst = ptr::null_mut();
    let status = duallity_wfst_new(
        source,
        query.as_ptr(),
        query.len(),
        distance,
        algorithm,
        kind,
        &mut wfst,
    );
    assert_eq!(status, DuallityStatus::Ok, "constructing kind {kind}");
    let mut resource = VtResource::NULL;
    assert_eq!(
        unsafe { duallity_wfst_resource(wfst, &mut resource) },
        DuallityStatus::Ok
    );
    let language = {
        let view = WfstView::new(resource);
        view.language(minimize)
    };
    duallity_resource_release(resource);
    unsafe { duallity_wfst_free(wfst) };
    language
}

/// The oracle Levenshtein ball, straight from liblevenshtein's distance
/// functions: `{ t : dist(query, t) <= d } -> dist(query, t)`.
fn edit_ball(
    terms: &BTreeSet<String>,
    query: &str,
    distance: usize,
    algorithm: u32,
) -> BTreeMap<String, f64> {
    let cache = create_memo_cache();
    terms
        .iter()
        .filter_map(|term| {
            let dist = match algorithm {
                ALGORITHM_STANDARD => standard_distance(query, term),
                ALGORITHM_TRANSPOSITION => transposition_distance(query, term),
                ALGORITHM_MERGE_AND_SPLIT => merge_and_split_distance(query, term, &cache),
                other => unreachable!("unexpected algorithm {other}"),
            };
            (dist <= distance).then(|| (term.clone(), dist as f64))
        })
        .collect()
}

fn assert_universal_language(
    terms: &BTreeSet<String>,
    query: &str,
    distance: usize,
    algorithm: u32,
    kind: u32,
) -> Result<(), TestCaseError> {
    let handle = dictionary(terms);
    let observed = wfst_language(
        handle.resource().as_raw(),
        query,
        distance,
        algorithm,
        kind,
        true,
    );
    let expected = edit_ball(terms, query, distance, algorithm);
    prop_assert_eq!(&observed, &expected, "universal kind {}", kind);
    Ok(())
}

prop_compose! {
    fn scenario()(
        terms in prop::collection::btree_set("[a-c]{1,4}", 1..6),
        query in "[a-c]{0,4}",
        distance in 0usize..=2,
    ) -> (BTreeSet<String>, String, usize) {
        (terms, query, distance)
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// DUAL-WFST-LANG-1 + DUAL-WFST-LANG-2 for the parameterized Levenshtein
    /// kind across all three alignment-expressible algorithms. This is the
    /// direct distance-function oracle check; liblevenshtein cross-validates the
    /// same automaton against the same functions.
    #[test]
    fn levenshtein_language_is_the_edit_ball((terms, query, distance) in scenario()) {
        let handle = dictionary(&terms);
        let source = handle.resource();
        for algorithm in [ALGORITHM_STANDARD, ALGORITHM_TRANSPOSITION, ALGORITHM_MERGE_AND_SPLIT] {
            let observed =
                wfst_language(source.as_raw(), &query, distance, algorithm, KIND_LEVENSHTEIN, true);
            let expected = edit_ball(&terms, &query, distance, algorithm);
            prop_assert_eq!(&observed, &expected, "algorithm={}", algorithm);
        }
    }

    /// DUAL-WFST-LANG-1/2 for the Generalized kinds: each realizes exactly the
    /// oracle ball for its algorithm, independently of the parameterized
    /// construction.
    #[test]
    fn generalized_language_is_the_edit_ball((terms, query, distance) in scenario()) {
        let handle = dictionary(&terms);
        let source = handle.resource();
        let variants = [
            (ALGORITHM_STANDARD, KIND_GENERALIZED_STANDARD),
            (ALGORITHM_TRANSPOSITION, KIND_GENERALIZED_TRANSPOSITION),
            (ALGORITHM_MERGE_AND_SPLIT, KIND_GENERALIZED_MERGE_AND_SPLIT),
        ];
        for (algorithm, generalized) in variants {
            let observed =
                wfst_language(source.as_raw(), &query, distance, algorithm, generalized, true);
            let expected = edit_ball(&terms, &query, distance, algorithm);
            prop_assert_eq!(&observed, &expected, "generalized kind {}", generalized);
        }
    }

    /// DUAL-UNIV-1 for the Standard universal position algebra.
    #[test]
    fn universal_standard_language_is_the_edit_ball((terms, query, distance) in scenario()) {
        assert_universal_language(
            &terms,
            &query,
            distance,
            ALGORITHM_STANDARD,
            KIND_UNIVERSAL_STANDARD,
        )?;
    }

    /// DUAL-UNIV-1 for the adjacent-transposition universal position algebra.
    #[test]
    fn universal_transposition_language_is_the_edit_ball((terms, query, distance) in scenario()) {
        assert_universal_language(
            &terms,
            &query,
            distance,
            ALGORITHM_TRANSPOSITION,
            KIND_UNIVERSAL_TRANSPOSITION,
        )?;
    }

    /// DUAL-UNIV-1 for the merge-and-split universal position algebra.
    #[test]
    fn universal_merge_and_split_language_is_the_edit_ball((terms, query, distance) in scenario()) {
        assert_universal_language(
            &terms,
            &query,
            distance,
            ALGORITHM_MERGE_AND_SPLIT,
            KIND_UNIVERSAL_MERGE_AND_SPLIT,
        )?;
    }
}

#[test]
fn standard_wfst_matches_known_edits() {
    // A hand-checked anchor so a proptest generator regression cannot silently
    // pass a vacuous language.
    let terms: BTreeSet<String> = ["cat", "cot", "cats", "dog"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    let handle = dictionary(&terms);
    let observed = wfst_language(
        handle.resource().as_raw(),
        "cat",
        1,
        ALGORITHM_STANDARD,
        KIND_LEVENSHTEIN,
        true,
    );
    let expected = BTreeMap::from([
        ("cat".to_owned(), 0.0),  // exact
        ("cot".to_owned(), 1.0),  // one substitution
        ("cats".to_owned(), 1.0), // one insertion
    ]);
    assert_eq!(observed, expected);
}

fn binary_words(maximum_len: usize) -> BTreeSet<String> {
    let mut words = BTreeSet::from([String::new()]);
    let mut frontier = vec![String::new()];
    for _ in 0..maximum_len {
        let mut next = Vec::with_capacity(frontier.len().saturating_mul(2));
        for prefix in frontier {
            for unit in ['a', 'b'] {
                let mut word = prefix.clone();
                word.push(unit);
                words.insert(word.clone());
                next.push(word);
            }
        }
        frontier = next;
    }
    words
}

#[test]
fn every_small_universal_abi_language_and_weight_matches_dynamic_programming() {
    let terms = binary_words(3);
    let handle = dictionary(&terms);
    let source = handle.resource();
    let variants = [
        (ALGORITHM_STANDARD, KIND_UNIVERSAL_STANDARD),
        (ALGORITHM_TRANSPOSITION, KIND_UNIVERSAL_TRANSPOSITION),
        (ALGORITHM_MERGE_AND_SPLIT, KIND_UNIVERSAL_MERGE_AND_SPLIT),
    ];

    for query in &terms {
        for distance in 0..=2 {
            for (algorithm, kind) in variants {
                let observed =
                    wfst_language(source.as_raw(), query, distance, algorithm, kind, true);
                let expected = edit_ball(&terms, query, distance, algorithm);
                assert_eq!(
                    observed, expected,
                    "query={query:?} distance={distance} kind={kind}"
                );
            }
        }
    }
}

#[test]
fn universal_padding_is_disjoint_from_every_unicode_scalar() {
    let terms: BTreeSet<String> = ["\0", "$", "$a", "a", "é"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    let handle = dictionary(&terms);
    let source = handle.resource();
    let variants = [
        (ALGORITHM_STANDARD, KIND_UNIVERSAL_STANDARD),
        (ALGORITHM_TRANSPOSITION, KIND_UNIVERSAL_TRANSPOSITION),
        (ALGORITHM_MERGE_AND_SPLIT, KIND_UNIVERSAL_MERGE_AND_SPLIT),
    ];

    for query in ["\0", "$", "a", "é"] {
        for distance in 0..=2 {
            for (algorithm, kind) in variants {
                let observed =
                    wfst_language(source.as_raw(), query, distance, algorithm, kind, true);
                let expected = edit_ball(&terms, query, distance, algorithm);
                assert_eq!(
                    observed, expected,
                    "query={query:?} distance={distance} kind={kind}"
                );
            }
        }
    }
}

#[test]
fn transposition_accepts_adjacent_swap_that_standard_rejects() {
    let terms: BTreeSet<String> = ["ab"].into_iter().map(str::to_owned).collect();
    let handle = dictionary(&terms);
    let source = handle.resource();
    // "ba" -> "ab" is one transposition, but two substitutions under standard.
    let transposition = wfst_language(
        source.as_raw(),
        "ba",
        1,
        ALGORITHM_TRANSPOSITION,
        KIND_LEVENSHTEIN,
        true,
    );
    assert_eq!(transposition, BTreeMap::from([("ab".to_owned(), 1.0)]));
    let standard = wfst_language(
        source.as_raw(),
        "ba",
        1,
        ALGORITHM_STANDARD,
        KIND_LEVENSHTEIN,
        true,
    );
    assert!(standard.is_empty(), "standard rejects a swap at distance 1");
}

#[test]
fn merge_and_split_accepts_merge_that_standard_rejects() {
    let terms: BTreeSet<String> = ["m"].into_iter().map(str::to_owned).collect();
    let handle = dictionary(&terms);
    // "rn" -> "m" is one merge under merge-and-split.
    let merged = wfst_language(
        handle.resource().as_raw(),
        "rn",
        1,
        ALGORITHM_MERGE_AND_SPLIT,
        KIND_LEVENSHTEIN,
        true,
    );
    assert_eq!(merged, BTreeMap::from([("m".to_owned(), 1.0)]));
}

#[test]
fn fzf_kind_exports_arctic_semiring_and_subsequence_language() {
    // DUAL-FZF-1: the Fzf adapter scores in the Arctic (max-plus) semiring and
    // accepts exactly the dictionary terms that contain the query as an ordered
    // subsequence.
    let terms: BTreeSet<String> = ["zshcompctl", "cache", "xyz"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    let handle = dictionary(&terms);
    let source = handle.resource();

    let mut wfst: *mut DuallityWfst = ptr::null_mut();
    assert_eq!(
        duallity_wfst_new(
            source.as_raw(),
            b"zsh".as_ptr(),
            3,
            0,
            0,
            KIND_FZF,
            &mut wfst
        ),
        DuallityStatus::Ok
    );
    let mut resource = VtResource::NULL;
    assert_eq!(
        unsafe { duallity_wfst_resource(wfst, &mut resource) },
        DuallityStatus::Ok
    );
    let (domain, language) = {
        let view = WfstView::new(resource);
        (view.weight_domain(), view.language(false))
    };
    duallity_resource_release(resource);
    unsafe { duallity_wfst_free(wfst) };

    assert_eq!(
        domain,
        VtWeightDomain::ArcticF64,
        "fzf pins the Arctic semiring"
    );
    let accepted: BTreeSet<String> = language.keys().cloned().collect();
    assert_eq!(
        accepted,
        BTreeSet::from(["zshcompctl".to_owned()]),
        "only the subsequence match is accepted"
    );
    let score = language["zshcompctl"];
    assert!(score.is_finite(), "an accepted fzf score is a real weight");
}

/// DUAL-UNIV-1 regression: a shorter dictionary term cannot become final
/// while one or more fixed-query labels remain unconsumed.
#[test]
fn universal_kind_rejects_shorter_term_outside_zero_ball() {
    let terms: BTreeSet<String> = ["a", "aa"].into_iter().map(str::to_owned).collect();
    let handle = dictionary(&terms);
    let source = handle.resource();

    // "a" is at standard distance 1 from query "aa", so it is outside the d=0
    // ball; the exact parameterized kind agrees.
    assert_eq!(standard_distance("aa", "a"), 1);
    let exact = wfst_language(
        source.as_raw(),
        "aa",
        0,
        ALGORITHM_STANDARD,
        KIND_LEVENSHTEIN,
        true,
    );
    assert_eq!(
        exact,
        BTreeMap::from([("aa".to_owned(), 0.0)]),
        "parameterized kind 0 accepts exactly the d=0 ball"
    );

    let universal = wfst_language(
        source.as_raw(),
        "aa",
        0,
        ALGORITHM_STANDARD,
        KIND_UNIVERSAL_STANDARD,
        true,
    );
    assert_eq!(
        universal,
        BTreeMap::from([("aa".to_owned(), 0.0)]),
        "universal kind 1 must accept exactly the d=0 ball"
    );
}
