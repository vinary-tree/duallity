//! Stable project-owned C ABI for duallity dictionary/WFST adapters.

use crate::bindings::{BindingError, WfstKind};
use liblevenshtein::transducer::Algorithm;
use lling_llang::bindings::OwnedWfstResource;
use std::cell::RefCell;
use std::ffi::{c_char, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::slice;
use std::str;
use vinary_tree_interop::VtResource;

/// Stable duallity C ABI version.
pub const DUALLITY_ABI_VERSION: u32 = 1;
/// Additive project API revision.
pub const DUALLITY_API_REVISION: u32 = 1;

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
    /// A numeric representation bound was exceeded.
    LimitExceeded = 7,
}

/// Opaque duallity WFST handle.
pub struct DuallityWfst {
    resource: OwnedWfstResource,
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
        BindingError::Provider(_) | BindingError::InvalidProviderOutput(_) => {
            DuallityStatus::ProviderError
        }
        BindingError::InvalidArgument(_) => DuallityStatus::InvalidArgument,
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
        *slot = Box::into_raw(Box::new(DuallityWfst { resource }));
        Ok(())
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use libdictenstein::bindings::{BindingUnitDomain, DynamicDawgBinding};
    use std::ptr;

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
