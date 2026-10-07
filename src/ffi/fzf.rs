//! Versioned FZF scoring and bounded ranking across the native resource ABI.

use super::*;
use crate::bindings::{ResourceDictionary, ResourceNode, WfstConstruction};
use crate::{FzfConfig, FzfScheme, FzfScorer};
use libdictenstein::{Dictionary, DictionaryNode};
use liblevenshtein::transducer::PrefixPruner;
use lling_llang::wfst::SharedCachePolicy;
use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;

const MAX_TOP_K: usize = 4_096;
const MAX_WORK_UNITS: usize = 1_000_000;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DuallityFzfConfigV1 {
    pub header: DuallityRecordHeaderV1,
    pub case_sensitive: u32,
    pub scheme: u32,
    pub top_k: u64,
    pub max_query_chars: u64,
    pub max_candidate_chars: u64,
    pub max_work_units: u64,
    pub cache_policy: u32,
    pub reserved_zero: u32,
    pub cache_capacity: u64,
    pub reserved: [u64; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DuallityFzfScoreV1 {
    pub header: DuallityRecordHeaderV1,
    pub matched: u32,
    pub score: i32,
    pub maximum_score: i32,
    pub reserved_zero: u32,
    pub reserved: [u64; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DuallityFzfStatisticsV1 {
    pub header: DuallityRecordHeaderV1,
    pub columns_computed: u64,
    pub candidates_scored: u64,
    pub prefixes_pruned: u64,
    pub score_bound_prefixes_pruned: u64,
    pub length_prefixes_pruned: u64,
    pub upper_bounds_computed: u64,
    pub result_count: u64,
    pub work_units: u64,
    pub reserved: [u64; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DuallityFzfHitV1 {
    pub header: DuallityRecordHeaderV1,
    pub term_data: *const u8,
    pub term_len: u64,
    pub score: i32,
    pub reserved_zero: u32,
    pub reserved: [u64; 2],
}

#[derive(Debug, Eq, PartialEq)]
struct RankedHit {
    term: String,
    score: i32,
}

impl Ord for RankedHit {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .cmp(&other.score)
            .then_with(|| other.term.cmp(&self.term))
    }
}

impl PartialOrd for RankedHit {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Owned result. Hit text pointers borrowed from this handle expire at free.
pub struct DuallityFzfRanking {
    hits: Vec<RankedHit>,
    stats: DuallityFzfStatisticsV1,
}

fn header<T>() -> DuallityRecordHeaderV1 {
    DuallityRecordHeaderV1 {
        struct_size: size_of::<T>() as u32,
        record_version: config::CONFIG_RECORD_VERSION,
        reserved: 0,
    }
}

fn defaults() -> DuallityFzfConfigV1 {
    let native = FzfConfig::default();
    DuallityFzfConfigV1 {
        header: header::<DuallityFzfConfigV1>(),
        case_sensitive: u32::from(native.case_sensitive),
        scheme: 0,
        top_k: native.top_k as u64,
        max_query_chars: native.max_query_chars as u64,
        max_candidate_chars: native.max_candidate_chars as u64,
        max_work_units: 100_000,
        cache_policy: 0,
        reserved_zero: 0,
        cache_capacity: 0,
        reserved: [0; 2],
    }
}

fn reject(status: DuallityStatus, message: &str) -> DuallityStatus {
    set_error(message);
    status
}

fn bounded_usize(value: u64, maximum: usize, name: &str) -> Result<usize, DuallityStatus> {
    let value = usize::try_from(value).map_err(|_| {
        reject(
            DuallityStatus::LimitExceeded,
            &format!("{name} exceeds size_t"),
        )
    })?;
    if value > maximum {
        return Err(reject(
            DuallityStatus::LimitExceeded,
            &format!("{name} exceeds the native hard limit {maximum}"),
        ));
    }
    Ok(value)
}

struct ParsedFzf {
    raw: DuallityFzfConfigV1,
    native: FzfConfig,
    work_units: usize,
    cache_policy: SharedCachePolicy,
}

/// # Safety
/// `pointer` must designate a readable, aligned caller-sized record.
unsafe fn parse(pointer: *const DuallityFzfConfigV1) -> Result<ParsedFzf, DuallityStatus> {
    let raw = unsafe { config::record(pointer, "fzf config", config::CONFIG_MAX_RECORD_BYTES)? };
    if raw.case_sensitive > 1 || raw.reserved_zero != 0 || raw.reserved != [0; 2] {
        return Err(reject(
            DuallityStatus::InvalidArgument,
            "invalid FZF boolean or reserved field",
        ));
    }
    let scheme = match raw.scheme {
        0 => FzfScheme::Default,
        1 => FzfScheme::Path,
        2 => FzfScheme::History,
        _ => {
            return Err(reject(
                DuallityStatus::InvalidArgument,
                "unknown FZF scheme",
            ))
        }
    };
    let top_k = bounded_usize(raw.top_k, MAX_TOP_K, "FZF top_k")?;
    let max_query_chars = bounded_usize(raw.max_query_chars, 1_000, "FZF query length")?;
    let max_candidate_chars =
        bounded_usize(raw.max_candidate_chars, 1_000_000, "FZF candidate length")?;
    let work_units = bounded_usize(raw.max_work_units, MAX_WORK_UNITS, "FZF work units")?;
    let cache_policy =
        config::cache_policy(raw.cache_policy, raw.cache_capacity)?.effective(WfstKind::Fzf);
    Ok(ParsedFzf {
        raw,
        native: FzfConfig {
            case_sensitive: raw.case_sensitive != 0,
            scheme,
            top_k,
            max_query_chars,
            max_candidate_chars,
        },
        work_units,
        cache_policy,
    })
}

fn scorer(query: &str, config: FzfConfig) -> Result<FzfScorer, DuallityStatus> {
    FzfScorer::with_config(query, config)
        .map_err(|error| reject(DuallityStatus::LimitExceeded, &error.to_string()))
}

/// Fill a caller-sized version-1 FZF configuration with native defaults.
///
/// # Safety
/// `out_config` must be writable for its declared record extent.
#[no_mangle]
pub unsafe extern "C" fn duallity_fzf_config_default(
    out_config: *mut DuallityFzfConfigV1,
) -> DuallityStatus {
    boundary(|| {
        let extent = unsafe { sized_output_extent(out_config, "out_config")? };
        unsafe { write_sized(out_config, extent, defaults()) };
        Ok(())
    })
}

/// Score one UTF-8 candidate with the exact native FZF recurrence.
///
/// # Safety
/// Input pointers must remain readable during the call. Output is caller-sized.
#[no_mangle]
pub unsafe extern "C" fn duallity_fzf_score(
    query_data: *const u8,
    query_len: usize,
    candidate_data: *const u8,
    candidate_len: usize,
    options: *const DuallityFzfConfigV1,
    out_score: *mut DuallityFzfScoreV1,
) -> DuallityStatus {
    boundary(|| {
        let extent = unsafe { sized_output_extent(out_score, "out_score")? };
        let query = query(query_data, query_len)?;
        let candidate = super::query(candidate_data, candidate_len)?;
        let parsed = unsafe { parse(options)? };
        let scorer = scorer(query, parsed.native)?;
        let matched = scorer
            .score(candidate)
            .map_err(|error| reject(DuallityStatus::LimitExceeded, &error.to_string()))?;
        let result = DuallityFzfScoreV1 {
            header: header::<DuallityFzfScoreV1>(),
            matched: u32::from(matched.is_some()),
            score: matched.map_or(0, |value| value.score),
            maximum_score: scorer.maximum_score(),
            reserved_zero: 0,
            reserved: [0; 2],
        };
        unsafe { write_sized(out_score, extent, result) };
        Ok(())
    })
}

/// Construct a cache-controlled FZF WFST using every native scoring knob.
///
/// # Safety
/// `dictionary` and `options` must be readable for this call; the returned
/// handle retains one independent dictionary snapshot.
#[no_mangle]
pub unsafe extern "C" fn duallity_fzf_wfst_new_ref(
    dictionary: *const VtResource,
    query_data: *const u8,
    query_len: usize,
    options: *const DuallityFzfConfigV1,
    out_wfst: *mut *mut DuallityWfst,
) -> DuallityStatus {
    boundary(|| {
        let slot = checked_mut_pointer(out_wfst, "out_wfst")?;
        unsafe { slot.write(ptr::null_mut()) };
        let dictionary = checked_pointer(dictionary, "dictionary")?;
        let query = query(query_data, query_len)?;
        let parsed = unsafe { parse(options)? };
        let resource = unsafe {
            crate::bindings::create_wfst_configured(
                dictionary.read(),
                query,
                WfstConstruction {
                    maximum_distance: 0,
                    algorithm: Algorithm::Standard,
                    kind: WfstKind::Fzf,
                    limits: None,
                    operations: None,
                    fzf_config: Some(parsed.native),
                    cache_policy: parsed.cache_policy,
                },
            )
        }
        .map_err(map_error)?;
        let options = OwnedOptions::legacy(WfstKind::Fzf, Algorithm::Standard, 0)?;
        let handle = Box::into_raw(Box::new(DuallityWfst {
            resource,
            options: Some(options),
            fzf_config: Some(parsed.raw),
        }));
        unsafe { slot.write(handle) };
        Ok(())
    })
}

/// Read back the captured FZF options. Cache settings reflect later changes.
///
/// # Safety
/// `wfst` must be live and `out_config` writable for its declared extent.
#[no_mangle]
pub unsafe extern "C" fn duallity_fzf_wfst_config_get(
    wfst: *const DuallityWfst,
    out_config: *mut DuallityFzfConfigV1,
) -> DuallityStatus {
    boundary(|| {
        let extent = unsafe { sized_output_extent(out_config, "out_config")? };
        let handle = unsafe { live_handle(wfst)? };
        let mut config = handle.fzf_config.ok_or_else(|| {
            reject(
                DuallityStatus::IncompatibleResource,
                "WFST was not constructed with FZF configuration",
            )
        })?;
        let (policy, capacity) = cache::effective_policy(&handle.resource)?;
        config.cache_policy = policy;
        config.cache_capacity = capacity;
        unsafe { write_sized(out_config, extent, config) };
        Ok(())
    })
}

enum WalkEvent {
    Enter(char, ResourceNode, usize),
    Leave(char, usize),
}

fn push_children(
    node: &ResourceNode,
    depth: usize,
    stack: &mut Vec<WalkEvent>,
    work: &mut usize,
    limit: usize,
) -> Result<(), DuallityStatus> {
    let remaining = limit.saturating_sub(*work);
    let children: Vec<_> = node.edges().take(remaining.saturating_add(1)).collect();
    if children.len() > remaining {
        return Err(reject(
            DuallityStatus::LimitExceeded,
            "FZF traversal exceeded max_work_units",
        ));
    }
    *work += children.len();
    for (unit, child) in children.into_iter().rev() {
        stack.push(WalkEvent::Enter(unit, child, depth + 1));
    }
    Ok(())
}

fn ranked(
    dictionary: &ResourceDictionary,
    query: &str,
    parsed: &ParsedFzf,
) -> Result<DuallityFzfRanking, DuallityStatus> {
    if parsed.native.top_k == 0 {
        return Err(reject(
            DuallityStatus::InvalidArgument,
            "FZF ranking requires top_k > 0",
        ));
    }
    let mut scorer = scorer(query, parsed.native)?;
    let root = dictionary.root();
    let mut heap: BinaryHeap<Reverse<RankedHit>> = BinaryHeap::new();
    let mut stack = Vec::new();
    let mut prefix = Vec::new();
    let mut work = 0usize;
    if root.is_final() && scorer.permits_accept(&prefix) {
        if let Some(score) = scorer.accept(&prefix) {
            heap.push(Reverse(RankedHit {
                term: String::new(),
                score: score as i32,
            }));
        }
    }
    push_children(&root, 0, &mut stack, &mut work, parsed.work_units)?;
    while let Some(event) = stack.pop() {
        match event {
            WalkEvent::Leave(unit, depth) => {
                scorer.leave(unit, depth);
                prefix.pop();
            }
            WalkEvent::Enter(unit, node, depth) => {
                if !scorer.enter(unit, depth) {
                    scorer.leave(unit, depth);
                    continue;
                }
                prefix.push(unit);
                if node.is_final() && scorer.permits_accept(&prefix) {
                    if let Some(score) = scorer.accept(&prefix) {
                        let hit = RankedHit {
                            term: prefix.iter().collect(),
                            score: score as i32,
                        };
                        if heap.len() < parsed.native.top_k {
                            heap.push(Reverse(hit));
                        } else if heap.peek().is_some_and(|worst| hit > worst.0) {
                            heap.pop();
                            heap.push(Reverse(hit));
                        }
                    }
                }
                stack.push(WalkEvent::Leave(unit, depth));
                push_children(&node, depth, &mut stack, &mut work, parsed.work_units)?;
            }
        }
    }
    let native = scorer.stats();
    let mut hits: Vec<_> = heap.into_iter().map(|entry| entry.0).collect();
    hits.sort_by(|left, right| right.cmp(left));
    let stats = DuallityFzfStatisticsV1 {
        header: header::<DuallityFzfStatisticsV1>(),
        columns_computed: native.columns_computed as u64,
        candidates_scored: native.candidates_scored as u64,
        prefixes_pruned: native.prefixes_pruned as u64,
        score_bound_prefixes_pruned: native.score_bound_prefixes_pruned as u64,
        length_prefixes_pruned: native.length_prefixes_pruned as u64,
        upper_bounds_computed: native.upper_bounds_computed as u64,
        result_count: hits.len() as u64,
        work_units: work as u64,
        reserved: [0; 2],
    };
    Ok(DuallityFzfRanking { hits, stats })
}

/// Rank a captured dictionary revision with bounded native DFS work.
///
/// # Safety
/// Input pointers are borrowed for this call. The output handle owns copied
/// ranked terms and must be freed with `duallity_fzf_ranking_free`.
#[no_mangle]
pub unsafe extern "C" fn duallity_fzf_rank_ref(
    dictionary: *const VtResource,
    query_data: *const u8,
    query_len: usize,
    options: *const DuallityFzfConfigV1,
    out_ranking: *mut *mut DuallityFzfRanking,
) -> DuallityStatus {
    boundary(|| {
        let slot = checked_mut_pointer(out_ranking, "out_ranking")?;
        unsafe { slot.write(ptr::null_mut()) };
        let dictionary = checked_pointer(dictionary, "dictionary")?;
        let query = query(query_data, query_len)?;
        let parsed = unsafe { parse(options)? };
        let captured =
            unsafe { ResourceDictionary::capture(dictionary.read()) }.map_err(map_error)?;
        let result = captured
            .with_checked(|| Ok(ranked(&captured, query, &parsed)))
            .map_err(|provider| map_error(BindingError::Provider(provider)))??;
        unsafe { slot.write(Box::into_raw(Box::new(result))) };
        Ok(())
    })
}

/// Return the number of copied ranking hits.
///
/// # Safety
/// `ranking` must be a live handle and `out_len` writable.
#[no_mangle]
pub unsafe extern "C" fn duallity_fzf_ranking_len(
    ranking: *const DuallityFzfRanking,
    out_len: *mut u64,
) -> DuallityStatus {
    boundary(|| {
        let ranking = checked_pointer(ranking, "ranking")?;
        let out_len = output(out_len, "out_len")?;
        *out_len = unsafe { &*ranking }.hits.len() as u64;
        Ok(())
    })
}

/// Read one borrowed hit record by zero-based index.
///
/// # Safety
/// `ranking` must remain live while the returned term pointer is used.
#[no_mangle]
pub unsafe extern "C" fn duallity_fzf_ranking_get(
    ranking: *const DuallityFzfRanking,
    index: u64,
    out_hit: *mut DuallityFzfHitV1,
) -> DuallityStatus {
    boundary(|| {
        let extent = unsafe { sized_output_extent(out_hit, "out_hit")? };
        let ranking = checked_pointer(ranking, "ranking")?;
        let index = usize::try_from(index).map_err(|_| {
            reject(
                DuallityStatus::InvalidArgument,
                "FZF ranking index is out of range",
            )
        })?;
        let hit = unsafe { &*ranking }.hits.get(index).ok_or_else(|| {
            reject(
                DuallityStatus::InvalidArgument,
                "FZF ranking index is out of range",
            )
        })?;
        let record = DuallityFzfHitV1 {
            header: header::<DuallityFzfHitV1>(),
            term_data: hit.term.as_ptr(),
            term_len: hit.term.len() as u64,
            score: hit.score,
            reserved_zero: 0,
            reserved: [0; 2],
        };
        unsafe { write_sized(out_hit, extent, record) };
        Ok(())
    })
}

/// Copy complete native traversal counters.
///
/// # Safety
/// `ranking` must be live and `out_stats` writable for its declared extent.
#[no_mangle]
pub unsafe extern "C" fn duallity_fzf_ranking_statistics(
    ranking: *const DuallityFzfRanking,
    out_stats: *mut DuallityFzfStatisticsV1,
) -> DuallityStatus {
    boundary(|| {
        let extent = unsafe { sized_output_extent(out_stats, "out_stats")? };
        let ranking = checked_pointer(ranking, "ranking")?;
        unsafe { write_sized(out_stats, extent, (&*ranking).stats) };
        Ok(())
    })
}

/// Release a ranking handle. Null is accepted.
///
/// # Safety
/// Non-null pointers must be live handles returned by `duallity_fzf_rank_ref`.
#[no_mangle]
pub unsafe extern "C" fn duallity_fzf_ranking_free(ranking: *mut DuallityFzfRanking) {
    if !ranking.is_null() {
        unsafe { drop(Box::from_raw(ranking)) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libdictenstein::bindings::{BindingUnitDomain, DynamicDawgBinding};

    fn fixture() -> DynamicDawgBinding {
        let dictionary = DynamicDawgBinding::new(BindingUnitDomain::UnicodeScalar);
        for term in ["foo/bar", "FooBar", "far", "fóó", "", "zoo"] {
            dictionary.insert_text(term.as_bytes(), None).unwrap();
        }
        dictionary
    }

    #[test]
    fn score_matches_native_for_all_schemes_and_case_modes() {
        for scheme in 0..=2 {
            for case_sensitive in 0..=1 {
                let mut options = defaults();
                options.scheme = scheme;
                options.case_sensitive = case_sensitive;
                for candidate in ["foo/bar", "FooBar", "far", "fóó", "zoo", ""] {
                    let mut output = DuallityFzfScoreV1 {
                        header: header::<DuallityFzfScoreV1>(),
                        matched: 0,
                        score: 0,
                        maximum_score: 0,
                        reserved_zero: 0,
                        reserved: [0; 2],
                    };
                    let status = unsafe {
                        duallity_fzf_score(
                            b"fb".as_ptr(),
                            2,
                            candidate.as_ptr(),
                            candidate.len(),
                            &options,
                            &mut output,
                        )
                    };
                    assert_eq!(status, DuallityStatus::Ok);
                    let native =
                        FzfScorer::with_config("fb", unsafe { parse(&options) }.unwrap().native)
                            .unwrap();
                    let expected = native.score(candidate).unwrap();
                    assert_eq!(output.matched != 0, expected.is_some());
                    assert_eq!(output.score, expected.map_or(0, |item| item.score));
                    assert_eq!(output.maximum_score, native.maximum_score());
                }
            }
        }
    }

    #[test]
    fn ranked_results_match_flat_oracle_and_capture_snapshot() {
        let dictionary = fixture();
        let source = dictionary.resource();
        let mut options = defaults();
        options.top_k = 3;
        options.scheme = 1;
        let mut ranking = ptr::null_mut();
        assert_eq!(
            unsafe {
                duallity_fzf_rank_ref(&source.as_raw(), b"fb".as_ptr(), 2, &options, &mut ranking)
            },
            DuallityStatus::Ok
        );
        assert!(!ranking.is_null());
        let scorer =
            FzfScorer::with_config("fb", unsafe { parse(&options) }.unwrap().native).unwrap();
        let mut expected: Vec<_> = ["foo/bar", "FooBar", "far", "fóó", "", "zoo"]
            .into_iter()
            .filter_map(|term| {
                scorer.score(term).unwrap().map(|matched| RankedHit {
                    term: term.to_owned(),
                    score: matched.score,
                })
            })
            .collect();
        expected.sort_by(|left, right| right.cmp(left));
        expected.truncate(3);
        let mut len = 0;
        assert_eq!(
            unsafe { duallity_fzf_ranking_len(ranking, &mut len) },
            DuallityStatus::Ok
        );
        assert_eq!(len as usize, expected.len());
        for (index, want) in expected.iter().enumerate() {
            let mut hit = DuallityFzfHitV1 {
                header: header::<DuallityFzfHitV1>(),
                term_data: ptr::null(),
                term_len: 0,
                score: 0,
                reserved_zero: 0,
                reserved: [0; 2],
            };
            assert_eq!(
                unsafe { duallity_fzf_ranking_get(ranking, index as u64, &mut hit) },
                DuallityStatus::Ok
            );
            let text = unsafe { slice::from_raw_parts(hit.term_data, hit.term_len as usize) };
            assert_eq!(text, want.term.as_bytes());
            assert_eq!(hit.score, want.score);
        }
        let mut stats = DuallityFzfStatisticsV1 {
            header: header::<DuallityFzfStatisticsV1>(),
            columns_computed: 0,
            candidates_scored: 0,
            prefixes_pruned: 0,
            score_bound_prefixes_pruned: 0,
            length_prefixes_pruned: 0,
            upper_bounds_computed: 0,
            result_count: 0,
            work_units: 0,
            reserved: [0; 2],
        };
        assert_eq!(
            unsafe { duallity_fzf_ranking_statistics(ranking, &mut stats) },
            DuallityStatus::Ok
        );
        assert_eq!(stats.result_count, len);
        assert!(stats.columns_computed > 0);
        assert!(stats.work_units > 0);
        unsafe { duallity_fzf_ranking_free(ranking) };
    }

    #[test]
    fn work_limit_rejects_incomplete_ranking_and_clears_output() {
        let dictionary = fixture();
        let source = dictionary.resource();
        let mut options = defaults();
        options.top_k = 2;
        options.max_work_units = 1;
        let mut ranking = std::ptr::dangling_mut::<DuallityFzfRanking>();
        assert_eq!(
            unsafe {
                duallity_fzf_rank_ref(&source.as_raw(), b"f".as_ptr(), 1, &options, &mut ranking)
            },
            DuallityStatus::LimitExceeded
        );
        assert!(ranking.is_null());
        options.reserved[0] = 1;
        assert_eq!(
            unsafe {
                duallity_fzf_rank_ref(&source.as_raw(), b"f".as_ptr(), 1, &options, &mut ranking)
            },
            DuallityStatus::InvalidArgument
        );
        assert!(ranking.is_null());
    }

    #[test]
    fn configured_wfst_preserves_fzf_options_and_cache_controls() {
        let dictionary = fixture();
        let source = dictionary.resource();
        let mut options = defaults();
        options.case_sensitive = 1;
        options.scheme = 2;
        options.cache_policy = 2;
        options.cache_capacity = 2;
        let mut wfst = ptr::null_mut();
        assert_eq!(
            unsafe {
                duallity_fzf_wfst_new_ref(&source.as_raw(), b"fb".as_ptr(), 2, &options, &mut wfst)
            },
            DuallityStatus::Ok
        );
        assert!(!wfst.is_null());
        let mut readback = defaults();
        assert_eq!(
            unsafe { duallity_fzf_wfst_config_get(wfst, &mut readback) },
            DuallityStatus::Ok
        );
        assert_eq!(readback.case_sensitive, 1);
        assert_eq!(readback.scheme, 2);
        assert_eq!(readback.cache_policy, 2);
        assert_eq!(readback.cache_capacity, 2);
        assert_eq!(
            unsafe { duallity_wfst_cache_set_policy(wfst, 1, 0) },
            DuallityStatus::Ok
        );
        assert_eq!(
            unsafe { duallity_fzf_wfst_config_get(wfst, &mut readback) },
            DuallityStatus::Ok
        );
        assert_eq!(readback.cache_policy, 1);
        assert_eq!(readback.cache_capacity, 0);
        unsafe { duallity_wfst_free(wfst) };
    }
}
