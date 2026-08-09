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
//! with the claimed-total bound realized *structurally* (the consumer never
//! sizes an allocation from the provider-reported `total`).
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
    ] {
        // Degree above the recommended batch so InflatedTotal survives the first
        // honest-looking page and can only be caught by the progress conjunct on
        // a later page — proving the total bound is realized structurally.
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
