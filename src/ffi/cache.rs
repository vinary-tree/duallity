//! Staged revision-3 cache controls over the single exported provider cache.
//!
//! These helpers remain internal until the complete C/foreign ABI is
//! qualified. No duallity-side state payload cache is created here.

use super::config::{DuallityCacheStatisticsV1, DuallityRecordHeaderV1, RequestedCachePolicy};
use super::{set_error, DuallityStatus};
use crate::bindings::WfstKind;
use lling_llang::bindings::{OwnedWfstResource, ProviderCacheControl};
use lling_llang::wfst::SharedCachePolicy;
use std::mem::size_of;

fn control(resource: &OwnedWfstResource) -> Result<ProviderCacheControl, DuallityStatus> {
    resource.provider_cache().ok_or_else(|| {
        set_error("duallity resource has no provider cache");
        DuallityStatus::IncompatibleResource
    })
}

fn wire_count(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

/// Return the effective (normalized) selector and capacity for inspection.
pub(super) fn effective_policy(resource: &OwnedWfstResource) -> Result<(u32, u64), DuallityStatus> {
    Ok(match control(resource)?.policy() {
        SharedCachePolicy::CacheAll => (0, 0),
        SharedCachePolicy::NoCache => (1, 0),
        SharedCachePolicy::Lru { capacity } => (2, wire_count(capacity.get())),
    })
}

/// Read cumulative provider counters and a coherent current-residency root.
pub(super) fn statistics(
    resource: &OwnedWfstResource,
) -> Result<DuallityCacheStatisticsV1, DuallityStatus> {
    let stats = control(resource)?.statistics();
    Ok(DuallityCacheStatisticsV1 {
        header: DuallityRecordHeaderV1 {
            struct_size: size_of::<DuallityCacheStatisticsV1>() as u32,
            record_version: super::config::CONFIG_RECORD_VERSION,
            reserved: 0,
        },
        hits: stats.hits,
        misses: stats.misses,
        faults: stats.faults,
        uncacheable_results: stats.uncacheable_results,
        insertions: stats.insertions,
        evictions: stats.evictions,
        raced_publications: stats.raced_publications,
        clears: stats.clears,
        resident_states: wire_count(stats.resident_states),
        recency_records: wire_count(stats.recency_records),
        reserved: [0; 2],
    })
}

/// Drop all cache residency without changing semantic state identifiers.
pub(super) fn clear(resource: &OwnedWfstResource) -> Result<(), DuallityStatus> {
    control(resource)?.clear();
    Ok(())
}

/// Atomically publish a new policy and empty cache generation.
pub(super) fn set_policy(
    resource: &OwnedWfstResource,
    requested: RequestedCachePolicy,
    kind: WfstKind,
) -> Result<(), DuallityStatus> {
    control(resource)?.set_policy(requested.effective(kind));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::config::{cache_policy, parse_options, DuallityWfstOptionsV1};
    use libdictenstein::bindings::{BindingUnitDomain, DynamicDawgBinding};
    use lling_llang::bindings::{ScalarWfstProvider, ScalarWfstState};
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, OnceLock};
    use std::thread;
    use vinary_tree_interop::{
        VtStatus, VtWeightDomain, VtWfstVTable, VT_WFST_INTERFACE_ID, VT_WFST_INTERFACE_VERSION,
    };

    fn state_result(resource: &OwnedWfstResource, state: u64) -> (u32, u8, u8) {
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
            let (mut valid, mut final_state, mut weight) = (0, 0, 0.0);
            let status = table.state_info.unwrap()(
                raw.context,
                state,
                &mut valid,
                &mut final_state,
                &mut weight,
            );
            (status, valid, final_state)
        }
    }

    fn state_info(resource: &OwnedWfstResource, state: u64) -> (u8, u8) {
        let (status, valid, final_state) = state_result(resource, state);
        assert_eq!(status, VtStatus::Ok.to_raw());
        (valid, final_state)
    }

    struct CountingProvider {
        calls: AtomicU64,
        control: OnceLock<ProviderCacheControl>,
        clear_reentrantly: bool,
    }

    impl ScalarWfstProvider for CountingProvider {
        fn weight_domain(&self) -> VtWeightDomain {
            VtWeightDomain::TropicalF64
        }

        fn start(&self) -> Result<u64, VtStatus> {
            Ok(0)
        }

        fn num_states(&self) -> Result<Option<usize>, VtStatus> {
            Ok(Some(3))
        }

        fn state(&self, state: u64) -> Result<ScalarWfstState, VtStatus> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if state == 97 {
                return Err(VtStatus::ProviderError);
            }
            if self.clear_reentrantly {
                self.control.get().expect("installed before query").clear();
            }
            Ok(ScalarWfstState {
                valid: state < 3,
                is_final: state < 3,
                final_weight: 0.0,
                arcs: Vec::new(),
            })
        }
    }

    fn provider_resource(reentrant: bool) -> (OwnedWfstResource, Arc<CountingProvider>) {
        let provider = Arc::new(CountingProvider {
            calls: AtomicU64::new(0),
            control: OnceLock::new(),
            clear_reentrantly: reentrant,
        });
        let resource = OwnedWfstResource::from_provider_with_cache(
            Arc::clone(&provider) as Arc<dyn ScalarWfstProvider>,
            SharedCachePolicy::CacheAll,
        );
        provider
            .control
            .set(resource.provider_cache().unwrap())
            .unwrap_or_else(|_| panic!("cache control already installed"));
        (resource, provider)
    }

    #[test]
    fn requested_policy_normalizes_legacy_zero_capacity_by_kind() {
        let zero = cache_policy(2, 0).unwrap();
        assert_eq!(
            zero.effective(WfstKind::GeneralizedStandard),
            SharedCachePolicy::Lru {
                capacity: std::num::NonZeroUsize::new(100_000).unwrap(),
            }
        );
        assert_eq!(zero.effective(WfstKind::Fzf), SharedCachePolicy::NoCache);
        for capacity in [1, 2] {
            assert_eq!(
                cache_policy(2, capacity).unwrap().effective(WfstKind::Fzf),
                SharedCachePolicy::Lru {
                    capacity: std::num::NonZeroUsize::new(capacity as usize).unwrap(),
                }
            );
        }
        assert_eq!(
            cache_policy(0, 0).unwrap().effective(WfstKind::Fzf),
            SharedCachePolicy::CacheAll
        );
        assert_eq!(
            cache_policy(1, 0).unwrap().effective(WfstKind::Levenshtein),
            SharedCachePolicy::NoCache
        );
    }

    #[test]
    fn configured_duallity_resource_uses_only_exported_provider_cache() {
        let dictionary = DynamicDawgBinding::new(BindingUnitDomain::UnicodeScalar);
        dictionary.insert_text(b"cat", None).unwrap();
        let source = dictionary.resource();
        for (selector, capacity, expected) in [
            (0, 0, (0, 0)),
            (1, 0, (1, 0)),
            (2, 0, (2, 100_000)),
            (2, 1, (2, 1)),
            (2, 2, (2, 2)),
        ] {
            let raw = DuallityWfstOptionsV1 {
                header: super::super::config::DuallityRecordHeaderV1 {
                    struct_size: size_of::<DuallityWfstOptionsV1>() as u32,
                    record_version: super::super::config::CONFIG_RECORD_VERSION,
                    reserved: 0,
                },
                kind: WfstKind::GeneralizedStandard as u32,
                algorithm: 0,
                maximum_distance: 1,
                cache_policy: selector,
                reserved_zero: 0,
                cache_capacity: capacity,
                limits: std::ptr::null(),
                operations: std::ptr::null(),
                operation_count: 0,
                operation_stride: 0,
                reserved: [0; 2],
            };
            let parsed = unsafe { parse_options(&raw) }.unwrap();
            let resource = unsafe {
                crate::bindings::create_wfst_configured(
                    source.as_raw(),
                    "cat",
                    parsed.into_construction(),
                )
            }
            .unwrap();
            assert_eq!(effective_policy(&resource).unwrap(), expected);
            let (valid, _) = state_info(&resource, 0);
            assert_eq!(valid, 1);
            let stats = statistics(&resource).unwrap();
            assert!(stats.misses >= 1);
            assert_eq!(stats.resident_states, u64::from(selector != 1));
            assert_eq!(stats.recency_records, u64::from(selector == 2));
        }
    }

    #[test]
    fn clone_policy_change_and_clear_share_one_generation() {
        let (resource, provider) = provider_resource(false);
        let clone = resource.clone();
        assert_eq!(state_info(&resource, 0), (1, 1));
        assert_eq!(state_info(&clone, 0), (1, 1));
        assert_eq!(provider.calls.load(Ordering::Relaxed), 1);
        let stats = statistics(&resource).unwrap();
        assert_eq!((stats.misses, stats.hits, stats.resident_states), (1, 1, 1));

        set_policy(
            &clone,
            RequestedCachePolicy::Lru { capacity: 1 },
            WfstKind::Fzf,
        )
        .unwrap();
        assert_eq!(effective_policy(&resource).unwrap(), (2, 1));
        assert_eq!(statistics(&resource).unwrap().resident_states, 0);
        assert_eq!(state_info(&resource, 0), (1, 1));
        assert_eq!(state_info(&clone, 1), (1, 1));
        assert_eq!(statistics(&resource).unwrap().resident_states, 1);
        assert_eq!(statistics(&resource).unwrap().recency_records, 1);

        clear(&resource).unwrap();
        let stats = statistics(&clone).unwrap();
        assert_eq!(stats.resident_states, 0);
        assert!(stats.clears >= 2);
        set_policy(&resource, RequestedCachePolicy::NoCache, WfstKind::Fzf).unwrap();
        assert_eq!(state_info(&clone, 0), (1, 1));
        assert_eq!(state_info(&clone, 0), (1, 1));
        assert_eq!(statistics(&clone).unwrap().resident_states, 0);
    }

    #[test]
    fn lru_one_and_two_enforce_exact_residency_and_cache_all_is_unbounded() {
        let (resource, provider) = provider_resource(false);
        set_policy(
            &resource,
            RequestedCachePolicy::Lru { capacity: 1 },
            WfstKind::GeneralizedStandard,
        )
        .unwrap();
        assert_eq!(state_info(&resource, 0), (1, 1));
        assert_eq!(state_info(&resource, 1), (1, 1));
        let one = statistics(&resource).unwrap();
        assert_eq!((one.resident_states, one.recency_records), (1, 1));
        assert!(one.evictions >= 1);
        assert_eq!(state_info(&resource, 0), (1, 1));
        assert_eq!(provider.calls.load(Ordering::Relaxed), 3);

        set_policy(
            &resource,
            RequestedCachePolicy::Lru { capacity: 2 },
            WfstKind::GeneralizedStandard,
        )
        .unwrap();
        for id in 0..3 {
            assert_eq!(state_info(&resource, id), (1, 1));
        }
        let two = statistics(&resource).unwrap();
        assert_eq!(effective_policy(&resource).unwrap(), (2, 2));
        assert_eq!((two.resident_states, two.recency_records), (2, 2));
        assert!(two.evictions > one.evictions);

        set_policy(
            &resource,
            RequestedCachePolicy::CacheAll,
            WfstKind::GeneralizedStandard,
        )
        .unwrap();
        for id in 0..3 {
            assert_eq!(state_info(&resource, id), (1, 1));
        }
        let all = statistics(&resource).unwrap();
        assert_eq!(effective_policy(&resource).unwrap(), (0, 0));
        assert_eq!((all.resident_states, all.recency_records), (3, 0));

        set_policy(
            &resource,
            RequestedCachePolicy::NoCache,
            WfstKind::GeneralizedStandard,
        )
        .unwrap();
        let before = provider.calls.load(Ordering::Relaxed);
        assert_eq!(state_info(&resource, 0), (1, 1));
        assert_eq!(state_info(&resource, 0), (1, 1));
        assert_eq!(provider.calls.load(Ordering::Relaxed), before + 2);
        let none = statistics(&resource).unwrap();
        assert_eq!(effective_policy(&resource).unwrap(), (1, 0));
        assert_eq!((none.resident_states, none.recency_records), (0, 0));
    }

    #[test]
    fn concurrent_and_reentrant_requests_do_not_hold_cache_lock_in_callback() {
        let (resource, provider) = provider_resource(true);
        let mut workers = Vec::new();
        for _ in 0..8 {
            let clone = resource.clone();
            workers.push(thread::spawn(move || {
                for _ in 0..16 {
                    assert_eq!(state_info(&clone, 0), (1, 1));
                }
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }
        assert!(provider.calls.load(Ordering::Relaxed) >= 1);
        let stats = statistics(&resource).unwrap();
        assert!(stats.clears >= 1);
        assert!(stats.hits + stats.misses >= 128);
        assert!(stats.resident_states <= 1);
    }

    #[test]
    fn invalid_and_faulting_states_are_counted_but_never_cached() {
        let (resource, provider) = provider_resource(false);
        assert_eq!(state_info(&resource, 99), (0, 0));
        assert_eq!(state_info(&resource, 99), (0, 0));
        assert_eq!(
            state_result(&resource, 97).0,
            VtStatus::ProviderError.to_raw()
        );
        assert_eq!(
            state_result(&resource, 97).0,
            VtStatus::ProviderError.to_raw()
        );
        let stats = statistics(&resource).unwrap();
        assert_eq!(stats.uncacheable_results, 2);
        assert_eq!(stats.faults, 2);
        assert_eq!(stats.resident_states, 0);
        assert_eq!(provider.calls.load(Ordering::Relaxed), 4);
    }
}
