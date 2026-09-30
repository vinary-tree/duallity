//! Additive phonetic WFST constructors. Every output is an owned
//! `vt.scalar-wfst.1` resource; no Rust or Julia object crosses the ABI.

use super::{boundary, config, map_error, output, query, set_error, DuallityStatus};
use crate::{CommonPhoneticRules, RewriteRule};
use std::mem::{align_of, size_of};
use std::ptr;
use std::slice;
use vinary_tree_interop::VtResource;

const MAX_TEXT_BYTES: usize = 1_048_576;
const MAX_RULES: usize = 4_096;

/// One borrowed, versioned unconditional phonetic rewrite rule.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DuallityPhoneticRuleV1 {
    pub header: config::DuallityRecordHeaderV1,
    pub input_data: *const u8,
    pub input_len: usize,
    pub output_data: *const u8,
    pub output_len: usize,
    pub cost: f64,
    pub priority: i32,
    pub reserved: u32,
}

fn bounded_text<'a>(data: *const u8, len: usize, name: &str) -> Result<&'a str, DuallityStatus> {
    if len > MAX_TEXT_BYTES {
        set_error(format!("{name} exceeds {MAX_TEXT_BYTES} bytes"));
        return Err(DuallityStatus::LimitExceeded);
    }
    query(data, len)
}

fn cache_policy(
    policy: u32,
    capacity: u64,
) -> Result<lling_llang::wfst::SharedCachePolicy, DuallityStatus> {
    Ok(config::cache_policy(policy, capacity)?.effective(crate::bindings::WfstKind::Levenshtein))
}

fn owned_output<'a>(out_resource: *mut VtResource) -> Result<&'a mut VtResource, DuallityStatus> {
    let slot = output(out_resource, "out_resource")?;
    *slot = VtResource::NULL;
    Ok(slot)
}

/// Compile a pattern NFA into an independently owned, lazy WFST.
///
/// `alphabet_data` defines a finite Unicode-scalar alphabet for wide regex
/// labels. Literal labels remain exact even when absent from the alphabet.
/// The caller retains ownership of both UTF-8 buffers; output owns one resource
/// retain and must be released via its Vinary Tree resource vtable.
///
/// # Safety
/// Input buffers and output pointer must be readable/writable for their lengths.
#[no_mangle]
pub unsafe extern "C" fn duallity_phonetic_nfa_new(
    pattern_data: *const u8,
    pattern_len: usize,
    alphabet_data: *const u8,
    alphabet_len: usize,
    phonetic_weight: f64,
    policy: u32,
    capacity: u64,
    out_resource: *mut VtResource,
) -> DuallityStatus {
    boundary(|| {
        let slot = owned_output(out_resource)?;
        let pattern = bounded_text(pattern_data, pattern_len, "pattern")?;
        let alphabet = if alphabet_data.is_null() && alphabet_len == 0 {
            None
        } else {
            Some(bounded_text(alphabet_data, alphabet_len, "alphabet")?)
        };
        let cache_policy = cache_policy(policy, capacity)?;
        let resource =
            crate::bindings::create_phonetic_nfa(pattern, alphabet, phonetic_weight, cache_policy)
                .map_err(map_error)?;
        *slot = resource.into_raw();
        Ok(())
    })
}

/// Capture one Unicode dictionary revision and construct a phonetic NFA ×
/// edit-distance × dictionary product WFST. The result survives release or
/// mutation of the source dictionary.
///
/// # Safety
/// `dictionary` points to a live borrowed `VtResource`; buffers and output
/// pointer must be valid for the duration of this call.
#[no_mangle]
pub unsafe extern "C" fn duallity_phonetic_product_new_ref(
    dictionary: *const VtResource,
    pattern_data: *const u8,
    pattern_len: usize,
    maximum_distance: u32,
    phonetic_weight: f64,
    edit_weight: f64,
    policy: u32,
    capacity: u64,
    out_resource: *mut VtResource,
) -> DuallityStatus {
    boundary(|| {
        let slot = owned_output(out_resource)?;
        if dictionary.is_null() {
            set_error("dictionary is null");
            return Err(DuallityStatus::NullPointer);
        }
        if !(dictionary as usize).is_multiple_of(align_of::<VtResource>()) {
            set_error("dictionary is not aligned");
            return Err(DuallityStatus::InvalidArgument);
        }
        let pattern = bounded_text(pattern_data, pattern_len, "pattern")?;
        let distance = u8::try_from(maximum_distance).map_err(|_| {
            set_error("maximum_distance must fit UInt8");
            DuallityStatus::LimitExceeded
        })?;
        let cache_policy = cache_policy(policy, capacity)?;
        let resource = unsafe {
            crate::bindings::create_phonetic_product(
                ptr::read(dictionary),
                pattern,
                distance,
                phonetic_weight,
                edit_weight,
                cache_policy,
            )
        }
        .map_err(map_error)?;
        *slot = resource.into_raw();
        Ok(())
    })
}

