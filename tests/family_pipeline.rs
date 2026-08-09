//! In-process end-to-end family pipeline across all three siblings.
//!
//! The full chain, entirely over the shared resource ABI:
//!
//! ```text
//!   libdictenstein DynamicDawgBinding          (producer: vt.dictionary.v1)
//!         │  .resource()
//!         ▼
//!   duallity_wfst_new  ── Levenshtein WFST ──▶  vt.scalar-wfst.1
//!         │  ∘ (lling-llang compose)
//!         ▼  with a case-mapping WFST (term → UPPER(term))
//!   composed WFST  ── traverse ──▶  { UPPER(term) : lev(query,term) ≤ d }
//! ```
//!
//! The composed language is checked three ways for agreement:
//!   1. against a golden TSV (`tests/fixtures/family_pipeline_golden.tsv`);
//!   2. against a liblevenshtein cursor (`ResourceTransducer`) over the same
//!      dictionary resource — the independent consumer of the same producer;
//!   3. under ~5 dictionary mutations performed AFTER capture, proving the
//!      WFST and the cursor are isolated on their captured revision.
//!
//! Two further tests tear the chain down in both orders and assert the
//! retain/release ledger of an instrumented provider balances to zero.
//!
//! Correspondence:
//! - DUAL-FAM-1: composed traversal ≡ golden ≡ liblevenshtein cursor.
//! - DUAL-FAM-2: mid-flight mutations do not change the captured language.
//! - DUAL-FAM-3: the retain/release ledger balances to zero in both teardown orders (no leaked snapshot retain, no double free).

#![cfg(feature = "ffi")]

mod support;

use std::collections::BTreeMap;
use std::ptr;

use duallity::ffi::{
    duallity_resource_release, duallity_wfst_free, duallity_wfst_new, duallity_wfst_resource,
    DuallityStatus, DuallityWfst,
};
use duallity::TropicalWeight;
use libdictenstein::bindings::{BindingUnitDomain, DynamicDawgBinding};
use liblevenshtein::bindings::{MatchBatch, MatchTerm, QueryOrder, ResourceTransducer};
use liblevenshtein::transducer::Algorithm;
use lling_llang::bindings::OwnedWfstResource;
use lling_llang::wfst::{MutableWfst, VectorWfst};
use support::counting_dictionary::CountingDictionary;
use support::wfst_walk::WfstView;
use vinary_tree_interop::VtResource;

const QUERY: &str = "cat";
const MAX_DISTANCE: usize = 1;
const ORIGINAL_TERMS: [&str; 5] = ["cat", "car", "cot", "dog", "cats"];

fn golden() -> BTreeMap<String, f64> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/family_pipeline_golden.tsv"
    );
    let text = std::fs::read_to_string(path).expect("golden fixture is readable");
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (term, distance) = line.split_once('\t').expect("golden row is TAB-separated");
            (
                term.to_owned(),
                distance.parse::<f64>().expect("golden distance"),
            )
        })
        .collect()
}

/// A single-state case-mapping transducer: for every alphabet scalar `c`, a
/// self-loop `c : UPPER(c)` at weight zero, so it uppercases any input string.
fn case_mapping_resource(alphabet: &[char]) -> OwnedWfstResource {
    let mut wfst = VectorWfst::<char, TropicalWeight>::new();
    let state = wfst.add_state();
    wfst.set_start(state);
    wfst.set_final(state, TropicalWeight::new(0.0));
    for &lower in alphabet {
        wfst.add_arc(
            state,
            Some(lower),
            Some(lower.to_ascii_uppercase()),
            state,
            TropicalWeight::new(0.0),
        );
    }
    OwnedWfstResource::from_wfst(wfst)
}

fn alphabet_of(terms: &[&str]) -> Vec<char> {
    let mut chars: Vec<char> = terms.iter().flat_map(|term| term.chars()).collect();
    chars.sort_unstable();
    chars.dedup();
    chars
}

