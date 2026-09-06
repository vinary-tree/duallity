//! End-to-end constructor limit classification and snapshot ownership.

#![cfg(feature = "ffi")]

mod support;

use duallity::bindings::{create_wfst, BindingError, WfstKind};
use duallity::ffi::{
    duallity_resource_release, duallity_wfst_free, duallity_wfst_new, DuallityStatus,
};
use duallity::{GeneralizedWfstError, GeneralizedWfstLimits, GeneralizedWfstResource};
use liblevenshtein::transducer::Algorithm;
use std::ptr;
use support::counting_dictionary::{CaptureCallback, CountingDictionary};
use vinary_tree_interop::VtStatus;

#[test]
fn generalized_query_limits_survive_the_typed_and_c_constructor_boundaries() {
    let limits = GeneralizedWfstLimits::default();
    let queries = [
        (
            "a".repeat(limits.max_query_scalars + 1),
            GeneralizedWfstResource::QueryScalars,
        ),
        (
            format!("{}a", "\u{10000}".repeat(limits.max_query_bytes / 4)),
            GeneralizedWfstResource::QueryBytes,
        ),
    ];
    for (query, resource) in queries {
        for kind in [
            WfstKind::GeneralizedStandard,
            WfstKind::GeneralizedTransposition,
            WfstKind::GeneralizedMergeAndSplit,
            WfstKind::GeneralizedPhonetic,
        ] {
            let fixture = CountingDictionary::from_terms(&["a"]);
            let raw = fixture.resource();
            let error = unsafe { create_wfst(raw, &query, 1, Algorithm::Standard, kind) }
                .err()
                .expect("oversized query must fail");
            assert!(matches!(error, BindingError::Generalized(
                GeneralizedWfstError::LimitExceeded { resource: actual, .. }
            ) if actual == resource));
            assert!(std::error::Error::source(&error).is_some());
            assert_eq!(fixture.outstanding_retains(), 1);
            let mut handle = ptr::null_mut();
            assert_eq!(
                duallity_wfst_new(
                    raw,
                    query.as_ptr(),
                    query.len(),
                    1,
                    0,
                    kind as u32,
                    &mut handle
                ),
                DuallityStatus::LimitExceeded,
                "kind {kind:?}, resource {resource:?}"
            );
            assert!(handle.is_null());
            assert_eq!(fixture.edges_calls(), 0);
            duallity_resource_release(raw);
            assert_eq!(fixture.outstanding_retains(), 0);
        }
    }
}

#[test]
fn generalized_accepts_the_exact_default_query_byte_and_scalar_limits() {
    let limits = GeneralizedWfstLimits::default();
    let query = "\u{10000}".repeat(limits.max_query_scalars);
    assert_eq!(query.len(), limits.max_query_bytes);
    let fixture = CountingDictionary::from_terms(&["a"]);
    let raw = fixture.resource();
    for kind in 4..=7 {
        let mut handle = ptr::null_mut();
        assert_eq!(
            duallity_wfst_new(raw, query.as_ptr(), query.len(), 1, 0, kind, &mut handle),
            DuallityStatus::Ok
        );
        assert!(!handle.is_null());
        unsafe { duallity_wfst_free(handle) };
        assert_eq!(fixture.outstanding_retains(), 1);
    }
    duallity_resource_release(raw);
    assert_eq!(fixture.outstanding_retains(), 0);
}

#[test]
fn every_constructor_preserves_provider_limits_without_leaking_a_snapshot() {
    for callback in [
        CaptureCallback::Snapshot,
        CaptureCallback::Root,
        CaptureCallback::Len,
    ] {
        for status in [VtStatus::LimitExceeded, VtStatus::Closed, VtStatus::IoError] {
            for kind in 0..=8 {
                let fixture = CountingDictionary::failing_capture(callback, status);
                let raw = fixture.resource();
                let mut handle = ptr::null_mut();
                let expected = if status == VtStatus::LimitExceeded {
                    DuallityStatus::LimitExceeded
                } else {
                    // The stable duallity alphabet has no Closed or IoError code.
                    DuallityStatus::ProviderError
                };
                assert_eq!(
                    duallity_wfst_new(raw, b"a".as_ptr(), 1, 1, 0, kind, &mut handle),
                    expected,
                    "kind {kind}, callback {callback:?}, status {status:?}"
                );
                assert!(handle.is_null());
                assert_eq!(fixture.edges_calls(), 0);
                assert_eq!(fixture.outstanding_retains(), 1);
                duallity_resource_release(raw);
                assert_eq!(fixture.outstanding_retains(), 0);
            }
        }
    }
}
