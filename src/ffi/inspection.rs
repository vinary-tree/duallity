//! Immutable, handle-owned configuration views for future revision-3 readback.
//!
//! Every wire pointer targets an independent boxed allocation, never the
//! address of a movable Rust field. The resource can outlive this view; its
//! pointers cannot outlive the opaque duallity handle that owns it.

use super::cache;
use super::config::{
    DuallityGeneralizedLimitsV1, DuallityOperationV1, DuallityRecordHeaderV1,
    DuallityRestrictionV1, DuallityWfstOptionsV1, ParsedOptions, CONFIG_RECORD_VERSION,
};
use super::{set_error, DuallityStatus};
use crate::bindings::WfstKind;
use crate::GeneralizedWfstLimits;
use liblevenshtein::transducer::{
    Algorithm, OperationApplicability, OperationSet, SubstitutionPair,
};
use lling_llang::bindings::OwnedWfstResource;
use std::mem::size_of;
use std::ptr;

/// Stable, immutable backing for every pointer returned by options inspection.
pub(super) struct OwnedOptions {
    kind: WfstKind,
    record: DuallityWfstOptionsV1,
    _limits: Option<Box<DuallityGeneralizedLimitsV1>>,
    _operations: Box<[DuallityOperationV1]>,
    _restrictions: Vec<Box<[DuallityRestrictionV1]>>,
    _text: Vec<Box<[u8]>>,
}

// SAFETY: Construction finishes all boxed allocations and raw-pointer wiring
// before publication. No method mutates their contents. Every raw pointer
// points into a private allocation owned until this handle is dropped; sharing
// immutable views across threads is safe under the C API's read-only contract.
unsafe impl Send for OwnedOptions {}
unsafe impl Sync for OwnedOptions {}

fn header<T>() -> DuallityRecordHeaderV1 {
    DuallityRecordHeaderV1 {
        struct_size: size_of::<T>() as u32,
        record_version: CONFIG_RECORD_VERSION,
        reserved: 0,
    }
}

fn wire(value: usize, name: &str) -> Result<u64, DuallityStatus> {
    u64::try_from(value).map_err(|_| {
        set_error(format!("{name} exceeds the revision-3 wire range"));
        DuallityStatus::LimitExceeded
    })
}

fn algorithm_value(algorithm: Algorithm) -> Result<u32, DuallityStatus> {
    match algorithm {
        Algorithm::Standard => Ok(0),
        Algorithm::Transposition => Ok(1),
        Algorithm::MergeAndSplit => Ok(2),
        Algorithm::DamerauLevenshtein => Ok(3),
        _ => {
            set_error("unrecognized edit algorithm cannot be inspected by revision 3");
            Err(DuallityStatus::InvalidArgument)
        }
    }
}

fn limits_record(
    value: GeneralizedWfstLimits,
) -> Result<DuallityGeneralizedLimitsV1, DuallityStatus> {
    Ok(DuallityGeneralizedLimitsV1 {
        header: header::<DuallityGeneralizedLimitsV1>(),
        max_query_bytes: wire(value.max_query_bytes, "max_query_bytes")?,
        max_query_scalars: wire(value.max_query_scalars, "max_query_scalars")?,
        max_operation_source_scalars: wire(
            value.max_operation_source_scalars,
            "max_operation_source_scalars",
        )?,
        max_operation_query_scalars: wire(
            value.max_operation_query_scalars,
            "max_operation_query_scalars",
        )?,
        max_retained_dictionary_nodes: wire(
            value.max_retained_dictionary_nodes,
            "max_retained_dictionary_nodes",
        )?,
        max_retained_wfst_states: wire(value.max_retained_wfst_states, "max_retained_wfst_states")?,
        max_paths_per_expansion: wire(value.max_paths_per_expansion, "max_paths_per_expansion")?,
        max_work_units_per_expansion: wire(
            value.max_work_units_per_expansion,
            "max_work_units_per_expansion",
        )?,
        reserved: [0; 2],
    })
}

fn own_bytes(text: &[u8], allocations: &mut Vec<Box<[u8]>>) -> *const u8 {
    let boxed = text.to_vec().into_boxed_slice();
    let pointer = boxed.as_ptr();
    allocations.push(boxed);
    pointer
}

impl OwnedOptions {
    /// Preserve the effective defaults of an existing revision-2 constructor.
    pub(super) fn legacy(
        kind: WfstKind,
        algorithm: Algorithm,
        maximum_distance: usize,
    ) -> Result<Self, DuallityStatus> {
        Self::build(kind, algorithm, maximum_distance, None, None)
    }

