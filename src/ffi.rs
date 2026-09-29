//! Stable project-owned C ABI for duallity dictionary/WFST adapters.

// Revision-3 configuration, inspection, and cache control share the
// provider-owned snapshot and cache with the revision-2 entry points.
#[allow(dead_code)]
mod cache;
#[allow(dead_code)]
mod inspection;

use inspection::OwnedOptions;
#[allow(dead_code)]
mod config;
pub use config::{
    DuallityCacheStatisticsV1, DuallityGeneralizedLimitsV1, DuallityOperationV1,
    DuallityRecordHeaderV1, DuallityRestrictionV1, DuallityWfstOptionsV1,
};

use crate::bindings::{BindingError, WfstKind};
use crate::GeneralizedWfstError;
use liblevenshtein::cost::ScaleError;
use liblevenshtein::transducer::Algorithm;
use lling_llang::bindings::OwnedWfstResource;
use std::cell::RefCell;
use std::ffi::{c_char, CString};
use std::mem::{align_of, size_of};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;
use std::slice;
use std::str;
use vinary_tree_interop::{VtResource, VtStatus};

/// Stable duallity C ABI version.
pub const DUALLITY_ABI_VERSION: u32 = 1;
/// Additive project API revision.
pub const DUALLITY_API_REVISION: u32 = 3;

/// Status returned by duallity C functions.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DuallityStatus {
    /// Operation completed successfully.
    Ok = 0,
    /// An argument or enum value was invalid.
    InvalidArgument = 1,
    /// Query bytes were not valid UTF-8.
    InvalidUtf8 = 2,
    /// A required pointer was null.
    NullPointer = 3,
    /// A Rust panic was caught at the boundary.
    Panic = 4,
    /// The resource ABI/interface/domain is incompatible.
    IncompatibleResource = 5,
    /// A foreign provider callback failed.
    ProviderError = 6,
    /// A configured resource or numeric representation bound was exceeded.
    LimitExceeded = 7,
}

/// Opaque duallity WFST handle.
pub struct DuallityWfst {
    resource: OwnedWfstResource,
    options: OwnedOptions,
}

impl DuallityWfst {
    /// Read back effective options while the handle still owns nested data.
    fn inspect_options(&self) -> Result<config::DuallityWfstOptionsV1, DuallityStatus> {
        self.options.readback(&self.resource)
    }
}

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::new("ok").expect("literal has no NUL"));
}

fn set_error(message: impl Into<String>) {
    let message = message.into().replace('\0', "\\0");
    LAST_ERROR.with(|slot| {
        *slot.borrow_mut() = CString::new(message)
            .unwrap_or_else(|_| CString::new("invalid error message").unwrap());
    });
}

fn map_error(error: BindingError) -> DuallityStatus {
    set_error(error.to_string());
    match error {
        BindingError::NullResource => DuallityStatus::NullPointer,
        BindingError::Provider(VtStatus::LimitExceeded) => DuallityStatus::LimitExceeded,
        BindingError::Provider(_) | BindingError::InvalidProviderOutput(_) => {
            DuallityStatus::ProviderError
        }
        BindingError::InvalidArgument(_) => DuallityStatus::InvalidArgument,
        BindingError::Generalized(error) => match error {
            GeneralizedWfstError::LimitExceeded { .. }
            | GeneralizedWfstError::ArithmeticOverflow(_)
            | GeneralizedWfstError::AllocationFailed(_)
            | GeneralizedWfstError::CostScale(
                ScaleError::DenominatorOverflow | ScaleError::CostOverflow,
            ) => DuallityStatus::LimitExceeded,
            GeneralizedWfstError::MissingQuery
            | GeneralizedWfstError::InvalidOperations(_)
            | GeneralizedWfstError::CostScale(
                ScaleError::ZeroDenominator
                | ScaleError::NonFiniteWeight
                | ScaleError::NegativeWeight
                | ScaleError::InexactWeight { .. },
            ) => DuallityStatus::InvalidArgument,
        },
        BindingError::IncompatibleResourceAbi
        | BindingError::MissingDictionaryInterface
        | BindingError::IncompatibleDictionaryInterface
        | BindingError::UnitDomainMismatch(_) => DuallityStatus::IncompatibleResource,
    }
}

fn boundary(operation: impl FnOnce() -> Result<(), DuallityStatus>) -> DuallityStatus {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(Ok(())) => DuallityStatus::Ok,
        Ok(Err(status)) => status,
        Err(_) => {
            set_error("panic caught at duallity C boundary");
            DuallityStatus::Panic
        }
    }
}

