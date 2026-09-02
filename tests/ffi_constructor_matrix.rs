//! Exhaustive `duallity_wfst_new` constructor matrix over the exported C ABI.
//!
//! Cross-product coverage: the 9 WFST kinds × 4 edit algorithms × 7 query
//! classes × 4 maximum distances, each asserted against the exact status the
//! `src/ffi.rs` validation ladder must return. Plus the out-of-range enum
//! rejections, the U64-domain incompatibility, null-pointer handling, the
//! version pins, and thread-local `duallity_last_error_message` correctness.
//!
//! Correspondence:
//! - DUAL-ENC-1: a valid (kind, algorithm, query, distance) constructs a live handle whose resource round-trips; invalid inputs are rejected without producing a handle.
//! - DUAL-DICT-1: a non-UnicodeScalar dictionary is refused as IncompatibleResource.
//! - DUAL-VER-1: `duallity_abi_version`/`duallity_api_revision` are pinned.
//! - DUAL-ERR-1: the thread-local last-error message matches the rejection.
//! - DUAL-B10: a null output pointer on `duallity_wfst_new` / `duallity_wfst_resource` is rejected without leaking the constructed handle or the retain (verified under the W8 asan/lsan leg).

#![cfg(feature = "ffi")]

use std::ffi::CStr;
use std::ptr;

use duallity::ffi::{
    duallity_abi_version, duallity_api_revision, duallity_last_error_message,
    duallity_resource_release, duallity_wfst_free, duallity_wfst_new, duallity_wfst_new_ref,
    duallity_wfst_resource, DuallityStatus, DuallityWfst, DUALLITY_ABI_VERSION,
    DUALLITY_API_REVISION,
};
use libdictenstein::bindings::{BindingUnitDomain, DynamicDawgBinding};
use vinary_tree_interop::VtResource;

// ---------------------------------------------------------------------------
// Enum encodings mirrored from src/ffi.rs (kept in lockstep on purpose).
// ---------------------------------------------------------------------------
const KIND_COUNT: u32 = 9; // 0..=8 valid; 9+ invalid.
const ALGORITHM_COUNT: u32 = 4; // 0..=3 valid; 4+ invalid.

/// The maximum distance a Universal/Generalized adapter accepts (u8-bounded).
const MAX_U8_DISTANCE: usize = u8::MAX as usize;

fn last_error() -> String {
    unsafe {
        CStr::from_ptr(duallity_last_error_message())
            .to_string_lossy()
            .into_owned()
    }
}

fn unicode_dictionary() -> DynamicDawgBinding {
    let dictionary = DynamicDawgBinding::new(BindingUnitDomain::UnicodeScalar);
    for term in ["cat", "cot", "café", "🦀ab", "a"] {
        dictionary.insert_text(term.as_bytes(), None).unwrap();
    }
    dictionary
}

/// One query input class and the raw `(pointer, length)` it produces.
struct QueryCase {
    name: &'static str,
    bytes: Vec<u8>,
    null_pointer: bool,
    length: usize,
}

impl QueryCase {
    fn valid(name: &'static str, bytes: Vec<u8>) -> Self {
        let length = bytes.len();
        Self {
            name,
            bytes,
            null_pointer: false,
            length,
        }
    }

    fn pointer(&self) -> *const u8 {
        if self.null_pointer {
            ptr::null()
        } else {
            self.bytes.as_ptr()
        }
    }
}

fn query_cases() -> Vec<QueryCase> {
    vec![
        // A null pointer with zero length is the canonical empty query.
        QueryCase {
            name: "empty-null",
            bytes: Vec::new(),
            null_pointer: true,
            length: 0,
        },
        QueryCase::valid("ascii", b"cat".to_vec()),
        QueryCase::valid("accented", "café".as_bytes().to_vec()),
        QueryCase::valid("emoji", "🦀ab".as_bytes().to_vec()),
        QueryCase::valid("long", vec![b'a'; 300]),
        // 0xFF is never a valid UTF-8 lead byte.
        QueryCase::valid("invalid-utf8", vec![0xff, 0xff, 0xff]),
        // A null pointer with a nonzero length is a caller error.
        QueryCase {
            name: "null-with-len",
            bytes: Vec::new(),
            null_pointer: true,
            length: 3,
        },
    ]
}

/// The status the validation ladder must return for a (query, algorithm, kind,
/// distance) with a UnicodeScalar dictionary. Mirrors `duallity_wfst_new`:
/// query is validated first, then algorithm, then kind, then construction.
fn expected(query: &QueryCase, algorithm: u32, kind: u32, distance: usize) -> DuallityStatus {
    if query.null_pointer && query.length != 0 {
        return DuallityStatus::NullPointer;
    }
    if query.name == "invalid-utf8" {
        return DuallityStatus::InvalidUtf8;
    }
    if algorithm >= ALGORITHM_COUNT || kind >= KIND_COUNT {
        return DuallityStatus::InvalidArgument;
    }
    match kind {
        // Levenshtein (0) accepts any usize distance; Fzf (8) ignores distance.
        0 | 8 => DuallityStatus::Ok,
        // Universal (1..=3) and Generalized (4..=7) require a u8-bounded distance.
        _ if distance > MAX_U8_DISTANCE => DuallityStatus::InvalidArgument,
        _ => DuallityStatus::Ok,
    }
}

