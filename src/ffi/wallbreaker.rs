//! Versioned, bounded ingress for native WallBreaker match results.

use super::{algorithm, boundary, query, set_error, DuallityStatus, DuallityWfst};
use crate::bindings::{create_wallbreaker_result_wfst, WfstKind};
use liblevenshtein::wallbreaker::WallBreakerResult;
use std::collections::HashSet;
use std::ptr;
use std::slice;
use std::str;

const MAX_RESULTS: usize = 4_096;
const MAX_RESULT_BYTES: usize = 1 << 20;
const MAX_TEXT_SCALARS: usize = 256;

/// One native WallBreaker match, borrowed only during construction.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DuallityWallBreakerResultV1 {
    pub header: super::DuallityRecordHeaderV1,
    pub term_data: *const u8,
    pub term_len: u64,
    pub distance: u64,
    pub reserved: [u64; 2],
}

fn reject(status: DuallityStatus, message: &str) -> DuallityStatus {
    set_error(message);
    status
}

/// Build an owned, cache-controlled scalar WFST from a complete native
/// WallBreaker result set. The result set is supplied by the caller; the
/// constructor validates its shape and bounds, then deep-copies every term.
///
/// # Safety
/// The query, result array, each result record, each term buffer, and the
/// output slot must remain readable or writable as appropriate for this call.
/// Caller memory must not be concurrently mutated or alias the output slot.
#[no_mangle]
pub unsafe extern "C" fn duallity_wallbreaker_wfst_new_results(
    query_data: *const u8,
    query_len: usize,
    algorithm_value: u32,
    maximum_distance: u64,
    results: *const DuallityWallBreakerResultV1,
    result_count: u64,
    result_stride: u64,
    cache_policy: u32,
    cache_capacity: u64,
    out_wfst: *mut *mut DuallityWfst,
) -> DuallityStatus {
    boundary(|| {
        let slot = super::checked_mut_pointer(out_wfst, "out_wfst")?;
        unsafe { slot.write(ptr::null_mut()) };
        let query = query(query_data, query_len)?;
        if query.chars().count() > MAX_TEXT_SCALARS {
            return Err(reject(
                DuallityStatus::LimitExceeded,
                "WallBreaker query scalar limit exceeded",
            ));
        }
        if maximum_distance > 8 {
            return Err(reject(
                DuallityStatus::InvalidArgument,
                "WallBreaker maximum_distance must be in 0:8",
            ));
        }
        let algorithm = algorithm(algorithm_value)?;
        if algorithm_value > 2 {
            return Err(reject(
                DuallityStatus::InvalidArgument,
                "WallBreaker supports standard, transposition, and merge-and-split algorithms",
            ));
        }
        let policy = super::config::cache_policy(cache_policy, cache_capacity)?
            .effective(WfstKind::Levenshtein);
        let count = super::config::array_shape(
            results,
            result_count,
            result_stride,
            MAX_RESULTS,
            "WallBreaker results",
        )?;
        let stride = result_stride as usize;
        let mut owned = Vec::with_capacity(count);
        let mut seen = HashSet::with_capacity(count);
        let mut total_bytes = 0usize;
        for index in 0..count {
            let pointer = unsafe { results.cast::<u8>().add(index * stride) }
                .cast::<DuallityWallBreakerResultV1>();
            let item = unsafe { super::config::record(pointer, "WallBreaker result", stride)? };
            if item.reserved != [0; 2] {
                return Err(reject(
                    DuallityStatus::InvalidArgument,
                    "WallBreaker result reserved fields are nonzero",
                ));
            }
            if item.distance > maximum_distance {
                return Err(reject(
                    DuallityStatus::InvalidArgument,
                    "WallBreaker result distance exceeds maximum_distance",
                ));
            }
            let term_len = usize::try_from(item.term_len).map_err(|_| {
                reject(
                    DuallityStatus::LimitExceeded,
                    "WallBreaker result length exceeds address space",
                )
            })?;
            total_bytes = total_bytes.checked_add(term_len).ok_or_else(|| {
                reject(
                    DuallityStatus::LimitExceeded,
                    "WallBreaker result byte total overflows",
                )
            })?;
            if total_bytes > MAX_RESULT_BYTES {
                return Err(reject(
                    DuallityStatus::LimitExceeded,
                    "WallBreaker result byte limit exceeded",
                ));
            }
            if term_len > 0 && item.term_data.is_null() {
                return Err(reject(
                    DuallityStatus::NullPointer,
                    "WallBreaker result term_data is null",
                ));
            }
            let bytes = if term_len == 0 {
                &[][..]
            } else {
                unsafe { slice::from_raw_parts(item.term_data, term_len) }
            };
            let term = str::from_utf8(bytes).map_err(|_| {
                reject(
                    DuallityStatus::InvalidUtf8,
                    "WallBreaker result term is not UTF-8",
                )
            })?;
            if term.chars().count() > MAX_TEXT_SCALARS {
                return Err(reject(
                    DuallityStatus::LimitExceeded,
                    "WallBreaker result term scalar limit exceeded",
                ));
            }
            if !seen.insert(term.to_owned()) {
                return Err(reject(
                    DuallityStatus::InvalidArgument,
                    "WallBreaker result term is duplicated",
                ));
            }
            owned.push(WallBreakerResult::new(
                term.to_owned(),
                item.distance as usize,
            ));
        }
        let resource = create_wallbreaker_result_wfst(
            query.to_owned(),
            maximum_distance as usize,
            algorithm,
            owned,
            policy,
        );
        let handle = Box::into_raw(Box::new(DuallityWfst {
            resource,
            options: None,
        }));
        unsafe { slot.write(handle) };
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::super::{
        duallity_wfst_cache_clear, duallity_wfst_cache_set_policy, duallity_wfst_free,
    };
    use super::*;
    use std::ffi::c_void;
    use std::mem::size_of;
    use vinary_tree_interop::{
        VtStatus, VtWfstVTable, VT_WFST_INTERFACE_ID, VT_WFST_INTERFACE_VERSION,
    };

    fn record(term: &[u8], distance: u64) -> DuallityWallBreakerResultV1 {
        DuallityWallBreakerResultV1 {
            header: super::super::DuallityRecordHeaderV1 {
                struct_size: size_of::<DuallityWallBreakerResultV1>() as u32,
                record_version: 1,
                reserved: 0,
            },
            term_data: if term.is_empty() {
                ptr::null()
            } else {
                term.as_ptr()
            },
            term_len: term.len() as u64,
            distance,
            reserved: [0; 2],
        }
    }

    unsafe fn state_info(handle: *const DuallityWfst, state: u64) -> (u32, u8, u8) {
        let raw = unsafe { (*handle).resource.as_raw() };
        let mut interface: *const c_void = ptr::null();
        assert_eq!(
            unsafe {
                (*raw.vtable).query_interface.unwrap()(
                    raw.context,
                    &VT_WFST_INTERFACE_ID,
                    VT_WFST_INTERFACE_VERSION,
                    &mut interface,
                )
            },
            VtStatus::Ok.to_raw()
        );
        let table = unsafe { &*interface.cast::<VtWfstVTable>() };
        let (mut valid, mut final_state, mut weight) = (0, 0, 0.0);
        let code = unsafe {
            table.state_info.unwrap()(
                raw.context,
                state,
                &mut valid,
                &mut final_state,
                &mut weight,
            )
        };
        (code, valid, final_state)
    }

    #[test]
    fn result_forest_uses_the_exported_cache_and_survives_source_buffers() {
        let query = b"cat";
        let cat = b"cat".to_vec();
        let cot = b"cot".to_vec();
        let records = [record(&cat, 0), record(&cot, 1)];
        let mut handle = ptr::null_mut();
        assert_eq!(
            unsafe {
                duallity_wallbreaker_wfst_new_results(
                    query.as_ptr(),
                    query.len(),
                    0,
                    1,
                    records.as_ptr(),
                    2,
                    size_of::<DuallityWallBreakerResultV1>() as u64,
                    0,
                    0,
                    &mut handle,
                )
            },
            DuallityStatus::Ok
        );
        assert!(!handle.is_null());
        drop((cat, cot));
        unsafe {
            assert_eq!(state_info(handle, 0), (VtStatus::Ok.to_raw(), 1, 0));
            assert_eq!(state_info(handle, 0), (VtStatus::Ok.to_raw(), 1, 0));
            let control = (*handle).resource.provider_cache().unwrap();
            assert_eq!(
                (control.statistics().misses, control.statistics().hits),
                (1, 1)
            );
            assert_eq!(duallity_wfst_cache_clear(handle), DuallityStatus::Ok);
            assert_eq!(control.statistics().resident_states, 0);
            assert_eq!(
                duallity_wfst_cache_set_policy(handle, 1, 0),
                DuallityStatus::Ok
            );
            assert_eq!(state_info(handle, 0), (VtStatus::Ok.to_raw(), 1, 0));
            assert_eq!(control.statistics().resident_states, 0);
            assert_eq!(
                duallity_wfst_cache_set_policy(handle, 2, 1),
                DuallityStatus::Ok
            );
            assert_eq!(state_info(handle, 0), (VtStatus::Ok.to_raw(), 1, 0));
            assert_eq!(state_info(handle, 1), (VtStatus::Ok.to_raw(), 1, 0));
            assert_eq!(control.statistics().resident_states, 1);
            assert!(control.statistics().evictions > 0);
            duallity_wfst_free(handle);
        }
    }

    #[test]
    fn result_ingress_rejects_invalid_and_noncanonical_records() {
        let query = b"cat";
        let cat = b"cat";
        let stride = size_of::<DuallityWallBreakerResultV1>() as u64;
        let mut handle = 1usize as *mut DuallityWfst;
        let mut invalid = record(cat, 2);
        assert_eq!(
            unsafe {
                duallity_wallbreaker_wfst_new_results(
                    query.as_ptr(),
                    query.len(),
                    0,
                    1,
                    &invalid,
                    1,
                    stride,
                    0,
                    0,
                    &mut handle,
                )
            },
            DuallityStatus::InvalidArgument
        );
        assert!(handle.is_null());
        invalid.distance = 0;
        invalid.header.record_version = 2;
        assert_eq!(
            unsafe {
                duallity_wallbreaker_wfst_new_results(
                    query.as_ptr(),
                    query.len(),
                    0,
                    1,
                    &invalid,
                    1,
                    stride,
                    0,
                    0,
                    &mut handle,
                )
            },
            DuallityStatus::InvalidArgument
        );
        invalid.header.record_version = 1;
        let duplicates = [invalid, invalid];
        assert_eq!(
            unsafe {
                duallity_wallbreaker_wfst_new_results(
                    query.as_ptr(),
                    query.len(),
                    0,
                    1,
                    duplicates.as_ptr(),
                    2,
                    stride,
                    0,
                    0,
                    &mut handle,
                )
            },
            DuallityStatus::InvalidArgument
        );
        let malformed = record(&[0xff], 0);
        assert_eq!(
            unsafe {
                duallity_wallbreaker_wfst_new_results(
                    query.as_ptr(),
                    query.len(),
                    0,
                    1,
                    &malformed,
                    1,
                    stride,
                    0,
                    0,
                    &mut handle,
                )
            },
            DuallityStatus::InvalidUtf8
        );
        assert_eq!(
            unsafe {
                duallity_wallbreaker_wfst_new_results(
                    query.as_ptr(),
                    query.len(),
                    3,
                    1,
                    ptr::null(),
                    0,
                    0,
                    0,
                    0,
                    &mut handle,
                )
            },
            DuallityStatus::InvalidArgument
        );
    }
}