/// Build a standalone, priority-ordered rewrite graph from borrowed records.
/// Rule strings are copied during this call. Empty rules are validated by the
/// native rewrite constructor, and caller buffers may be released afterward.
///
/// # Safety
/// `rules` designates `rule_count` readable records; each nested buffer is
/// readable for its stated length; `out_resource` is writable.
#[no_mangle]
pub unsafe extern "C" fn duallity_phonetic_rewrite_new(
    rules: *const DuallityPhoneticRuleV1,
    rule_count: usize,
    allow_identity: u8,
    policy: u32,
    capacity: u64,
    out_resource: *mut VtResource,
) -> DuallityStatus {
    boundary(|| {
        let slot = owned_output(out_resource)?;
        if rule_count > MAX_RULES {
            set_error(format!("rule count exceeds {MAX_RULES}"));
            return Err(DuallityStatus::LimitExceeded);
        }
        if allow_identity > 1 {
            set_error("allow_identity must be zero or one");
            return Err(DuallityStatus::InvalidArgument);
        }
        if rule_count > 0 && rules.is_null() {
            set_error("rules is null");
            return Err(DuallityStatus::NullPointer);
        }
        if !(rules as usize).is_multiple_of(align_of::<DuallityPhoneticRuleV1>()) {
            set_error("rules is not aligned");
            return Err(DuallityStatus::InvalidArgument);
        }
        let cache_policy = cache_policy(policy, capacity)?;
        let input = if rule_count == 0 {
            &[][..]
        } else {
            unsafe { slice::from_raw_parts(rules, rule_count) }
        };
        let mut owned = Vec::with_capacity(rule_count);
        for (index, rule) in input.iter().enumerate() {
            if rule.header.struct_size as usize != size_of::<DuallityPhoneticRuleV1>()
                || rule.header.record_version != 1
                || rule.header.reserved != 0
                || rule.reserved != 0
            {
                set_error(format!(
                    "rule {index} has unsupported size, version, or reserved field"
                ));
                return Err(DuallityStatus::InvalidArgument);
            }
            let source = bounded_text(rule.input_data, rule.input_len, "rule input")?;
            let target = bounded_text(rule.output_data, rule.output_len, "rule output")?;
            let entry = RewriteRule::with_cost(source, target, rule.cost)
                .map_err(|error| {
                    set_error(error.to_string());
                    DuallityStatus::InvalidArgument
                })?
                .with_priority(rule.priority);
            owned.push(entry);
        }
        let resource =
            crate::bindings::create_phonetic_rewrite(owned, allow_identity != 0, cache_policy)
                .map_err(map_error)?;
        *slot = resource.into_raw();
        Ok(())
    })
}