fn output<'a, T>(pointer: *mut T, name: &'static str) -> Result<&'a mut T, DuallityStatus> {
    if pointer.is_null() {
        set_error(format!("{name} is null"));
        Err(DuallityStatus::NullPointer)
    } else {
        Ok(unsafe { &mut *pointer })
    }
}

fn checked_pointer<T>(pointer: *const T, name: &str) -> Result<*const T, DuallityStatus> {
    if pointer.is_null() {
        set_error(format!("{name} is null"));
        return Err(DuallityStatus::NullPointer);
    }
    if !(pointer as usize).is_multiple_of(align_of::<T>()) {
        set_error(format!("{name} is not aligned"));
        return Err(DuallityStatus::InvalidArgument);
    }
    Ok(pointer)
}

fn checked_mut_pointer<T>(pointer: *mut T, name: &str) -> Result<*mut T, DuallityStatus> {
    checked_pointer(pointer.cast_const(), name).map(|_| pointer)
}

/// # Safety
/// A non-null aligned pointer must designate a live duallity handle for the
/// duration of this call. The caller synchronizes its destruction.
unsafe fn live_handle<'a>(
    pointer: *const DuallityWfst,
) -> Result<&'a DuallityWfst, DuallityStatus> {
    let pointer = checked_pointer(pointer, "wfst")?;
    Ok(unsafe { &*pointer })
}

/// Validate the caller-declared writable record extent before any output.
///
/// # Safety
/// The pointer must be null or designate writable storage containing at least
/// a readable `DuallityRecordHeaderV1`; its declared extent must be writable.
unsafe fn sized_output_extent<T>(pointer: *mut T, name: &str) -> Result<usize, DuallityStatus> {
    let pointer = checked_mut_pointer(pointer, name)?;
    let header = unsafe { pointer.cast::<config::DuallityRecordHeaderV1>().read() };
    let extent = header.struct_size as usize;
    if extent < size_of::<T>() {
        set_error(format!("{name} needs at least {} bytes", size_of::<T>()));
        return Err(DuallityStatus::LimitExceeded);
    }
    if extent > config::CONFIG_MAX_RECORD_BYTES
        || header.record_version != config::CONFIG_RECORD_VERSION
        || header.reserved != 0
    {
        set_error(format!(
            "{name} has an unsupported size, version, or reserved field"
        ));
        return Err(DuallityStatus::InvalidArgument);
    }
    Ok(extent)
}

/// Commit a fully constructed record and zero newer unknown trailing fields.
///
/// # Safety
/// `pointer` must be aligned and writable for `extent` bytes, and `extent`
/// must be at least `size_of::<T>()`.
unsafe fn write_sized<T: Copy>(pointer: *mut T, extent: usize, value: T) {
    let known = size_of::<T>();
    if extent > known {
        unsafe { ptr::write_bytes(pointer.cast::<u8>().add(known), 0, extent - known) };
    }
    unsafe { pointer.write(value) };
}

fn algorithm(value: u32) -> Result<Algorithm, DuallityStatus> {
    match value {
        0 => Ok(Algorithm::Standard),
        1 => Ok(Algorithm::Transposition),
        2 => Ok(Algorithm::MergeAndSplit),
        3 => Ok(Algorithm::DamerauLevenshtein),
        _ => {
            set_error("unknown edit algorithm");
            Err(DuallityStatus::InvalidArgument)
        }
    }
}

fn kind(value: u32) -> Result<WfstKind, DuallityStatus> {
    match value {
        0 => Ok(WfstKind::Levenshtein),
        1 => Ok(WfstKind::UniversalStandard),
        2 => Ok(WfstKind::UniversalTransposition),
        3 => Ok(WfstKind::UniversalMergeAndSplit),
        4 => Ok(WfstKind::GeneralizedStandard),
        5 => Ok(WfstKind::GeneralizedTransposition),
        6 => Ok(WfstKind::GeneralizedMergeAndSplit),
        7 => Ok(WfstKind::GeneralizedPhonetic),
        8 => Ok(WfstKind::Fzf),
        _ => {
            set_error("unknown duallity WFST kind");
            Err(DuallityStatus::InvalidArgument)
        }
    }
}