/// Compose a duallity WFST resource with a case-mapper and read the composed
/// language as `UPPER(term) -> edit distance`.
fn composed_language(
    duallity: VtResource,
    case_mapper: &OwnedWfstResource,
) -> BTreeMap<String, f64> {
    let composed = OwnedWfstResource::compose(duallity, case_mapper.as_raw())
        .expect("compose duallity WFST with the case mapper");
    let view = WfstView::new(composed.as_raw());
    view.language(true)
}

/// Fold one filled cursor batch into `ball` as `UPPER(term) -> distance`.
fn collect_matches(ball: &mut BTreeMap<String, f64>, batch: &MatchBatch, written: usize) {
    for item in batch.as_slice().iter().take(written) {
        let MatchTerm::Utf8(term) = &item.term else {
            panic!("unexpected non-UTF-8 term from a UnicodeScalar dictionary");
        };
        ball.insert(term.to_ascii_uppercase(), item.distance as f64);
    }
}

/// The liblevenshtein cursor's ball over `dictionary`, uppercased to match the
/// composed output tape: `UPPER(term) -> distance`.
fn cursor_language(dictionary: VtResource, query: &str, distance: usize) -> BTreeMap<String, f64> {
    let transducer = unsafe { ResourceTransducer::from_resource(dictionary, Algorithm::Standard) }
        .expect("liblevenshtein transducer over the dictionary resource");
    let mut cursor = transducer
        .query_utf8(query, distance, QueryOrder::Traversal)
        .expect("start cursor query");
    let mut ball = BTreeMap::new();
    let mut batch = MatchBatch::default();
    loop {
        let written = cursor.next_batch(&mut batch, 4).expect("cursor batch");
        if written == 0 {
            break;
        }
        for item in batch.as_slice() {
            let MatchTerm::Utf8(term) = &item.term else {
                panic!("unexpected non-UTF-8 term from a UnicodeScalar dictionary");
            };
            ball.insert(term.to_ascii_uppercase(), item.distance as f64);
        }
    }
    ball
}

#[test]
fn composed_language_equals_golden_equals_cursor_under_mutation() {
    let dictionary = DynamicDawgBinding::new(BindingUnitDomain::UnicodeScalar);
    for term in ORIGINAL_TERMS {
        dictionary.insert_text(term.as_bytes(), None).unwrap();
    }

    // Capture: construct the duallity WFST (snapshots the dictionary) and get a
    // scalar-WFST resource.
    let source = dictionary.resource();
    let mut wfst: *mut DuallityWfst = ptr::null_mut();
    assert_eq!(
        duallity_wfst_new(
            source.as_raw(),
            QUERY.as_ptr(),
            QUERY.len(),
            MAX_DISTANCE,
            0,
            0,
            &mut wfst,
        ),
        DuallityStatus::Ok
    );
    let mut duallity_resource = VtResource::NULL;
    assert_eq!(
        unsafe { duallity_wfst_resource(wfst, &mut duallity_resource) },
        DuallityStatus::Ok
    );

    // A liblevenshtein cursor over the same producer. `query_utf8` captures the
    // revision NOW (before the mutations), and one pulled batch locks it in —
    // mirroring liblevenshtein's own `libdictenstein_resource_handoff` test.
    let cursor_source = dictionary.resource();
    let transducer =
        unsafe { ResourceTransducer::from_resource(cursor_source.as_raw(), Algorithm::Standard) }
            .expect("liblevenshtein transducer over the dictionary resource");
    let mut cursor = transducer
        .query_utf8(QUERY, MAX_DISTANCE, QueryOrder::Traversal)
        .expect("start cursor query");
    let mut cursor_ball: BTreeMap<String, f64> = BTreeMap::new();
    let mut batch = MatchBatch::default();
    let first = cursor
        .next_batch(&mut batch, 4)
        .expect("first cursor batch");
    collect_matches(&mut cursor_ball, &batch, first);

    let case_mapper = case_mapping_resource(&alphabet_of(&ORIGINAL_TERMS));

    // ~5 mutations AFTER both captures. If either capture leaked into the live
    // dictionary, the observed language would drift from the golden.
    assert!(dictionary.remove_text(b"car").unwrap());
    assert!(dictionary.remove_text(b"cot").unwrap());
    assert!(dictionary.insert_text(b"cab", None).unwrap());
    assert!(dictionary.insert_text(b"caw", None).unwrap());
    assert!(dictionary.insert_text(b"bat", None).unwrap());

    // Drain the rest of the cursor AFTER the mutations: an isolated cursor still
    // yields its captured (original) revision.
    loop {
        let written = cursor.next_batch(&mut batch, 4).expect("cursor batch");
        if written == 0 {
            break;
        }
        collect_matches(&mut cursor_ball, &batch, written);
    }

    let golden = golden();
    let composed = composed_language(duallity_resource, &case_mapper);

    assert_eq!(
        composed, golden,
        "composed traversal must equal the golden TSV"
    );
    assert_eq!(
        cursor_ball, golden,
        "the liblevenshtein cursor must equal the golden TSV"
    );

    // DUAL-FAM-2: a FRESH cursor over the mutated dictionary reflects the
    // mutations, confirming the mutations were real and the captured views were
    // genuinely isolated (not vacuously equal because nothing changed).
    let live = cursor_language(dictionary.resource().as_raw(), QUERY, MAX_DISTANCE);
    assert_ne!(
        live, golden,
        "the live dictionary must reflect the mutations"
    );
    assert!(live.contains_key("CAB"), "live view sees the inserted term");
    assert!(
        !live.contains_key("CAR"),
        "live view sees the removed term gone"
    );

    duallity_resource_release(duallity_resource);
    unsafe { duallity_wfst_free(wfst) };
    drop(case_mapper);
    drop(cursor_source);
    drop(source);
    drop(dictionary);
}

