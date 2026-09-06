//! Consumer paging-acceptance regression suite (family finding F3 / LLEV-B8).
//!
//! `src/bindings.rs::ResourceNode::expanded_edges` pages a foreign provider's
//! `node_edges` reply. F3 recorded that duallity, liblevenshtein, and
//! lling-llang each validated that reply with a slightly different ad-hoc
//! predicate; they are now harmonized to the single predicate proved in
//! `docs/verification/abi/theories/ConsumerAcceptance.v` (llev) —
//!
//! ```text
//! reject unless  written <= capacity
//!            &&  offset   <= total
//!            &&  offset + written <= total          (never past the end)
//!            &&  (written != 0 || offset >= total)  (progress)
//! ```
//!
//! The consumer keeps one fixed page, requires an immutable total across
//! pages, and caps that total by the Unicode scalar alphabet cardinality.
//!
//! These tests drive the REAL consumer: a duallity WFST is constructed over an
//! adversarial `vt.dictionary.v1` provider and then traversed through the
//! exported `vt.scalar-wfst.1` surface. Each misbehavior must surface as a
//! provider error during traversal — and an inflated `total` must never drive
//! an allocation abort. The honest high-degree path must page losslessly across
//! the recommended batch.
//!
//! Correspondence: DUAL-PAGE-1 (adversarial replies rejected as provider
//! errors, no abort) and DUAL-PAGE-2 (honest replies paged losslessly).

#![cfg(feature = "ffi")]

mod support;

use std::ptr;

use duallity::ffi::{
    duallity_resource_release, duallity_wfst_free, duallity_wfst_new, duallity_wfst_resource,
    DuallityStatus, DuallityWfst,
};
use support::counting_dictionary::{CountingDictionary, Misbehavior};
use support::wfst_walk::WfstView;
use vinary_tree_interop::{VtResource, VtStatus, VT_RECOMMENDED_EDGE_BATCH};

#[test]
fn generalized_rejects_late_provider_faults_without_committing_a_partial_language() {
    use duallity::bindings::ResourceDictionary;
    use duallity::{GeneralizedWfst, LazyWfst, Wfst};
    use liblevenshtein::transducer::OperationSet;
    for misbehavior in [
        Misbehavior::LatePageFailure,
        Misbehavior::ChangingTotal,
        Misbehavior::InflatedTotal,
    ] {
        let fixture = CountingDictionary::misbehaving(VT_RECOMMENDED_EDGE_BATCH + 44, misbehavior);
        let raw = fixture.resource();
        let dictionary = unsafe { ResourceDictionary::capture(raw) }.expect("capture");
        duallity_resource_release(raw);
        let mut wfst = GeneralizedWfst::new(&dictionary, "a", 1, OperationSet::standard());
        for _ in 0..2 {
            let result = dictionary.with_checked(|| {
                wfst.try_transitions(0)
                    .map(|arcs| arcs.len())
                    .map_err(|_| VtStatus::ProviderError)
            });
            let expected = match misbehavior {
                Misbehavior::LatePageFailure => VtStatus::IoError,
                Misbehavior::InflatedTotal => VtStatus::LimitExceeded,
                _ => VtStatus::ProviderError,
            };
            assert_eq!(result, Err(expected));
            assert_eq!(wfst.num_states(), 1);
            assert_eq!(wfst.computed_states(), 0);
        }
    }
}

#[test]
fn generalized_work_budget_stops_before_fetching_all_foreign_pages() {
    use duallity::bindings::ResourceDictionary;
    use duallity::{GeneralizedWfst, GeneralizedWfstLimits, Wfst};
    use liblevenshtein::transducer::{OperationSetBuilder, OperationType};
    let fixture = CountingDictionary::high_degree(VT_RECOMMENDED_EDGE_BATCH * 4);
    let raw = fixture.resource();
    let dictionary = unsafe { ResourceDictionary::capture(raw) }.expect("capture");
    duallity_resource_release(raw);
    let operations = OperationSetBuilder::new()
        .with_operation(OperationType::new(1, 1, 1.0, "any"))
        .build();
    let mut wfst = GeneralizedWfst::try_new_with_limits(
        &dictionary,
        "a",
        1,
        operations,
        GeneralizedWfstLimits {
            max_work_units_per_expansion: 10,
            ..Default::default()
        },
    )
    .expect("bounded product");
    let result = dictionary.with_checked(|| {
        wfst.try_transitions(0)
            .map(|arcs| arcs.len())
            .map_err(|_| VtStatus::LimitExceeded)
    });
    assert_eq!(result, Err(VtStatus::LimitExceeded));
    assert_eq!(fixture.edges_calls(), 1);
    assert_eq!(wfst.num_states(), 1);
}