fn query<'a>(data: *const u8, len: usize) -> Result<&'a str, DuallityStatus> {
    if data.is_null() && len != 0 {
        set_error("query data is null");
        return Err(DuallityStatus::NullPointer);
    }
    let bytes = if len == 0 {
        &[]
    } else {
        unsafe { slice::from_raw_parts(data, len) }
    };
    str::from_utf8(bytes).map_err(|_| {
        set_error("query is not valid UTF-8");
        DuallityStatus::InvalidUtf8
    })
}

/// Return the stable C ABI version.
#[no_mangle]
pub extern "C" fn duallity_abi_version() -> u32 {
    DUALLITY_ABI_VERSION
}

/// Return the additive project API revision.
#[no_mangle]
pub extern "C" fn duallity_api_revision() -> u32 {
    DUALLITY_API_REVISION
}

/// Return this thread's last boundary error.
#[no_mangle]
pub extern "C" fn duallity_last_error_message() -> *const c_char {
    LAST_ERROR.with(|slot| slot.borrow().as_ptr())
}

/// Capture a dictionary revision and create a lazy project-owned WFST.
#[no_mangle]
pub extern "C" fn duallity_wfst_new(
    dictionary: VtResource,
    query_data: *const u8,
    query_len: usize,
    maximum_distance: usize,
    algorithm_value: u32,
    kind_value: u32,
    out_wfst: *mut *mut DuallityWfst,
) -> DuallityStatus {
    boundary(|| {
        let query = query(query_data, query_len)?;
        let algorithm = algorithm(algorithm_value)?;
        let kind = kind(kind_value)?;
        let options = OwnedOptions::legacy(kind, algorithm, maximum_distance)?;
        let resource = unsafe {
            crate::bindings::create_wfst(dictionary, query, maximum_distance, algorithm, kind)
        }
        .map_err(map_error)?;
        // Resolve the output slot BEFORE relinquishing ownership of the boxed
        // handle. Written as one assignment, the right-hand side
        // `Box::into_raw(Box::new(..))` runs first: it constructs the handle
        // (which retains the dictionary snapshot) and hands out a raw pointer
        // with no owner, so a null `out_wfst` short-circuiting the left-hand
        // `output(..)?` afterward orphans the box and leaks the retained
        // snapshot. Binding `slot` first makes the null-pointer early return
        // drop `resource` (releasing the snapshot) instead. This preserves the
        // validation order (query, algorithm, kind, construction, then output).
        // Found by the W8 asan/lsan leg; see finding DUAL-B10.
        let slot = output(out_wfst, "out_wfst")?;
        *slot = Box::into_raw(Box::new(DuallityWfst { resource, options }));
        Ok(())
    })
}

/// Pointer-form constructor for FFIs that cannot pass C aggregates by value.
///
/// # Safety
/// `dictionary` must be null or point to a readable `VtResource` for this
/// call. The pointed-to resource remains borrowed; construction captures its
/// own immutable dictionary snapshot.
#[no_mangle]
pub unsafe extern "C" fn duallity_wfst_new_ref(
    dictionary: *const VtResource,
    query_data: *const u8,
    query_len: usize,
    maximum_distance: usize,
    algorithm_value: u32,
    kind_value: u32,
    out_wfst: *mut *mut DuallityWfst,
) -> DuallityStatus {
    if dictionary.is_null() {
        set_error("dictionary is null");
        return DuallityStatus::NullPointer;
    }
    duallity_wfst_new(
        unsafe { *dictionary },
        query_data,
        query_len,
        maximum_distance,
        algorithm_value,
        kind_value,
        out_wfst,
    )
}

/// Free a duallity WFST handle. Null is accepted.
///
/// # Safety
/// A non-null pointer must have been returned by `duallity_wfst_new` and must
/// not already have been freed.
#[no_mangle]
pub unsafe extern "C" fn duallity_wfst_free(wfst: *mut DuallityWfst) {
    if !wfst.is_null() {
        unsafe {
            drop(Box::from_raw(wfst));
        }
    }
}

/// Return a new owned scalar-WFST resource retain.
///
/// # Safety
/// `wfst` must point to a live handle returned by this API and `out_resource`
/// must be writable when non-null.
#[no_mangle]
pub unsafe extern "C" fn duallity_wfst_resource(
    wfst: *const DuallityWfst,
    out_resource: *mut VtResource,
) -> DuallityStatus {
    boundary(|| {
        if wfst.is_null() {
            set_error("wfst is null");
            return Err(DuallityStatus::NullPointer);
        }
        // Resolve the output slot BEFORE taking the retain, for the same reason
        // as DUAL-B10 in `duallity_wfst_new`: `clone().into_raw()` produces an
        // owned VtResource retain with no owner, so checking `out_resource` for
        // null afterward would leak that retain (its refcount is never
        // released). Binding `slot` first means a null `out_resource` returns
        // before any retain is taken.
        let slot = output(out_resource, "out_resource")?;
        *slot = unsafe { &*wfst }.resource.clone().into_raw();
        Ok(())
    })
}