/// Construct one of duallity's native language-specific rewrite rule sets.
/// `locale` is 0=English, 1=German, 2=French. The built-in rules are copied
/// into the returned graph and share the same ownership/caching contract as
/// `duallity_phonetic_rewrite_new`.
///
/// # Safety
/// `out_resource` must be writable for one `VtResource`.
#[no_mangle]
pub unsafe extern "C" fn duallity_phonetic_rewrite_builtin_new(
    locale: u32,
    allow_identity: u8,
    policy: u32,
    capacity: u64,
    out_resource: *mut VtResource,
) -> DuallityStatus {
    boundary(|| {
        let slot = owned_output(out_resource)?;
        if allow_identity > 1 {
            set_error("allow_identity must be zero or one");
            return Err(DuallityStatus::InvalidArgument);
        }
        let rules = match locale {
            0 => CommonPhoneticRules::english(),
            1 => CommonPhoneticRules::german(),
            2 => CommonPhoneticRules::french(),
            _ => {
                set_error("unknown phonetic rewrite locale");
                return Err(DuallityStatus::InvalidArgument);
            }
        };
        let cache_policy = cache_policy(policy, capacity)?;
        let resource =
            crate::bindings::create_phonetic_rewrite(rules, allow_identity != 0, cache_policy)
                .map_err(map_error)?;
        *slot = resource.into_raw();
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use libdictenstein::bindings::{BindingUnitDomain, DynamicDawgBinding};

    #[test]
    fn phonetic_constructor_outputs_are_owned_and_fail_atomically() {
        let mut output = VtResource::NULL;
        assert_eq!(
            unsafe {
                duallity_phonetic_nfa_new(
                    b"(ph|f)one".as_ptr(),
                    b"(ph|f)one".len(),
                    ptr::null(),
                    0,
                    0.25,
                    0,
                    0,
                    &mut output,
                )
            },
            DuallityStatus::Ok
        );
        assert!(!output.is_null());
        super::super::duallity_resource_release(output);

        output = VtResource::NULL;
        assert_eq!(
            unsafe {
                duallity_phonetic_nfa_new(b"(".as_ptr(), 1, ptr::null(), 0, 0.0, 0, 0, &mut output)
            },
            DuallityStatus::InvalidArgument
        );
        assert!(output.is_null());
        assert_eq!(
            unsafe {
                duallity_phonetic_nfa_new(
                    [0xff].as_ptr(),
                    1,
                    ptr::null(),
                    0,
                    0.0,
                    0,
                    0,
                    &mut output,
                )
            },
            DuallityStatus::InvalidUtf8
        );
        assert!(output.is_null());
        assert_eq!(
            unsafe {
                duallity_phonetic_nfa_new(
                    b"a".as_ptr(),
                    1,
                    ptr::null(),
                    0,
                    f64::NAN,
                    0,
                    0,
                    &mut output,
                )
            },
            DuallityStatus::InvalidArgument
        );
        assert!(output.is_null());
    }

    #[test]
    fn phonetic_product_captures_dictionary_and_checks_threshold() {
        let dictionary = DynamicDawgBinding::new(BindingUnitDomain::UnicodeScalar);
        dictionary.insert_text(b"fone", None).unwrap();
        let resource = dictionary.resource();
        let raw = resource.as_raw();
        let mut output = VtResource::NULL;
        assert_eq!(
            unsafe {
                duallity_phonetic_product_new_ref(
                    &raw,
                    b"(ph|f)one".as_ptr(),
                    b"(ph|f)one".len(),
                    0,
                    0.0,
                    1.0,
                    1,
                    0,
                    &mut output,
                )
            },
            DuallityStatus::Ok
        );
        assert!(!output.is_null());
        let mut invalid_output = VtResource::NULL;
        assert_eq!(
            unsafe {
                duallity_phonetic_product_new_ref(
                    &raw,
                    b"a".as_ptr(),
                    1,
                    256,
                    0.0,
                    1.0,
                    0,
                    0,
                    &mut invalid_output,
                )
            },
            DuallityStatus::LimitExceeded
        );
        assert!(invalid_output.is_null());
        dictionary.clear();
        drop(resource);
        drop(dictionary);
        super::super::duallity_resource_release(output);

        output = VtResource::NULL;
        assert_eq!(
            unsafe {
                duallity_phonetic_product_new_ref(
                    ptr::null(),
                    b"a".as_ptr(),
                    1,
                    0,
                    0.0,
                    1.0,
                    0,
                    0,
                    &mut output,
                )
            },
            DuallityStatus::NullPointer
        );
        assert!(output.is_null());
    }

    #[test]
    fn rewrite_records_and_builtin_locale_validate_before_transfer() {
        let rule = DuallityPhoneticRuleV1 {
            header: config::DuallityRecordHeaderV1 {
                struct_size: size_of::<DuallityPhoneticRuleV1>() as u32,
                record_version: 1,
                reserved: 0,
            },
            input_data: b"ph".as_ptr(),
            input_len: 2,
            output_data: b"f".as_ptr(),
            output_len: 1,
            cost: 0.5,
            priority: 2,
            reserved: 0,
        };
        let mut output = VtResource::NULL;
        assert_eq!(
            unsafe { duallity_phonetic_rewrite_new(&rule, 1, 0, 2, 2, &mut output,) },
            DuallityStatus::Ok
        );
        assert!(!output.is_null());
        super::super::duallity_resource_release(output);

        output = VtResource::NULL;
        let invalid = DuallityPhoneticRuleV1 {
            header: config::DuallityRecordHeaderV1 {
                record_version: 2,
                ..rule.header
            },
            ..rule
        };
        assert_eq!(
            unsafe { duallity_phonetic_rewrite_new(&invalid, 1, 0, 0, 0, &mut output,) },
            DuallityStatus::InvalidArgument
        );
        assert!(output.is_null());
        assert_eq!(
            unsafe { duallity_phonetic_rewrite_builtin_new(3, 1, 0, 0, &mut output,) },
            DuallityStatus::InvalidArgument
        );
        assert!(output.is_null());
        assert_eq!(
            unsafe { duallity_phonetic_rewrite_builtin_new(1, 1, 0, 0, &mut output,) },
            DuallityStatus::Ok
        );
        super::super::duallity_resource_release(output);
    }
}