#[test]
fn nested_other_provider_scope_cannot_hide_a_generalized_expansion_fault() {
    use duallity::bindings::ResourceDictionary;
    use duallity::{GeneralizedWfst, LazyWfst, Wfst};
    use liblevenshtein::transducer::OperationSet;
    let fixture = CountingDictionary::misbehaving(
        VT_RECOMMENDED_EDGE_BATCH + 44,
        Misbehavior::LatePageFailure,
    );
    let healthy = CountingDictionary::high_degree(1);
    let raw_a = fixture.resource();
    let raw_b = healthy.resource();
    let a = unsafe { ResourceDictionary::capture(raw_a) }.expect("A");
    let b = unsafe { ResourceDictionary::capture(raw_b) }.expect("B");
    duallity_resource_release(raw_a);
    duallity_resource_release(raw_b);
    let mut wfst = GeneralizedWfst::new(&a, "a", 1, OperationSet::standard());
    let result = a.with_checked(|| {
        b.with_checked(|| {
            wfst.try_transitions(0)
                .map(|arcs| arcs.len())
                .map_err(|_| VtStatus::ProviderError)
        })
    });
    assert_eq!(result, Err(VtStatus::IoError));
    assert_eq!(wfst.num_states(), 1);
    assert_eq!(wfst.computed_states(), 0);
    // The native fallible API itself now owns an observation boundary.
    assert!(wfst.try_transitions(0).is_err());
    assert_eq!(wfst.num_states(), 1);
}

#[test]
fn alphabet_limit_is_reported_by_every_scalar_adapter() {
    for kind in 0..=8 {
        let fixture = CountingDictionary::misbehaving(300, Misbehavior::InflatedTotal);
        let raw = fixture.resource();
        let mut wfst = ptr::null_mut();
        assert_eq!(
            duallity_wfst_new(raw, b"a".as_ptr(), 1, 1, 0, kind, &mut wfst),
            DuallityStatus::Ok
        );
        let mut resource = VtResource::NULL;
        assert_eq!(
            unsafe { duallity_wfst_resource(wfst, &mut resource) },
            DuallityStatus::Ok
        );
        let view = WfstView::new(resource);
        assert_eq!(
            view.first_page_status(view.start()),
            VtStatus::LimitExceeded.to_raw(),
            "kind {kind}"
        );
        assert_eq!(
            fixture.edges_calls(),
            1,
            "inflated total must fail on its first page"
        );
        duallity_resource_release(resource);
        unsafe {
            duallity_wfst_free(wfst);
        }
        duallity_resource_release(raw);
    }
}

/// Standard Levenshtein WFST (kind 0, algorithm 0) at distance 1 over `dict`.
fn new_levenshtein(dict: VtResource) -> *mut DuallityWfst {
    let mut wfst = ptr::null_mut();
    let status = duallity_wfst_new(dict, b"a".as_ptr(), 1, 1, 0, 0, &mut wfst);
    assert_eq!(
        status,
        DuallityStatus::Ok,
        "lazy construction must not touch node_edges"
    );
    assert!(!wfst.is_null());
    wfst
}

#[test]
fn adversarial_pages_surface_as_provider_error_without_abort() {
    for misbehavior in [
        Misbehavior::Overfill,
        Misbehavior::PastEnd,
        Misbehavior::StalledProgress,
        Misbehavior::InflatedTotal,
        Misbehavior::LatePageFailure,
        Misbehavior::ChangingTotal,
    ] {
        // A late error must invalidate earlier successful pages too.
        let fixture = CountingDictionary::misbehaving(VT_RECOMMENDED_EDGE_BATCH + 44, misbehavior);
        let dict = fixture.resource();

        let wfst = new_levenshtein(dict);
        assert_eq!(
            fixture.edges_calls(),
            0,
            "{misbehavior:?}: construction must be O(1) in the dictionary"
        );

        let mut resource = VtResource::NULL;
        assert_eq!(
            unsafe { duallity_wfst_resource(wfst, &mut resource) },
            DuallityStatus::Ok
        );
        let view = WfstView::new(resource);
        let start = view.start();
        let status = view.first_page_status(start);
        assert_ne!(
            status,
            VtStatus::Ok.to_raw(),
            "{misbehavior:?}: misbehaving page must surface as a provider error"
        );
        // The traversal returned rather than aborting: reaching this line at all
        // is the "no allocation abort" evidence for InflatedTotal.

        duallity_resource_release(resource);
        unsafe { duallity_wfst_free(wfst) };
        duallity_resource_release(dict);
    }
}

#[test]
fn honest_high_degree_paging_is_lossless() {
    let degree = VT_RECOMMENDED_EDGE_BATCH * 2 + 17;
    let fixture = CountingDictionary::high_degree(degree);
    let dict = fixture.resource();

    let wfst = new_levenshtein(dict);
    let mut resource = VtResource::NULL;
    assert_eq!(
        unsafe { duallity_wfst_resource(wfst, &mut resource) },
        DuallityStatus::Ok
    );
    let view = WfstView::new(resource);
    let expansion = view
        .expand(view.start())
        .expect("honest paging expands the start state without fault");

    // Every one of the `degree` root edges is a substitution arc from the start
    // state, so a lossless paged expansion recovers them all.
    let substitutions = expansion
        .arcs
        .iter()
        .filter(|arc| arc.has_input == 1 && arc.has_output == 1)
        .count();
    assert_eq!(
        substitutions, degree,
        "every root edge must survive multi-batch paging"
    );
    assert!(
        fixture.edges_calls() >= 2,
        "a degree above the batch must page more than once"
    );
    assert_eq!(
        fixture.max_edge_page_capacity(),
        VT_RECOMMENDED_EDGE_BATCH,
        "the consumer requests exactly the recommended batch per page"
    );

    duallity_resource_release(resource);
    unsafe { duallity_wfst_free(wfst) };
    duallity_resource_release(dict);
}