/// Run the whole chain over an instrumented provider and tear it down, asserting
/// the retain/release ledger returns to zero. `source_first` selects the
/// teardown order.
fn ledger_after_pipeline(source_first: bool) {
    let fixture = CountingDictionary::from_terms(&["cat", "cot", "cats"]);
    let source = fixture.resource();

    let mut wfst: *mut DuallityWfst = ptr::null_mut();
    assert_eq!(
        duallity_wfst_new(
            source,
            QUERY.as_ptr(),
            QUERY.len(),
            MAX_DISTANCE,
            0,
            0,
            &mut wfst
        ),
        DuallityStatus::Ok
    );
    let mut duallity_resource = VtResource::NULL;
    assert_eq!(
        unsafe { duallity_wfst_resource(wfst, &mut duallity_resource) },
        DuallityStatus::Ok
    );
    let case_mapper = case_mapping_resource(&['a', 'c', 'o', 's', 't']);
    // Exercise the composed traversal, then drop the composition retain.
    let observed = composed_language(duallity_resource, &case_mapper);
    assert!(
        observed.contains_key("CAT"),
        "instrumented pipeline still transduces (got {observed:?})"
    );

    // The source snapshot is retained by the WFST chain until every scalar-WFST
    // retain is released, so it must survive an early source release.
    if source_first {
        duallity_resource_release(source);
        duallity_resource_release(duallity_resource);
        unsafe { duallity_wfst_free(wfst) };
    } else {
        duallity_resource_release(duallity_resource);
        unsafe { duallity_wfst_free(wfst) };
        duallity_resource_release(source);
    }
    drop(case_mapper);

    assert_eq!(
        fixture.outstanding_retains(),
        0,
        "retain/release ledger must balance (source_first={source_first}): \
         handed_out={}, retain={}, release={}",
        fixture.handed_out_calls(),
        fixture.retain_calls(),
        fixture.release_calls(),
    );
}

#[test]
fn retain_release_ledger_balances_source_released_first() {
    ledger_after_pipeline(true);
}

#[test]
fn retain_release_ledger_balances_wfst_released_first() {
    ledger_after_pipeline(false);
}