    /// Copy a parsed custom catalog into stable inspection allocations before
    /// its native operation set is moved into the WFST builder.
    pub(super) fn from_parsed(parsed: &ParsedOptions) -> Result<Self, DuallityStatus> {
        Self::build(
            parsed.kind,
            parsed.algorithm,
            parsed.maximum_distance,
            parsed.limits,
            parsed.operations.as_ref(),
        )
    }

    fn build(
        kind: WfstKind,
        algorithm: Algorithm,
        maximum_distance: usize,
        limits: Option<GeneralizedWfstLimits>,
        operations: Option<&OperationSet>,
    ) -> Result<Self, DuallityStatus> {
        let limits = limits.map(limits_record).transpose()?.map(Box::new);
        let mut texts = Vec::new();
        let mut restriction_arrays = Vec::new();
        let mut operation_records = Vec::new();
        if let Some(operations) = operations {
            for operation in operations.operations() {
                let name = operation.name().as_bytes();
                let name_data = own_bytes(name, &mut texts);
                let (applicability, restrictions, restriction_count, restriction_stride) =
                    match operation.applicability() {
                        OperationApplicability::Any => (0, ptr::null(), 0, 0),
                        OperationApplicability::Equal => (1, ptr::null(), 0, 0),
                        OperationApplicability::AdjacentTranspose => (2, ptr::null(), 0, 0),
                        OperationApplicability::Listed(set) => {
                            let pairs = set.pairs();
                            let mut records = Vec::with_capacity(pairs.len());
                            for pair in pairs {
                                let (source, target): (Vec<u8>, Vec<u8>) = match pair {
                                    SubstitutionPair::Bytes { source, target } => {
                                        (vec![source], vec![target])
                                    }
                                    SubstitutionPair::Strings { source, target } => {
                                        (source.as_bytes().to_vec(), target.as_bytes().to_vec())
                                    }
                                };
                                records.push(DuallityRestrictionV1 {
                                    header: header::<DuallityRestrictionV1>(),
                                    source_data: own_bytes(&source, &mut texts),
                                    source_len: wire(source.len(), "restriction source length")?,
                                    target_data: own_bytes(&target, &mut texts),
                                    target_len: wire(target.len(), "restriction target length")?,
                                    reserved: [0; 2],
                                });
                            }
                            let records = records.into_boxed_slice();
                            let pointer = records.as_ptr();
                            let count = wire(records.len(), "restriction count")?;
                            restriction_arrays.push(records);
                            (3, pointer, count, size_of::<DuallityRestrictionV1>() as u64)
                        }
                    };
                operation_records.push(DuallityOperationV1 {
                    header: header::<DuallityOperationV1>(),
                    consume_x: wire(operation.consume_x(), "consume_x")?,
                    consume_y: wire(operation.consume_y(), "consume_y")?,
                    weight: operation.weight(),
                    applicability,
                    reserved_zero: 0,
                    name_data,
                    name_len: wire(name.len(), "operation name length")?,
                    restrictions,
                    restriction_count,
                    restriction_stride,
                    reserved: [0; 2],
                });
            }
        }
        let operation_records = operation_records.into_boxed_slice();
        let operation_count = wire(operation_records.len(), "operation count")?;
        let record = DuallityWfstOptionsV1 {
            header: header::<DuallityWfstOptionsV1>(),
            kind: kind as u32,
            algorithm: if kind == WfstKind::Levenshtein {
                algorithm_value(algorithm)?
            } else {
                0
            },
            maximum_distance: if kind == WfstKind::Fzf {
                0
            } else {
                wire(maximum_distance, "maximum distance")?
            },
            cache_policy: 0,
            reserved_zero: 0,
            cache_capacity: 0,
            limits: limits.as_deref().map_or(ptr::null(), ptr::from_ref),
            operations: if operation_count == 0 {
                ptr::null()
            } else {
                operation_records.as_ptr()
            },
            operation_count,
            operation_stride: if operation_count == 0 {
                0
            } else {
                size_of::<DuallityOperationV1>() as u64
            },
            reserved: [0; 2],
        };
        Ok(Self {
            kind,
            record,
            _limits: limits,
            _operations: operation_records,
            _restrictions: restriction_arrays,
            _text: texts,
        })
    }

    pub(super) fn kind(&self) -> WfstKind {
        self.kind
    }

    /// Copy the record while reading policy from the resource's cache owner.
    /// The pointers in this copy remain valid only while `self` is alive.
    pub(super) fn readback(
        &self,
        resource: &OwnedWfstResource,
    ) -> Result<DuallityWfstOptionsV1, DuallityStatus> {
        let mut record = self.record;
        let (policy, capacity) = cache::effective_policy(resource)?;
        record.cache_policy = policy;
        record.cache_capacity = capacity;
        Ok(record)
    }
}