/// Release one owned Vinary Tree resource retain.
#[no_mangle]
pub extern "C" fn duallity_resource_release(resource: VtResource) {
    if resource.context.is_null() || resource.vtable.is_null() {
        return;
    }
    unsafe {
        if let Some(release) = (*resource.vtable).release {
            release(resource.context);
        }
    }
}

/// Fill a caller-sized record with the standard configurable WFST defaults.
/// The caller initializes its header with its writable size and version 1.
///
/// # Safety
/// `out_options` must be null or writable for its declared byte extent.
#[no_mangle]
pub unsafe extern "C" fn duallity_wfst_options_default(
    out_options: *mut config::DuallityWfstOptionsV1,
) -> DuallityStatus {
    boundary(|| {
        let extent = unsafe { sized_output_extent(out_options, "out_options")? };
        let defaults = config::DuallityWfstOptionsV1 {
            header: config::DuallityRecordHeaderV1 {
                struct_size: size_of::<config::DuallityWfstOptionsV1>() as u32,
                record_version: config::CONFIG_RECORD_VERSION,
                reserved: 0,
            },
            kind: WfstKind::Levenshtein as u32,
            algorithm: 0,
            maximum_distance: 2,
            cache_policy: 0,
            reserved_zero: 0,
            cache_capacity: 0,
            limits: ptr::null(),
            operations: ptr::null(),
            operation_count: 0,
            operation_stride: 0,
            reserved: [0; 2],
        };
        unsafe { write_sized(out_options, extent, defaults) };
        Ok(())
    })
}

/// Validate and own a configured WFST before capturing one dictionary revision.
/// The output slot is cleared on every failure after it is validated.
///
/// # Safety
/// `dictionary`, `options`, and non-null nested pointers must designate live,
/// readable input for this call. `out_wfst` must be writable. Caller memory
/// must not be concurrently mutated during parsing or alias the output slot.
#[no_mangle]
pub unsafe extern "C" fn duallity_wfst_new_configured_ref(
    dictionary: *const VtResource,
    query_data: *const u8,
    query_len: usize,
    options: *const config::DuallityWfstOptionsV1,
    out_wfst: *mut *mut DuallityWfst,
) -> DuallityStatus {
    boundary(|| {
        let slot = checked_mut_pointer(out_wfst, "out_wfst")?;
        unsafe { slot.write(ptr::null_mut()) };
        let dictionary = checked_pointer(dictionary, "dictionary")?;
        let query = query(query_data, query_len)?;
        let parsed = unsafe { config::parse_options(options)? };
        let owned_options = inspection::OwnedOptions::from_parsed(&parsed)?;
        let resource = unsafe {
            crate::bindings::create_wfst_configured(
                dictionary.read(),
                query,
                parsed.into_construction(),
            )
        }
        .map_err(map_error)?;
        let handle = Box::into_raw(Box::new(DuallityWfst {
            resource,
            options: owned_options,
        }));
        unsafe { slot.write(handle) };
        Ok(())
    })
}

/// Copy effective options, borrowing nested pointers from the live handle.
///
/// # Safety
/// `wfst` must be a live handle. `out_options` must be writable for its
/// declared extent. Returned nested pointers expire when `wfst` is freed.
#[no_mangle]
pub unsafe extern "C" fn duallity_wfst_options_get(
    wfst: *const DuallityWfst,
    out_options: *mut config::DuallityWfstOptionsV1,
) -> DuallityStatus {
    boundary(|| {
        let extent = unsafe { sized_output_extent(out_options, "out_options")? };
        let handle = unsafe { live_handle(wfst)? };
        let record = handle.inspect_options()?;
        unsafe { write_sized(out_options, extent, record) };
        Ok(())
    })
}

