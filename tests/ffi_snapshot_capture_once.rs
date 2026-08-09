//! The dictionary snapshot is captured exactly once, at `duallity_wfst_new`,
//! and never re-captured during traversal.
//!
//! This is the executable correspondence for the coordinator's
//! `SnapshotCaptureOnce.tla` model. It uses an instrumented `vt.dictionary.v1`
//! provider (`support::counting_dictionary`) that counts every callback shared
//! across the source resource and every snapshot it hands out.
//!
//! Correspondence:
//! - DUAL-CAP-1: `snapshot` fires exactly once, at construction; `root`/`len` are read exactly once there too (construction is O(1) in the dictionary — no `node_edges` before the first expansion).
//! - DUAL-CAP-2: zero `snapshot` calls occur during traversal, no matter how many resource retains or full walks the consumer performs.

#![cfg(feature = "ffi")]

mod support;

use std::ptr;

use duallity::ffi::{
    duallity_resource_release, duallity_wfst_free, duallity_wfst_new, duallity_wfst_resource,
    DuallityStatus, DuallityWfst,
};
use support::counting_dictionary::CountingDictionary;
use support::wfst_walk::WfstView;
use vinary_tree_interop::VtResource;

fn construct(dict: VtResource) -> *mut DuallityWfst {
    let mut wfst = ptr::null_mut();
    let status = duallity_wfst_new(dict, b"cat".as_ptr(), 3, 2, 0, 0, &mut wfst);
    assert_eq!(status, DuallityStatus::Ok);
    assert!(!wfst.is_null());
    wfst
}

#[test]
fn snapshot_is_captured_once_and_never_during_traversal() {
    let fixture = CountingDictionary::from_terms(&["cat", "car", "cot", "dog"]);
    let dict = fixture.resource();
    assert_eq!(fixture.snapshot_calls(), 0);
    assert_eq!(fixture.edges_calls(), 0);

    let wfst = construct(dict);

    // DUAL-CAP-1: exactly one snapshot, one root read, one len read; and no
    // edge paging yet — construction is O(1) in the dictionary size.
    assert_eq!(
        fixture.snapshot_calls(),
        1,
        "snapshot captured exactly once"
    );
    assert_eq!(fixture.root_calls(), 1, "root read exactly once at capture");
    assert_eq!(fixture.len_calls(), 1, "len read exactly once at capture");
    assert_eq!(
        fixture.edges_calls(),
        0,
        "no node_edges before the first lazy expansion"
    );

    // Traverse the whole reachable graph through the exported WFST surface.
    let mut resource = VtResource::NULL;
    assert_eq!(
        unsafe { duallity_wfst_resource(wfst, &mut resource) },
        DuallityStatus::Ok
    );
    let visited = {
        let view = WfstView::new(resource);
        view.visit_all()
    };
    assert!(
        visited > 1,
        "traversal must expand more than the start state"
    );

    // DUAL-CAP-2: traversal expanded states (node_edges fired) but captured no
    // new snapshot, and never re-read root/len.
    assert!(
        fixture.edges_calls() > 0,
        "traversal must page dictionary edges"
    );
    assert_eq!(
        fixture.snapshot_calls(),
        1,
        "zero snapshots during traversal"
    );
    assert_eq!(fixture.root_calls(), 1, "root not re-read during traversal");
    assert_eq!(fixture.len_calls(), 1, "len not re-read during traversal");

    duallity_resource_release(resource);
    unsafe { duallity_wfst_free(wfst) };
    duallity_resource_release(dict);
}

#[test]
fn repeated_retains_and_walks_never_re_snapshot() {
    // DUAL-CAP-2 under repetition: many resource retains and repeated full walks
    // of one handle must still show exactly the one construction-time snapshot.
    let fixture = CountingDictionary::from_terms(&["alpha", "alpine", "beta"]);
    let dict = fixture.resource();
    let wfst = construct(dict);
    assert_eq!(fixture.snapshot_calls(), 1);

    let mut retained = Vec::new();
    for _ in 0..4 {
        let mut resource = VtResource::NULL;
        assert_eq!(
            unsafe { duallity_wfst_resource(wfst, &mut resource) },
            DuallityStatus::Ok
        );
        let view = WfstView::new(resource);
        let _ = view.visit_all();
        let _ = view.visit_all();
        retained.push(resource);
    }
    assert_eq!(
        fixture.snapshot_calls(),
        1,
        "repeated retains and walks re-capture nothing"
    );

    for resource in retained {
        duallity_resource_release(resource);
    }
    unsafe { duallity_wfst_free(wfst) };
    duallity_resource_release(dict);
}

#[test]
fn each_construction_captures_its_own_single_snapshot() {
    // Two WFSTs from one source are two independent captures — exactly one
    // snapshot each, four total for four constructions.
    let fixture = CountingDictionary::from_terms(&["cat", "cot"]);
    let dict = fixture.resource();

    let mut handles = Vec::new();
    for index in 1..=4 {
        let wfst = construct(dict);
        assert_eq!(
            fixture.snapshot_calls(),
            index,
            "construction {index} adds exactly one snapshot"
        );
        handles.push(wfst);
    }

    for wfst in handles {
        unsafe { duallity_wfst_free(wfst) };
    }
    duallity_resource_release(dict);
}