#[cfg(test)]
mod tests {
    use super::super::config::{parse_options, DuallityRestrictionV1};
    use super::super::{duallity_wfst_free, duallity_wfst_new, DuallityWfst};
    use super::*;
    use libdictenstein::bindings::{BindingUnitDomain, DynamicDawgBinding};
    use std::slice;

    #[test]
    fn caller_buffers_do_not_back_handle_inspection_and_resource_outlives_handle() {
        fn assert_thread_safe<T: Send + Sync>() {}
        assert_thread_safe::<OwnedOptions>();

        let mut name = b"ph-to-f".to_vec();
        let mut source_text = b"ph".to_vec();
        let mut target_text = b"f".to_vec();
        let pair = DuallityRestrictionV1 {
            header: header::<DuallityRestrictionV1>(),
            source_data: source_text.as_ptr(),
            source_len: source_text.len() as u64,
            target_data: target_text.as_ptr(),
            target_len: target_text.len() as u64,
            reserved: [0; 2],
        };
        let operation = DuallityOperationV1 {
            header: header::<DuallityOperationV1>(),
            consume_x: 2,
            consume_y: 1,
            weight: 0.5,
            applicability: 3,
            reserved_zero: 0,
            name_data: name.as_ptr(),
            name_len: name.len() as u64,
            restrictions: &pair,
            restriction_count: 1,
            restriction_stride: size_of::<DuallityRestrictionV1>() as u64,
            reserved: [0; 2],
        };
        let native_limits = GeneralizedWfstLimits {
            max_query_bytes: 5,
            ..GeneralizedWfstLimits::default()
        };
        let limits = Box::new(limits_record(native_limits).unwrap());
        let raw = DuallityWfstOptionsV1 {
            header: header::<DuallityWfstOptionsV1>(),
            kind: WfstKind::GeneralizedPhonetic as u32,
            algorithm: 0,
            maximum_distance: 1,
            cache_policy: 2,
            reserved_zero: 0,
            cache_capacity: 2,
            limits: limits.as_ref(),
            operations: &operation,
            operation_count: 1,
            operation_stride: size_of::<DuallityOperationV1>() as u64,
            reserved: [0; 2],
        };
        let parsed = unsafe { parse_options(&raw) }.unwrap();
        let options = OwnedOptions::from_parsed(&parsed).unwrap();
        let dictionary = DynamicDawgBinding::new(BindingUnitDomain::UnicodeScalar);
        dictionary.insert_text(b"phone", None).unwrap();
        let source = dictionary.resource();
        let resource = unsafe {
            crate::bindings::create_wfst_configured(
                source.as_raw(),
                "fone",
                parsed.into_construction(),
            )
        }
        .unwrap();
        let handle = Box::new(DuallityWfst {
            resource,
            options: Some(options),
            #[cfg(feature = "native-bindings-full")]
            fzf_config: None,
        });
        let retained = handle.resource.clone();
        name.fill(b'x');
        source_text.fill(b'x');
        target_text.fill(b'x');
        drop(name);
        drop(source_text);
        drop(target_text);
        drop(limits);
        drop(source);
        drop(dictionary);

        let readback = handle.inspect_options().unwrap();
        assert_eq!((readback.kind, readback.maximum_distance), (7, 1));
        assert_eq!((readback.cache_policy, readback.cache_capacity), (2, 2));
        assert_eq!(readback.operation_count, 1);
        assert_eq!(unsafe { (*readback.limits).max_query_bytes }, 5);
        let inspected = unsafe { &*readback.operations };
        assert_eq!(inspected.applicability, 3);
        assert_eq!(
            unsafe { slice::from_raw_parts(inspected.name_data, inspected.name_len as usize) },
            b"ph-to-f"
        );
        let listed = unsafe { &*inspected.restrictions };
        assert_eq!(
            unsafe { slice::from_raw_parts(listed.source_data, listed.source_len as usize) },
            b"ph"
        );
        assert_eq!(
            unsafe { slice::from_raw_parts(listed.target_data, listed.target_len as usize) },
            b"f"
        );

        cache::set_policy(
            &retained,
            super::super::config::RequestedCachePolicy::NoCache,
            WfstKind::GeneralizedPhonetic,
        )
        .unwrap();
        let changed = handle.inspect_options().unwrap();
        assert_eq!((changed.cache_policy, changed.cache_capacity), (1, 0));
        assert_eq!(
            (changed.operations, changed.limits),
            (readback.operations, readback.limits)
        );
        drop(handle);
        assert_eq!(cache::effective_policy(&retained).unwrap(), (1, 0));
        cache::clear(&retained).unwrap();
    }

