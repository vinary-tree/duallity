//! Revision-3 C entry-point behavior against an instrumented foreign provider.

#![cfg(feature = "ffi")]

mod support;

use duallity::ffi::{
    duallity_api_revision, duallity_resource_release, duallity_wfst_cache_clear,
    duallity_wfst_cache_set_policy, duallity_wfst_cache_statistics, duallity_wfst_free,
    duallity_wfst_new_configured_ref, duallity_wfst_options_default, duallity_wfst_options_get,
    duallity_wfst_resource, DuallityCacheStatisticsV1, DuallityRecordHeaderV1, DuallityStatus,
    DuallityWfstOptionsV1,
};
use std::{mem::size_of, ptr};
use support::counting_dictionary::{CaptureCallback, CountingDictionary};
use vinary_tree_interop::{VtResource, VtStatus};

fn header<T>() -> DuallityRecordHeaderV1 {
    DuallityRecordHeaderV1 {
        struct_size: size_of::<T>() as u32,
        record_version: 1,
        reserved: 0,
    }
}

fn defaults() -> DuallityWfstOptionsV1 {
    let mut options = DuallityWfstOptionsV1 {
        header: header::<DuallityWfstOptionsV1>(),
        kind: u32::MAX,
        algorithm: u32::MAX,
        maximum_distance: u64::MAX,
        cache_policy: u32::MAX,
        reserved_zero: u32::MAX,
        cache_capacity: u64::MAX,
        limits: ptr::null(),
        operations: ptr::null(),
        operation_count: u64::MAX,
        operation_stride: u64::MAX,
        reserved: [u64::MAX; 2],
    };
    assert_eq!(
        unsafe { duallity_wfst_options_default(&mut options) },
        DuallityStatus::Ok
    );
    assert_eq!(options.kind, 0);
    assert_eq!(options.algorithm, 0);
    assert_eq!(options.maximum_distance, 2);
    assert_eq!(options.cache_policy, 0);
    assert_eq!(options.reserved, [0; 2]);
    options
}

#[test]
fn configured_constructor_captures_once_and_handle_outlives_caller_inputs() {
    assert_eq!(duallity_api_revision(), 3);
    let fixture = CountingDictionary::from_terms(&["cat", "cot"]);
    let dictionary = fixture.resource();
    let mut options = defaults();
    options.cache_policy = 2;
    options.cache_capacity = 1;
    let mut handle = ptr::null_mut();
    assert_eq!(
        unsafe {
            duallity_wfst_new_configured_ref(&dictionary, b"cat".as_ptr(), 3, &options, &mut handle)
        },
        DuallityStatus::Ok
    );
    assert!(!handle.is_null());
    assert_eq!(fixture.snapshot_calls(), 1);
    assert_eq!(fixture.edges_calls(), 0);
    options.cache_capacity = 99;
    assert_eq!(options.cache_capacity, 99);
    let mut readback = defaults();
    assert_eq!(
        unsafe { duallity_wfst_options_get(handle, &mut readback) },
        DuallityStatus::Ok
    );
    assert_eq!(readback.cache_policy, 2);
    assert_eq!(readback.cache_capacity, 1);

    let mut stats = DuallityCacheStatisticsV1 {
        header: header::<DuallityCacheStatisticsV1>(),
        hits: 0,
        misses: 0,
        faults: 0,
        uncacheable_results: 0,
        insertions: 0,
        evictions: 0,
        raced_publications: 0,
        clears: 0,
        resident_states: 0,
        recency_records: 0,
        reserved: [0; 2],
    };
    assert_eq!(
        unsafe { duallity_wfst_cache_statistics(handle, &mut stats) },
        DuallityStatus::Ok
    );
    assert_eq!(stats.resident_states, 0);
    assert_eq!(
        unsafe { duallity_wfst_cache_clear(handle) },
        DuallityStatus::Ok
    );
    assert_eq!(
        unsafe { duallity_wfst_cache_set_policy(handle, 1, 0) },
        DuallityStatus::Ok
    );
    assert_eq!(
        unsafe { duallity_wfst_options_get(handle, &mut readback) },
        DuallityStatus::Ok
    );
    assert_eq!(readback.cache_policy, 1);
    assert_eq!(readback.cache_capacity, 0);
    assert_eq!(fixture.snapshot_calls(), 1);

    let mut retained = VtResource::NULL;
    assert_eq!(
        unsafe { duallity_wfst_resource(handle, &mut retained) },
        DuallityStatus::Ok
    );
    unsafe { duallity_wfst_free(handle) };
    assert!(!retained.is_null());
    duallity_resource_release(retained);
    duallity_resource_release(dictionary);
    assert_eq!(fixture.outstanding_retains(), 0);
}