/// Copy cumulative cache counters and a coherent current-residency view.
///
/// # Safety
/// `wfst` must be a live handle and `out_statistics` writable for its declared
/// extent. Concurrent counters may advance during the call.
#[no_mangle]
pub unsafe extern "C" fn duallity_wfst_cache_statistics(
    wfst: *const DuallityWfst,
    out_statistics: *mut config::DuallityCacheStatisticsV1,
) -> DuallityStatus {
    boundary(|| {
        let extent = unsafe { sized_output_extent(out_statistics, "out_statistics")? };
        let handle = unsafe { live_handle(wfst)? };
        let record = cache::statistics(&handle.resource)?;
        unsafe { write_sized(out_statistics, extent, record) };
        Ok(())
    })
}

/// Evict cached state payloads without changing the captured WFST semantics.
///
/// # Safety
/// `wfst` must be a live handle for this call; callers synchronize freeing it.
#[no_mangle]
pub unsafe extern "C" fn duallity_wfst_cache_clear(wfst: *mut DuallityWfst) -> DuallityStatus {
    boundary(|| {
        let handle = unsafe { live_handle(wfst.cast_const())? };
        cache::clear(&handle.resource)
    })
}

/// Publish a new cache policy and empty generation atomically.
///
/// # Safety
/// `wfst` must be a live handle for this call; callers synchronize freeing it.
#[no_mangle]
pub unsafe extern "C" fn duallity_wfst_cache_set_policy(
    wfst: *mut DuallityWfst,
    policy: u32,
    capacity: u64,
) -> DuallityStatus {
    boundary(|| {
        let handle = unsafe { live_handle(wfst.cast_const())? };
        let requested = config::cache_policy(policy, capacity)?;
        cache::set_policy(&handle.resource, requested, handle.options.kind())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use libdictenstein::bindings::{BindingUnitDomain, DynamicDawgBinding};
    use std::ptr;

    #[test]
    fn generalized_and_provider_error_classification_is_exact() {
        for status in 0..=9 {
            let provider = VtStatus::from_raw(status).expect("known status");
            assert_eq!(
                map_error(BindingError::Provider(provider)),
                if provider == VtStatus::LimitExceeded {
                    DuallityStatus::LimitExceeded
                } else {
                    DuallityStatus::ProviderError
                }
            );
        }
        for error in [
            GeneralizedWfstError::LimitExceeded {
                resource: crate::GeneralizedWfstResource::QueryBytes,
                limit: 1,
                required: 2,
            },
            GeneralizedWfstError::ArithmeticOverflow("state ID"),
            GeneralizedWfstError::AllocationFailed("query storage"),
            GeneralizedWfstError::CostScale(ScaleError::DenominatorOverflow),
            GeneralizedWfstError::CostScale(ScaleError::CostOverflow),
        ] {
            assert_eq!(
                map_error(BindingError::Generalized(error)),
                DuallityStatus::LimitExceeded
            );
        }
        for error in [
            GeneralizedWfstError::MissingQuery,
            GeneralizedWfstError::InvalidOperations(
                liblevenshtein::transducer::OperationSetValidationError::InvalidName {
                    index: 0,
                    observed: 0,
                    limit: 4096,
                },
            ),
            GeneralizedWfstError::CostScale(ScaleError::ZeroDenominator),
            GeneralizedWfstError::CostScale(ScaleError::NonFiniteWeight),
            GeneralizedWfstError::CostScale(ScaleError::NegativeWeight),
            GeneralizedWfstError::CostScale(ScaleError::InexactWeight {
                scale_denominator: 1,
                required_denominator: 2,
            }),
        ] {
            assert_eq!(
                map_error(BindingError::Generalized(error)),
                DuallityStatus::InvalidArgument
            );
        }
    }

    #[test]
    fn c_constructor_retains_query_start_dictionary_revision() {
        let dictionary = DynamicDawgBinding::new(BindingUnitDomain::UnicodeScalar);
        dictionary.insert_text(b"cat", None).unwrap();
        let source = dictionary.resource();
        let mut wfst = ptr::null_mut();
        assert_eq!(
            duallity_wfst_new(source.as_raw(), b"cat".as_ptr(), 3, 1, 0, 0, &mut wfst),
            DuallityStatus::Ok
        );
        dictionary.clear();
        drop(source);
        drop(dictionary);
        let mut resource = VtResource::NULL;
        assert_eq!(
            unsafe { duallity_wfst_resource(wfst, &mut resource) },
            DuallityStatus::Ok
        );
        unsafe { duallity_wfst_free(wfst) };
        assert!(!resource.is_null());
        duallity_resource_release(resource);
    }
}
