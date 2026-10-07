//! Owned validation for the revision-3 configurable WFST C ABI.
//!
//! Record layouts are public in the C header. Parsing deep-copies every
//! borrowed custom operation before a dictionary snapshot is captured.

use super::{algorithm, kind, set_error, DuallityStatus};
use crate::bindings::{WfstConstruction, WfstKind};
use crate::GeneralizedWfstLimits;
use liblevenshtein::transducer::{
    Algorithm, OperationApplicability, OperationSet, OperationType, SubstitutionSet,
};
use lling_llang::wfst::SharedCachePolicy;
use std::mem::{align_of, size_of};
use std::num::NonZeroUsize;
use std::slice;
use std::str;

pub const CONFIG_RECORD_VERSION: u32 = 1;
pub const CONFIG_MAX_RECORD_BYTES: usize = 4_096;
pub const CONFIG_MAX_OPERATIONS: usize = 4_096;
pub const CONFIG_MAX_RESTRICTION_PAIRS: usize = 4_096;
pub const CONFIG_MAX_CUSTOM_TEXT_BYTES: usize = 1_048_576;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DuallityRecordHeaderV1 {
    pub struct_size: u32,
    pub record_version: u32,
    pub reserved: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DuallityRestrictionV1 {
    pub header: DuallityRecordHeaderV1,
    pub source_data: *const u8,
    pub source_len: u64,
    pub target_data: *const u8,
    pub target_len: u64,
    pub reserved: [u64; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DuallityOperationV1 {
    pub header: DuallityRecordHeaderV1,
    pub consume_x: u64,
    pub consume_y: u64,
    pub weight: f64,
    pub applicability: u32,
    pub reserved_zero: u32,
    pub name_data: *const u8,
    pub name_len: u64,
    pub restrictions: *const DuallityRestrictionV1,
    pub restriction_count: u64,
    pub restriction_stride: u64,
    pub reserved: [u64; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DuallityGeneralizedLimitsV1 {
    pub header: DuallityRecordHeaderV1,
    pub max_query_bytes: u64,
    pub max_query_scalars: u64,
    pub max_operation_source_scalars: u64,
    pub max_operation_query_scalars: u64,
    pub max_retained_dictionary_nodes: u64,
    pub max_retained_wfst_states: u64,
    pub max_paths_per_expansion: u64,
    pub max_work_units_per_expansion: u64,
    pub reserved: [u64; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DuallityWfstOptionsV1 {
    pub header: DuallityRecordHeaderV1,
    pub kind: u32,
    pub algorithm: u32,
    pub maximum_distance: u64,
    pub cache_policy: u32,
    pub reserved_zero: u32,
    pub cache_capacity: u64,
    pub limits: *const DuallityGeneralizedLimitsV1,
    pub operations: *const DuallityOperationV1,
    pub operation_count: u64,
    pub operation_stride: u64,
    pub reserved: [u64; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DuallityCacheStatisticsV1 {
    pub header: DuallityRecordHeaderV1,
    pub hits: u64,
    pub misses: u64,
    pub faults: u64,
    pub uncacheable_results: u64,
    pub insertions: u64,
    pub evictions: u64,
    pub raced_publications: u64,
    pub clears: u64,
    pub resident_states: u64,
    pub recency_records: u64,
    pub reserved: [u64; 2],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RequestedCachePolicy {
    CacheAll,
    NoCache,
    Lru { capacity: usize },
}

impl RequestedCachePolicy {
    /// Resolve the legacy zero-capacity heuristic before constructing the
    /// single exported provider cache. It is not a second cache layer.
    pub(super) fn effective(self, kind: WfstKind) -> SharedCachePolicy {
        match self {
            Self::CacheAll => SharedCachePolicy::CacheAll,
            Self::NoCache => SharedCachePolicy::NoCache,
            Self::Lru { capacity: 0 } if kind == WfstKind::Fzf => SharedCachePolicy::NoCache,
            Self::Lru { capacity: 0 } => SharedCachePolicy::Lru {
                capacity: NonZeroUsize::new(100_000).expect("positive native cache default"),
            },
            Self::Lru { capacity } => SharedCachePolicy::Lru {
                capacity: NonZeroUsize::new(capacity).expect("nonzero capacity"),
            },
        }
    }
}

pub(super) struct ParsedOptions {
    pub kind: WfstKind,
    pub algorithm: Algorithm,
    pub maximum_distance: usize,
    pub cache_policy: RequestedCachePolicy,
    pub limits: Option<GeneralizedWfstLimits>,
    pub operations: Option<OperationSet>,
}

impl ParsedOptions {
    pub(super) fn into_construction(self) -> WfstConstruction {
        WfstConstruction {
            maximum_distance: self.maximum_distance,
            algorithm: self.algorithm,
            kind: self.kind,
            limits: self.limits,
            operations: self.operations,
            fzf_config: None,
            cache_policy: self.cache_policy.effective(self.kind),
        }
    }
}

fn reject(status: DuallityStatus, message: impl Into<String>) -> DuallityStatus {
    set_error(message);
    status
}

fn integer(value: u64, name: &str) -> Result<usize, DuallityStatus> {
    usize::try_from(value).map_err(|_| {
        reject(
            DuallityStatus::LimitExceeded,
            format!("{name} does not fit the host address space"),
        )
    })
}

/// Read a complete known record only after its first 16 bytes are validated.
///
/// # Safety
/// `pointer` must be null or point to aligned, readable storage of at least
/// the `struct_size` bytes advertised by its header. Foreign C pointers
/// cannot be proven mapped by the callee.
pub(super) unsafe fn record<T: Copy>(
    pointer: *const T,
    name: &str,
    available: usize,
) -> Result<T, DuallityStatus> {
    if pointer.is_null() {
        return Err(reject(
            DuallityStatus::NullPointer,
            format!("{name} is null"),
        ));
    }
    if !(pointer as usize).is_multiple_of(align_of::<T>()) {
        return Err(reject(
            DuallityStatus::InvalidArgument,
            format!("{name} is not aligned"),
        ));
    }
    let header = unsafe { pointer.cast::<DuallityRecordHeaderV1>().read() };
    let size = header.struct_size as usize;
    let known = size_of::<T>();
    if size < known || size > CONFIG_MAX_RECORD_BYTES || size > available {
        return Err(reject(
            DuallityStatus::InvalidArgument,
            format!("{name} has an unsupported size"),
        ));
    }
    if header.record_version != CONFIG_RECORD_VERSION || header.reserved != 0 {
        return Err(reject(
            DuallityStatus::InvalidArgument,
            format!("{name} has an unsupported version or reserved field"),
        ));
    }
    if size > known {
        let tail = unsafe { slice::from_raw_parts(pointer.cast::<u8>().add(known), size - known) };
        if tail.iter().any(|byte| *byte != 0) {
            return Err(reject(
                DuallityStatus::InvalidArgument,
                format!("{name} has nonzero unknown trailing bytes"),
            ));
        }
    }
    Ok(unsafe { pointer.read() })
}

pub(super) fn array_shape<T>(
    pointer: *const T,
    count: u64,
    stride: u64,
    maximum: usize,
    name: &str,
) -> Result<usize, DuallityStatus> {
    if count == 0 {
        if !pointer.is_null() || stride != 0 {
            return Err(reject(
                DuallityStatus::InvalidArgument,
                format!("{name} has a noncanonical empty array"),
            ));
        }
        return Ok(0);
    }
    if pointer.is_null() {
        return Err(reject(
            DuallityStatus::NullPointer,
            format!("{name} is null with nonzero count"),
        ));
    }
    let count = integer(count, &format!("{name} count"))?;
    let stride = integer(stride, &format!("{name} stride"))?;
    if count > maximum {
        return Err(reject(
            DuallityStatus::LimitExceeded,
            format!("{name} count exceeds {maximum}"),
        ));
    }
    if stride < size_of::<T>()
        || stride > CONFIG_MAX_RECORD_BYTES
        || !stride.is_multiple_of(align_of::<T>())
        || !(pointer as usize).is_multiple_of(align_of::<T>())
    {
        return Err(reject(
            DuallityStatus::InvalidArgument,
            format!("{name} has an invalid stride or alignment"),
        ));
    }
    let Some(bytes) = count.checked_mul(stride) else {
        return Err(reject(
            DuallityStatus::LimitExceeded,
            format!("{name} byte extent overflows"),
        ));
    };
    if bytes > isize::MAX as usize {
        return Err(reject(
            DuallityStatus::LimitExceeded,
            format!("{name} byte extent exceeds isize::MAX"),
        ));
    }
    Ok(count)
}

/// # Safety
/// Non-null input must designate `length` readable bytes for this call.
unsafe fn utf8<'a>(
    pointer: *const u8,
    length: u64,
    name: &str,
    total_bytes: &mut usize,
) -> Result<&'a str, DuallityStatus> {
    let length = integer(length, &format!("{name} length"))?;
    if length == 0 {
        return Err(reject(
            DuallityStatus::InvalidArgument,
            format!("{name} is empty"),
        ));
    }
    if pointer.is_null() {
        return Err(reject(
            DuallityStatus::NullPointer,
            format!("{name} is null"),
        ));
    }
    *total_bytes = total_bytes.checked_add(length).ok_or_else(|| {
        reject(
            DuallityStatus::LimitExceeded,
            "custom text byte total overflows",
        )
    })?;
    if *total_bytes > CONFIG_MAX_CUSTOM_TEXT_BYTES {
        return Err(reject(
            DuallityStatus::LimitExceeded,
            "custom text byte total exceeds one MiB",
        ));
    }
    let bytes = unsafe { slice::from_raw_parts(pointer, length) };
    str::from_utf8(bytes).map_err(|_| {
        reject(
            DuallityStatus::InvalidUtf8,
            format!("{name} is not valid UTF-8"),
        )
    })
}

fn decode_limits(
    raw: DuallityGeneralizedLimitsV1,
) -> Result<GeneralizedWfstLimits, DuallityStatus> {
    if raw.reserved != [0; 2] {
        return Err(reject(
            DuallityStatus::InvalidArgument,
            "limits reserved fields are nonzero",
        ));
    }
    let limits = GeneralizedWfstLimits {
        max_query_bytes: integer(raw.max_query_bytes, "max_query_bytes")?,
        max_query_scalars: integer(raw.max_query_scalars, "max_query_scalars")?,
        max_operation_source_scalars: integer(
            raw.max_operation_source_scalars,
            "max_operation_source_scalars",
        )?,
        max_operation_query_scalars: integer(
            raw.max_operation_query_scalars,
            "max_operation_query_scalars",
        )?,
        max_retained_dictionary_nodes: integer(
            raw.max_retained_dictionary_nodes,
            "max_retained_dictionary_nodes",
        )?,
        max_retained_wfst_states: integer(
            raw.max_retained_wfst_states,
            "max_retained_wfst_states",
        )?,
        max_paths_per_expansion: integer(raw.max_paths_per_expansion, "max_paths_per_expansion")?,
        max_work_units_per_expansion: integer(
            raw.max_work_units_per_expansion,
            "max_work_units_per_expansion",
        )?,
    };
    if limits.max_retained_dictionary_nodes == 0 || limits.max_retained_wfst_states == 0 {
        return Err(reject(
            DuallityStatus::InvalidArgument,
            "retained node and state limits must be at least one",
        ));
    }
    Ok(limits)
}

pub(super) fn cache_policy(
    policy: u32,
    capacity: u64,
) -> Result<RequestedCachePolicy, DuallityStatus> {
    match policy {
        0 if capacity == 0 => Ok(RequestedCachePolicy::CacheAll),
        1 if capacity == 0 => Ok(RequestedCachePolicy::NoCache),
        2 => Ok(RequestedCachePolicy::Lru {
            capacity: integer(capacity, "cache capacity")?,
        }),
        0 | 1 => Err(reject(
            DuallityStatus::InvalidArgument,
            "non-LRU cache policy requires zero capacity",
        )),
        _ => Err(reject(
            DuallityStatus::InvalidArgument,
            "unknown cache policy",
        )),
    }
}

/// Decode borrowed C records into owned native data before dictionary capture.
///
/// # Safety
/// The options pointer and every non-null nested pointer must designate
/// readable, correctly aligned storage of its advertised extent for this
/// call. No pointer is retained after parsing.
pub(super) unsafe fn parse_options(
    pointer: *const DuallityWfstOptionsV1,
) -> Result<ParsedOptions, DuallityStatus> {
    let raw = unsafe { record(pointer, "options", CONFIG_MAX_RECORD_BYTES)? };
    if raw.reserved_zero != 0 || raw.reserved != [0; 2] {
        return Err(reject(
            DuallityStatus::InvalidArgument,
            "options reserved fields are nonzero",
        ));
    }
    let kind = kind(raw.kind)?;
    let algorithm = algorithm(raw.algorithm)?;
    let maximum_distance = integer(raw.maximum_distance, "maximum distance")?;
    let cache_policy = cache_policy(raw.cache_policy, raw.cache_capacity)?;
    let generalized = matches!(
        kind,
        WfstKind::GeneralizedStandard
            | WfstKind::GeneralizedTransposition
            | WfstKind::GeneralizedMergeAndSplit
            | WfstKind::GeneralizedPhonetic
    );
    if kind != WfstKind::Levenshtein && raw.algorithm != 0 {
        return Err(reject(
            DuallityStatus::InvalidArgument,
            "this WFST kind requires the standard algorithm sentinel",
        ));
    }
    if kind == WfstKind::Fzf && maximum_distance != 0 {
        return Err(reject(
            DuallityStatus::InvalidArgument,
            "FZF requires a zero maximum-distance sentinel",
        ));
    }
    if kind != WfstKind::Levenshtein && kind != WfstKind::Fzf && maximum_distance > u8::MAX as usize
    {
        return Err(reject(
            DuallityStatus::InvalidArgument,
            "universal/generalized maximum distance must fit u8",
        ));
    }
    if !generalized && (!raw.limits.is_null() || raw.operation_count != 0) {
        return Err(reject(
            DuallityStatus::InvalidArgument,
            "only generalized kinds accept limits or custom operations",
        ));
    }
    let limits = if raw.limits.is_null() {
        None
    } else {
        Some(decode_limits(unsafe {
            record(raw.limits, "limits", CONFIG_MAX_RECORD_BYTES)?
        })?)
    };
    let count = array_shape(
        raw.operations,
        raw.operation_count,
        raw.operation_stride,
        CONFIG_MAX_OPERATIONS,
        "operations",
    )?;
    let mut total_text_bytes = 0;
    let mut total_pairs = 0usize;
    let mut total_consumption = 0usize;
    let mut operations = OperationSet::with_capacity(count);
    for index in 0..count {
        let base = unsafe {
            raw.operations
                .cast::<u8>()
                .add(index * raw.operation_stride as usize)
        };
        let operation = unsafe {
            record(
                base.cast::<DuallityOperationV1>(),
                "operation",
                raw.operation_stride as usize,
            )?
        };
        if operation.reserved_zero != 0 || operation.reserved != [0; 2] {
            return Err(reject(
                DuallityStatus::InvalidArgument,
                format!("operation {index} has nonzero reserved fields"),
            ));
        }
        let consume_x = integer(operation.consume_x, "consume_x")?;
        let consume_y = integer(operation.consume_y, "consume_y")?;
        let consumed = consume_x
            .checked_add(consume_y)
            .ok_or_else(|| reject(DuallityStatus::LimitExceeded, "operation arity overflows"))?;
        total_consumption = total_consumption.checked_add(consumed).ok_or_else(|| {
            reject(
                DuallityStatus::LimitExceeded,
                "operation grammar width overflows",
            )
        })?;
        if total_consumption > CONFIG_MAX_OPERATIONS {
            return Err(reject(
                DuallityStatus::LimitExceeded,
                "operation grammar width exceeds 4096 scalars",
            ));
        }
        if consumed == 0 || !operation.weight.is_finite() || operation.weight < 0.0 {
            return Err(reject(
                DuallityStatus::InvalidArgument,
                format!("operation {index} has no progress or invalid weight"),
            ));
        }
        if operation.weight == 0.0 && consume_x != consume_y {
            return Err(reject(
                DuallityStatus::InvalidArgument,
                format!("operation {index} has a zero-cost length change"),
            ));
        }
        let name = unsafe {
            utf8(
                operation.name_data,
                operation.name_len,
                "operation name",
                &mut total_text_bytes,
            )?
        };
        if name.len() > 1_024 || name.as_bytes().contains(&0) {
            return Err(reject(
                DuallityStatus::InvalidArgument,
                format!("operation {index} name is too long or contains NUL"),
            ));
        }
        let pair_count = array_shape(
            operation.restrictions,
            operation.restriction_count,
            operation.restriction_stride,
            CONFIG_MAX_RESTRICTION_PAIRS,
            "restrictions",
        )?;
        total_pairs = total_pairs
            .checked_add(pair_count)
            .ok_or_else(|| reject(DuallityStatus::LimitExceeded, "restriction count overflows"))?;
        if total_pairs > CONFIG_MAX_RESTRICTION_PAIRS {
            return Err(reject(
                DuallityStatus::LimitExceeded,
                "restriction count exceeds 4096",
            ));
        }
        let applicability = match operation.applicability {
            0 if pair_count == 0 => OperationApplicability::Any,
            1 if pair_count == 0 && consume_x == consume_y => OperationApplicability::Equal,
            2 if pair_count == 0 && consume_x == 2 && consume_y == 2 => {
                OperationApplicability::AdjacentTranspose
            }
            3 if pair_count > 0 => {
                let mut set = SubstitutionSet::with_capacity(pair_count);
                for pair_index in 0..pair_count {
                    let base = unsafe {
                        operation
                            .restrictions
                            .cast::<u8>()
                            .add(pair_index * operation.restriction_stride as usize)
                    };
                    let pair = unsafe {
                        record(
                            base.cast::<DuallityRestrictionV1>(),
                            "restriction",
                            operation.restriction_stride as usize,
                        )?
                    };
                    if pair.reserved != [0; 2] {
                        return Err(reject(
                            DuallityStatus::InvalidArgument,
                            "restriction reserved fields are nonzero",
                        ));
                    }
                    let source = unsafe {
                        utf8(
                            pair.source_data,
                            pair.source_len,
                            "restriction source",
                            &mut total_text_bytes,
                        )?
                    };
                    let target = unsafe {
                        utf8(
                            pair.target_data,
                            pair.target_len,
                            "restriction target",
                            &mut total_text_bytes,
                        )?
                    };
                    if source.chars().count() != consume_x || target.chars().count() != consume_y {
                        return Err(reject(
                            DuallityStatus::InvalidArgument,
                            "restriction scalar lengths disagree with operation arity",
                        ));
                    }
                    set.allow_str(source, target);
                }
                OperationApplicability::Listed(set)
            }
            _ => {
                return Err(reject(
                    DuallityStatus::InvalidArgument,
                    format!("operation {index} has incompatible applicability/restrictions"),
                ));
            }
        };
        operations.add(OperationType::with_owned_applicability(
            consume_x,
            consume_y,
            operation.weight,
            applicability,
            name.to_owned(),
        ));
    }
    if count > 0 {
        operations.validate().map_err(|error| {
            reject(
                DuallityStatus::InvalidArgument,
                format!("invalid custom operation set: {error}"),
            )
        })?;
    }
    Ok(ParsedOptions {
        kind,
        algorithm,
        maximum_distance,
        cache_policy,
        limits,
        operations: (count > 0).then_some(operations),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use libdictenstein::bindings::{BindingUnitDomain, DynamicDawgBinding};
    use liblevenshtein::transducer::SubstitutionPair;
    use lling_llang::bindings::OwnedWfstResource;
    use std::collections::{HashSet, VecDeque};
    use std::ffi::c_void;
    use vinary_tree_interop::{
        VtStatus, VtWfstArc, VtWfstVTable, VT_WFST_INTERFACE_ID, VT_WFST_INTERFACE_VERSION,
    };

    type ArcSignature = (u64, u64, u64, u64, u8, u8);
    type StateSignature = (u64, u8, u64, Vec<ArcSignature>);

    fn header<T>() -> DuallityRecordHeaderV1 {
        DuallityRecordHeaderV1 {
            struct_size: size_of::<T>() as u32,
            record_version: CONFIG_RECORD_VERSION,
            reserved: 0,
        }
    }

    fn options(kind: WfstKind) -> DuallityWfstOptionsV1 {
        DuallityWfstOptionsV1 {
            header: header::<DuallityWfstOptionsV1>(),
            kind: kind as u32,
            algorithm: 0,
            maximum_distance: 1,
            cache_policy: 0,
            reserved_zero: 0,
            cache_capacity: 0,
            limits: std::ptr::null(),
            operations: std::ptr::null(),
            operation_count: 0,
            operation_stride: 0,
            reserved: [0; 2],
        }
    }

    fn operation(name: &[u8]) -> DuallityOperationV1 {
        DuallityOperationV1 {
            header: header::<DuallityOperationV1>(),
            consume_x: 1,
            consume_y: 1,
            weight: 1.0,
            applicability: 0,
            reserved_zero: 0,
            name_data: name.as_ptr(),
            name_len: name.len() as u64,
            restrictions: std::ptr::null(),
            restriction_count: 0,
            restriction_stride: 0,
            reserved: [0; 2],
        }
    }

    fn limits_record(value: GeneralizedWfstLimits) -> DuallityGeneralizedLimitsV1 {
        DuallityGeneralizedLimitsV1 {
            header: header::<DuallityGeneralizedLimitsV1>(),
            max_query_bytes: value.max_query_bytes as u64,
            max_query_scalars: value.max_query_scalars as u64,
            max_operation_source_scalars: value.max_operation_source_scalars as u64,
            max_operation_query_scalars: value.max_operation_query_scalars as u64,
            max_retained_dictionary_nodes: value.max_retained_dictionary_nodes as u64,
            max_retained_wfst_states: value.max_retained_wfst_states as u64,
            max_paths_per_expansion: value.max_paths_per_expansion as u64,
            max_work_units_per_expansion: value.max_work_units_per_expansion as u64,
            reserved: [0; 2],
        }
    }

    fn graph(resource: &OwnedWfstResource) -> Vec<StateSignature> {
        unsafe {
            let raw = resource.as_raw();
            let mut interface: *const c_void = std::ptr::null();
            assert_eq!(
                (*raw.vtable).query_interface.unwrap()(
                    raw.context,
                    &VT_WFST_INTERFACE_ID,
                    VT_WFST_INTERFACE_VERSION,
                    &mut interface,
                ),
                VtStatus::Ok.to_raw(),
            );
            let table = &*interface.cast::<VtWfstVTable>();
            let mut start = 0;
            assert_eq!(
                table.start.unwrap()(raw.context, &mut start),
                VtStatus::Ok.to_raw(),
            );
            let mut queue = VecDeque::from([start]);
            let mut seen = HashSet::new();
            let mut states = Vec::new();
            while let Some(state) = queue.pop_front() {
                if !seen.insert(state) {
                    continue;
                }
                assert!(seen.len() < 1_000);
                let (mut valid, mut final_state, mut final_weight) = (0, 0, 0.0);
                assert_eq!(
                    table.state_info.unwrap()(
                        raw.context,
                        state,
                        &mut valid,
                        &mut final_state,
                        &mut final_weight,
                    ),
                    VtStatus::Ok.to_raw(),
                );
                assert_eq!(valid, 1);
                let mut arcs = [VtWfstArc::default(); 128];
                let (mut written, mut total) = (0, 0);
                assert_eq!(
                    table.state_arcs.unwrap()(
                        raw.context,
                        state,
                        0,
                        arcs.as_mut_ptr(),
                        arcs.len(),
                        &mut written,
                        &mut total,
                    ),
                    VtStatus::Ok.to_raw(),
                );
                assert_eq!(written, total);
                let edges = arcs[..written]
                    .iter()
                    .map(|arc| {
                        queue.push_back(arc.target_state);
                        (
                            arc.input_label,
                            arc.output_label,
                            arc.target_state,
                            arc.weight.to_bits(),
                            arc.has_input,
                            arc.has_output,
                        )
                    })
                    .collect();
                states.push((state, final_state, final_weight.to_bits(), edges));
            }
            states.sort_by_key(|state| state.0);
            states
        }
    }

    #[test]
    fn parser_owns_custom_text_before_caller_mutation() {
        let mut name = b"caller-owned".to_vec();
        let op = operation(&name);
        let mut raw = options(WfstKind::GeneralizedStandard);
        raw.operations = &op;
        raw.operation_count = 1;
        raw.operation_stride = size_of::<DuallityOperationV1>() as u64;
        let parsed = unsafe { parse_options(&raw) }.unwrap();
        name.fill(b'x');
        assert_eq!(
            parsed.operations.unwrap().operations()[0].name(),
            "caller-owned"
        );
    }

    #[test]
    fn parser_owns_listed_restriction_strings_before_caller_mutation() {
        let mut source = b"ph".to_vec();
        let mut target = b"f".to_vec();
        let pair = DuallityRestrictionV1 {
            header: header::<DuallityRestrictionV1>(),
            source_data: source.as_ptr(),
            source_len: source.len() as u64,
            target_data: target.as_ptr(),
            target_len: target.len() as u64,
            reserved: [0; 2],
        };
        let mut op = operation(b"ph-to-f");
        op.consume_x = 2;
        op.applicability = 3;
        op.restrictions = &pair;
        op.restriction_count = 1;
        op.restriction_stride = size_of::<DuallityRestrictionV1>() as u64;
        let mut raw = options(WfstKind::GeneralizedPhonetic);
        raw.operations = &op;
        raw.operation_count = 1;
        raw.operation_stride = size_of::<DuallityOperationV1>() as u64;
        let parsed = unsafe { parse_options(&raw) }.unwrap();
        source.fill(b'x');
        target.fill(b'y');
        let operations = parsed.operations.unwrap();
        let OperationApplicability::Listed(listed) = operations.operations()[0].applicability()
        else {
            panic!("listed applicability was not retained");
        };
        assert_eq!(
            listed.pairs(),
            vec![SubstitutionPair::Strings {
                source: "ph".into(),
                target: "f".into(),
            }]
        );
    }

    #[test]
    fn parser_rejects_records_that_overrun_array_stride() {
        let name = b"substitute";
        let mut op = operation(name);
        op.header.struct_size += 8;
        let mut raw = options(WfstKind::GeneralizedStandard);
        raw.operations = &op;
        raw.operation_count = 1;
        raw.operation_stride = size_of::<DuallityOperationV1>() as u64;
        assert!(matches!(
            unsafe { parse_options(&raw) },
            Err(DuallityStatus::InvalidArgument)
        ));
    }

    #[test]
    fn parser_rejects_invalid_restriction_and_noncanonical_empty_array() {
        let name = b"listed";
        let source = b"";
        let target = b"b";
        let pair = DuallityRestrictionV1 {
            header: header::<DuallityRestrictionV1>(),
            source_data: source.as_ptr(),
            source_len: 0,
            target_data: target.as_ptr(),
            target_len: 1,
            reserved: [0; 2],
        };
        let mut op = operation(name);
        op.applicability = 3;
        op.restrictions = &pair;
        op.restriction_count = 1;
        op.restriction_stride = size_of::<DuallityRestrictionV1>() as u64;
        let mut raw = options(WfstKind::GeneralizedStandard);
        raw.operations = &op;
        raw.operation_count = 1;
        raw.operation_stride = size_of::<DuallityOperationV1>() as u64;
        assert!(matches!(
            unsafe { parse_options(&raw) },
            Err(DuallityStatus::InvalidArgument)
        ));

        raw.operation_count = 0;
        assert!(matches!(
            unsafe { parse_options(&raw) },
            Err(DuallityStatus::InvalidArgument)
        ));
    }

    #[test]
    fn parser_rejects_unknown_fields_and_unsupported_combinations() {
        let mut raw = options(WfstKind::Fzf);
        assert!(matches!(
            unsafe { parse_options(&raw) },
            Err(DuallityStatus::InvalidArgument)
        ));
        raw.maximum_distance = 0;
        assert!(unsafe { parse_options(&raw) }.is_ok());

        raw = options(WfstKind::Levenshtein);
        raw.algorithm = 4;
        assert!(matches!(
            unsafe { parse_options(&raw) },
            Err(DuallityStatus::InvalidArgument)
        ));
        raw.algorithm = 0;
        raw.cache_policy = 1;
        raw.cache_capacity = 1;
        assert!(matches!(
            unsafe { parse_options(&raw) },
            Err(DuallityStatus::InvalidArgument)
        ));
        raw.cache_policy = 0;
        raw.cache_capacity = 0;
        raw.header.reserved = 1;
        assert!(matches!(
            unsafe { parse_options(&raw) },
            Err(DuallityStatus::InvalidArgument)
        ));
        raw.header.reserved = 0;
        raw.header.record_version += 1;
        assert!(matches!(
            unsafe { parse_options(&raw) },
            Err(DuallityStatus::InvalidArgument)
        ));
    }

    #[test]
    fn parser_accepts_zero_forward_tail_and_rejects_nonzero_tail() {
        #[repr(C)]
        struct Extended {
            base: DuallityWfstOptionsV1,
            tail: [u8; 8],
        }
        let mut extended = Extended {
            base: options(WfstKind::Levenshtein),
            tail: [0; 8],
        };
        extended.base.header.struct_size = size_of::<Extended>() as u32;
        let pointer = (&raw const extended).cast::<DuallityWfstOptionsV1>();
        assert!(unsafe { parse_options(pointer) }.is_ok());
        extended.tail[0] = 1;
        let pointer = (&raw const extended).cast::<DuallityWfstOptionsV1>();
        assert!(matches!(
            unsafe { parse_options(pointer) },
            Err(DuallityStatus::InvalidArgument)
        ));
    }

    #[test]
    fn parser_limits_catalog_before_reading_entries() {
        let name = b"unused";
        let op = operation(name);
        let mut raw = options(WfstKind::GeneralizedStandard);
        raw.operations = &op;
        raw.operation_count = (CONFIG_MAX_OPERATIONS + 1) as u64;
        raw.operation_stride = size_of::<DuallityOperationV1>() as u64;
        assert!(matches!(
            unsafe { parse_options(&raw) },
            Err(DuallityStatus::LimitExceeded)
        ));
    }

    #[test]
    fn parser_rejects_null_invalid_utf8_and_impossible_limits() {
        assert!(matches!(
            unsafe { parse_options(std::ptr::null()) },
            Err(DuallityStatus::NullPointer)
        ));
        let name = [0xff];
        let mut op = operation(&name);
        let mut raw = options(WfstKind::GeneralizedStandard);
        raw.operations = &op;
        raw.operation_count = 1;
        raw.operation_stride = size_of::<DuallityOperationV1>() as u64;
        assert!(matches!(
            unsafe { parse_options(&raw) },
            Err(DuallityStatus::InvalidUtf8)
        ));
        op.name_data = std::ptr::null();
        raw.operations = &op;
        assert!(matches!(
            unsafe { parse_options(&raw) },
            Err(DuallityStatus::NullPointer)
        ));

        raw.operations = std::ptr::null();
        raw.operation_count = 0;
        raw.operation_stride = 0;
        let mut limits = limits_record(GeneralizedWfstLimits::default());
        limits.max_retained_wfst_states = 0;
        raw.limits = &limits;
        assert!(matches!(
            unsafe { parse_options(&raw) },
            Err(DuallityStatus::InvalidArgument)
        ));
    }

    #[test]
    fn configured_builder_applies_owned_operations_and_query_limit() {
        let name = b"one-cost-substitution";
        let op = operation(name);
        let mut raw = options(WfstKind::GeneralizedStandard);
        raw.operations = &op;
        raw.operation_count = 1;
        raw.operation_stride = size_of::<DuallityOperationV1>() as u64;
        let parsed = unsafe { parse_options(&raw) }.unwrap();
        let dictionary = DynamicDawgBinding::new(BindingUnitDomain::UnicodeScalar);
        dictionary.insert_text(b"cat", None).unwrap();
        let source = dictionary.resource();
        let result = unsafe {
            crate::bindings::create_wfst_configured(
                source.as_raw(),
                "cat",
                parsed.into_construction(),
            )
        };
        assert!(result.is_ok(), "custom operation construction failed");

        let limits = GeneralizedWfstLimits {
            max_query_bytes: 2,
            ..GeneralizedWfstLimits::default()
        };
        let wire_limits = limits_record(limits);
        raw.limits = &wire_limits;
        let parsed = unsafe { parse_options(&raw) }.unwrap();
        let result = unsafe {
            crate::bindings::create_wfst_configured(
                source.as_raw(),
                "cat",
                parsed.into_construction(),
            )
        };
        assert!(matches!(
            result,
            Err(crate::bindings::BindingError::Generalized(
                crate::GeneralizedWfstError::LimitExceeded { .. }
            ))
        ));
    }

    #[test]
    fn parsed_standard_grammar_matches_legacy_and_retains_snapshot() {
        let names: [&[u8]; 4] = [b"match", b"substitute", b"insert", b"delete"];
        let mut ops = names.map(operation);
        ops[0].weight = 0.0;
        ops[0].applicability = 1;
        ops[2].consume_x = 0;
        ops[3].consume_y = 0;
        let mut raw = options(WfstKind::GeneralizedStandard);
        raw.operations = ops.as_ptr();
        raw.operation_count = ops.len() as u64;
        raw.operation_stride = size_of::<DuallityOperationV1>() as u64;
        let parsed = unsafe { parse_options(&raw) }.unwrap();

        let dictionary = DynamicDawgBinding::new(BindingUnitDomain::UnicodeScalar);
        for term in [b"cat", b"bat", b"cot"] {
            dictionary.insert_text(term, None).unwrap();
        }
        let source = dictionary.resource();
        let configured = unsafe {
            crate::bindings::create_wfst_configured(
                source.as_raw(),
                "cat",
                parsed.into_construction(),
            )
        }
        .unwrap();
        let legacy = unsafe {
            crate::bindings::create_wfst(
                source.as_raw(),
                "cat",
                1,
                Algorithm::Standard,
                WfstKind::GeneralizedStandard,
            )
        }
        .unwrap();
        dictionary.clear();
        drop(source);
        drop(dictionary);

        let configured_graph = graph(&configured);
        assert!(configured_graph.iter().any(|state| state.1 == 1));
        assert_eq!(configured_graph, graph(&legacy));
    }
}