#[test]
fn malformed_records_fail_before_capture_and_preserve_output_contracts() {
    let fixture = CountingDictionary::from_terms(&["cat"]);
    let dictionary = fixture.resource();
    let good = defaults();
    let bad_headers = [
        DuallityRecordHeaderV1 {
            struct_size: 15,
            ..good.header
        },
        DuallityRecordHeaderV1 {
            record_version: 2,
            ..good.header
        },
        DuallityRecordHeaderV1 {
            reserved: 1,
            ..good.header
        },
    ];
    for bad in bad_headers {
        let mut options = good;
        options.header = bad;
        let mut handle = ptr::dangling_mut::<duallity::ffi::DuallityWfst>();
        assert_ne!(
            unsafe {
                duallity_wfst_new_configured_ref(
                    &dictionary,
                    b"cat".as_ptr(),
                    3,
                    &options,
                    &mut handle,
                )
            },
            DuallityStatus::Ok
        );
        assert!(handle.is_null());
    }
    let unaligned_options = (&good as *const DuallityWfstOptionsV1)
        .cast::<u8>()
        .wrapping_add(1)
        .cast::<DuallityWfstOptionsV1>();
    let mut handle = ptr::dangling_mut::<duallity::ffi::DuallityWfst>();
    assert_eq!(
        unsafe {
            duallity_wfst_new_configured_ref(
                &dictionary,
                b"cat".as_ptr(),
                3,
                unaligned_options,
                &mut handle,
            )
        },
        DuallityStatus::InvalidArgument
    );
    assert!(handle.is_null());
    let mut handle = ptr::dangling_mut::<duallity::ffi::DuallityWfst>();
    assert_eq!(
        unsafe {
            duallity_wfst_new_configured_ref(&dictionary, [0xff].as_ptr(), 1, &good, &mut handle)
        },
        DuallityStatus::InvalidUtf8
    );
    assert!(handle.is_null());
    assert_eq!(fixture.snapshot_calls(), 0);

    let mut output = good;
    output.header.struct_size = 15;
    let sentinel = output.kind;
    assert_eq!(
        unsafe { duallity_wfst_options_default(&mut output) },
        DuallityStatus::LimitExceeded
    );
    assert_eq!(output.kind, sentinel);
    let unaligned_output = (&mut output as *mut DuallityWfstOptionsV1)
        .cast::<u8>()
        .wrapping_add(1)
        .cast::<DuallityWfstOptionsV1>();
    assert_eq!(
        unsafe { duallity_wfst_options_default(unaligned_output) },
        DuallityStatus::InvalidArgument
    );
    duallity_resource_release(dictionary);
    assert_eq!(fixture.outstanding_retains(), 0);
}

#[test]
fn capture_failures_do_not_leak_and_invalid_policy_does_not_mutate_live_handle() {
    for callback in [
        CaptureCallback::Snapshot,
        CaptureCallback::Root,
        CaptureCallback::Len,
    ] {
        let fixture = CountingDictionary::failing_capture(callback, VtStatus::LimitExceeded);
        let dictionary = fixture.resource();
        let mut handle = ptr::null_mut();
        assert_eq!(
            unsafe {
                duallity_wfst_new_configured_ref(
                    &dictionary,
                    b"cat".as_ptr(),
                    3,
                    &defaults(),
                    &mut handle,
                )
            },
            DuallityStatus::LimitExceeded
        );
        assert!(handle.is_null());
        duallity_resource_release(dictionary);
        assert_eq!(fixture.outstanding_retains(), 0);
    }

    let fixture = CountingDictionary::from_terms(&["cat"]);
    let dictionary = fixture.resource();
    let mut handle = ptr::null_mut();
    assert_eq!(
        unsafe {
            duallity_wfst_new_configured_ref(
                &dictionary,
                b"cat".as_ptr(),
                3,
                &defaults(),
                &mut handle,
            )
        },
        DuallityStatus::Ok
    );
    assert_eq!(
        unsafe { duallity_wfst_cache_set_policy(handle, 1, 1) },
        DuallityStatus::InvalidArgument
    );
    let mut inspected = defaults();
    assert_eq!(
        unsafe { duallity_wfst_options_get(handle, &mut inspected) },
        DuallityStatus::Ok
    );
    assert_eq!(inspected.cache_policy, 0);
    unsafe { duallity_wfst_free(handle) };
    duallity_resource_release(dictionary);
    assert_eq!(fixture.outstanding_retains(), 0);
}