#[test]
fn constructor_matrix_covers_kinds_algorithms_queries_distances() {
    let dictionary = unicode_dictionary();
    let source = dictionary.resource();
    let distances = [0usize, 1, 3, 1000];

    for query in query_cases() {
        for kind in 0..KIND_COUNT {
            for algorithm in 0..ALGORITHM_COUNT {
                for distance in distances {
                    let want = expected(&query, algorithm, kind, distance);
                    let mut wfst: *mut DuallityWfst = ptr::null_mut();
                    let status = duallity_wfst_new(
                        source.as_raw(),
                        query.pointer(),
                        query.length,
                        distance,
                        algorithm,
                        kind,
                        &mut wfst,
                    );
                    assert_eq!(
                        status, want,
                        "query={} algorithm={} kind={} distance={}",
                        query.name, algorithm, kind, distance
                    );
                    if status == DuallityStatus::Ok {
                        assert!(
                            !wfst.is_null(),
                            "Ok must yield a handle (query={}, kind={})",
                            query.name,
                            kind
                        );
                        // DUAL-ENC-1: a valid construction round-trips to a live
                        // scalar-WFST resource retain.
                        let mut resource = VtResource::NULL;
                        assert_eq!(
                            unsafe { duallity_wfst_resource(wfst, &mut resource) },
                            DuallityStatus::Ok
                        );
                        assert!(!resource.is_null());
                        duallity_resource_release(resource);
                        unsafe { duallity_wfst_free(wfst) };
                    } else {
                        assert!(
                            wfst.is_null(),
                            "a rejected construction must not write a handle \
                             (query={}, kind={}, algorithm={}, distance={})",
                            query.name,
                            kind,
                            algorithm,
                            distance
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn out_of_range_algorithm_is_invalid_argument() {
    let dictionary = unicode_dictionary();
    let source = dictionary.resource();
    for algorithm in [ALGORITHM_COUNT, ALGORITHM_COUNT + 7, u32::MAX] {
        let mut wfst = ptr::null_mut();
        let status = duallity_wfst_new(
            source.as_raw(),
            b"cat".as_ptr(),
            3,
            1,
            algorithm,
            0,
            &mut wfst,
        );
        assert_eq!(
            status,
            DuallityStatus::InvalidArgument,
            "algorithm={algorithm}"
        );
        assert!(wfst.is_null());
        assert_eq!(last_error(), "unknown edit algorithm");
    }
}

#[test]
fn out_of_range_kind_is_invalid_argument() {
    let dictionary = unicode_dictionary();
    let source = dictionary.resource();
    for kind in [KIND_COUNT, KIND_COUNT + 3, u32::MAX] {
        let mut wfst = ptr::null_mut();
        let status = duallity_wfst_new(source.as_raw(), b"cat".as_ptr(), 3, 1, 0, kind, &mut wfst);
        assert_eq!(status, DuallityStatus::InvalidArgument, "kind={kind}");
        assert!(wfst.is_null());
        assert_eq!(last_error(), "unknown duallity WFST kind");
    }
}

#[test]
fn universal_and_generalized_reject_distance_above_u8() {
    let dictionary = unicode_dictionary();
    let source = dictionary.resource();
    let distance = MAX_U8_DISTANCE + 1;
    let cases = [
        (1u32, "universal maximum distance must fit u8"),
        (4u32, "generalized maximum distance must fit u8"),
    ];
    for (kind, message) in cases {
        let mut wfst = ptr::null_mut();
        let status = duallity_wfst_new(
            source.as_raw(),
            b"cat".as_ptr(),
            3,
            distance,
            0,
            kind,
            &mut wfst,
        );
        assert_eq!(status, DuallityStatus::InvalidArgument, "kind={kind}");
        assert!(wfst.is_null());
        assert_eq!(last_error(), message);
    }
}

#[test]
fn levenshtein_accepts_saturating_distance() {
    // Kind 0 stores the distance lazily; even usize::MAX must construct.
    let dictionary = unicode_dictionary();
    let source = dictionary.resource();
    let mut wfst = ptr::null_mut();
    let status = duallity_wfst_new(
        source.as_raw(),
        b"cat".as_ptr(),
        3,
        usize::MAX,
        0,
        0,
        &mut wfst,
    );
    assert_eq!(status, DuallityStatus::Ok);
    assert!(!wfst.is_null());
    unsafe { duallity_wfst_free(wfst) };
}

#[test]
fn u64_domain_dictionary_is_incompatible_resource() {
    // A U64-token dictionary snapshots fine but is not a text (UnicodeScalar)
    // dictionary, so duallity's text adapters must refuse it.
    let dictionary = DynamicDawgBinding::new(BindingUnitDomain::U64);
    let source = dictionary.resource();
    let mut wfst = ptr::null_mut();
    let status = duallity_wfst_new(source.as_raw(), b"cat".as_ptr(), 3, 1, 0, 0, &mut wfst);
    assert_eq!(status, DuallityStatus::IncompatibleResource);
    assert!(wfst.is_null());
    assert_eq!(last_error(), "expected Unicode scalar dictionary, got U64");
}

#[test]
fn null_output_pointer_is_null_pointer() {
    let dictionary = unicode_dictionary();
    let source = dictionary.resource();
    let status = duallity_wfst_new(
        source.as_raw(),
        b"cat".as_ptr(),
        3,
        1,
        0,
        0,
        ptr::null_mut(),
    );
    assert_eq!(status, DuallityStatus::NullPointer);
    assert_eq!(last_error(), "out_wfst is null");
}

#[test]
fn null_out_resource_pointer_is_null_pointer() {
    // Companion to `null_output_pointer_is_null_pointer` for the resource
    // accessor. A null `out_resource` must be rejected BEFORE a retain is taken:
    // otherwise the `clone().into_raw()` retain is orphaned and the captured
    // dictionary snapshot leaks. This is the `duallity_wfst_resource` instance
    // of DUAL-B10 (the `duallity_wfst_new` instance was caught empirically by
    // the W8 asan/lsan leg); under that leg this test's construction retain must
    // be released on the early return, so a leak here fails CI.
    let dictionary = unicode_dictionary();
    let source = dictionary.resource();
    let mut wfst = ptr::null_mut();
    assert_eq!(
        duallity_wfst_new(source.as_raw(), b"cat".as_ptr(), 3, 1, 0, 0, &mut wfst),
        DuallityStatus::Ok
    );
    assert!(!wfst.is_null());
    let status = unsafe { duallity_wfst_resource(wfst, ptr::null_mut()) };
    assert_eq!(status, DuallityStatus::NullPointer);
    assert_eq!(last_error(), "out_resource is null");
    unsafe { duallity_wfst_free(wfst) };
}

#[test]
fn null_query_with_length_reports_null_pointer_message() {
    let dictionary = unicode_dictionary();
    let source = dictionary.resource();
    let mut wfst = ptr::null_mut();
    let status = duallity_wfst_new(source.as_raw(), ptr::null(), 3, 1, 0, 0, &mut wfst);
    assert_eq!(status, DuallityStatus::NullPointer);
    assert!(wfst.is_null());
    assert_eq!(last_error(), "query data is null");
}

#[test]
fn pointer_form_preserves_constructor_semantics_and_checks_the_resource_pointer() {
    let dictionary = unicode_dictionary();
    let source = dictionary.resource();
    let raw = source.as_raw();
    let mut wfst = ptr::null_mut();
    assert_eq!(
        unsafe { duallity_wfst_new_ref(ptr::null(), b"cat".as_ptr(), 3, 1, 0, 0, &mut wfst) },
        DuallityStatus::NullPointer
    );
    assert!(wfst.is_null());
    assert_eq!(last_error(), "dictionary is null");

    assert_eq!(
        unsafe { duallity_wfst_new_ref(&raw, b"cat".as_ptr(), 3, 1, 0, 0, &mut wfst) },
        DuallityStatus::Ok
    );
    assert!(!wfst.is_null());
    unsafe { duallity_wfst_free(wfst) };
}

#[test]
fn invalid_utf8_query_reports_utf8_message() {
    let dictionary = unicode_dictionary();
    let source = dictionary.resource();
    let mut wfst = ptr::null_mut();
    let query = [0xffu8, 0xfe, 0xfd];
    let status = duallity_wfst_new(
        source.as_raw(),
        query.as_ptr(),
        query.len(),
        1,
        0,
        0,
        &mut wfst,
    );
    assert_eq!(status, DuallityStatus::InvalidUtf8);
    assert!(wfst.is_null());
    assert_eq!(last_error(), "query is not valid UTF-8");
}

#[test]
fn version_pins_match_header_constants() {
    // DUAL-VER-1: the runtime accessors equal the compile-time pins, which the
    // C header (include/duallity.h) mirrors as DUALLITY_ABI_VERSION / _REVISION.
    assert_eq!(duallity_abi_version(), DUALLITY_ABI_VERSION);
    assert_eq!(duallity_api_revision(), DUALLITY_API_REVISION);
    assert_eq!(duallity_abi_version(), 1);
    assert_eq!(duallity_api_revision(), 2);
}