    #[test]
    fn legacy_handles_report_effective_ignored_fields_and_no_custom_catalog() {
        let dictionary = DynamicDawgBinding::new(BindingUnitDomain::UnicodeScalar);
        dictionary.insert_text(b"cat", None).unwrap();
        let source = dictionary.resource();
        for (kind, requested_distance, expected_distance) in
            [(WfstKind::Fzf, 37, 0), (WfstKind::UniversalStandard, 2, 2)]
        {
            let mut handle = ptr::null_mut();
            assert_eq!(
                duallity_wfst_new(
                    source.as_raw(),
                    b"cat".as_ptr(),
                    3,
                    requested_distance,
                    1,
                    kind as u32,
                    &mut handle,
                ),
                DuallityStatus::Ok,
            );
            let readback = unsafe { (*handle).inspect_options().unwrap() };
            assert_eq!((readback.kind, readback.algorithm), (kind as u32, 0));
            assert_eq!(readback.maximum_distance, expected_distance);
            assert_eq!((readback.cache_policy, readback.cache_capacity), (0, 0));
            assert!(readback.limits.is_null());
            assert!(readback.operations.is_null());
            assert_eq!(
                (readback.operation_count, readback.operation_stride),
                (0, 0)
            );
            unsafe { duallity_wfst_free(handle) };
        }
    }

    #[test]
    fn ascii_fast_path_pairs_inspect_as_utf8_and_duplicate_pairs_coalesce() {
        let source = b"a";
        let target = b"b";
        let pair = DuallityRestrictionV1 {
            header: header::<DuallityRestrictionV1>(),
            source_data: source.as_ptr(),
            source_len: 1,
            target_data: target.as_ptr(),
            target_len: 1,
            reserved: [0; 2],
        };
        let pairs = [pair, pair];
        let name = b"ascii-pair";
        let operation = DuallityOperationV1 {
            header: header::<DuallityOperationV1>(),
            consume_x: 1,
            consume_y: 1,
            weight: 0.5,
            applicability: 3,
            reserved_zero: 0,
            name_data: name.as_ptr(),
            name_len: name.len() as u64,
            restrictions: pairs.as_ptr(),
            restriction_count: 2,
            restriction_stride: size_of::<DuallityRestrictionV1>() as u64,
            reserved: [0; 2],
        };
        let raw = DuallityWfstOptionsV1 {
            header: header::<DuallityWfstOptionsV1>(),
            kind: WfstKind::GeneralizedStandard as u32,
            algorithm: 0,
            maximum_distance: 1,
            cache_policy: 0,
            reserved_zero: 0,
            cache_capacity: 0,
            limits: ptr::null(),
            operations: &operation,
            operation_count: 1,
            operation_stride: size_of::<DuallityOperationV1>() as u64,
            reserved: [0; 2],
        };
        let parsed = unsafe { parse_options(&raw) }.unwrap();
        let view = OwnedOptions::from_parsed(&parsed).unwrap();
        let inspected = unsafe { &*view.record.operations };
        assert_eq!(inspected.restriction_count, 1);
        let listed = unsafe { &*inspected.restrictions };
        assert_eq!(
            unsafe { slice::from_raw_parts(listed.source_data, 1) },
            b"a"
        );
        assert_eq!(
            unsafe { slice::from_raw_parts(listed.target_data, 1) },
            b"b"
        );
    }

    #[test]
    fn null_output_path_remains_reusable_after_temporary_options_drop() {
        let dictionary = DynamicDawgBinding::new(BindingUnitDomain::UnicodeScalar);
        dictionary.insert_text(b"cat", None).unwrap();
        let source = dictionary.resource();
        for _ in 0..32 {
            assert_eq!(
                duallity_wfst_new(
                    source.as_raw(),
                    b"cat".as_ptr(),
                    3,
                    1,
                    0,
                    WfstKind::GeneralizedStandard as u32,
                    ptr::null_mut(),
                ),
                DuallityStatus::NullPointer,
            );
        }
        let mut handle = ptr::null_mut();
        assert_eq!(
            duallity_wfst_new(
                source.as_raw(),
                b"cat".as_ptr(),
                3,
                1,
                0,
                WfstKind::GeneralizedStandard as u32,
                &mut handle,
            ),
            DuallityStatus::Ok,
        );
        assert!(unsafe { (*handle).inspect_options().is_ok() });
        unsafe { duallity_wfst_free(handle) };
    }
}
