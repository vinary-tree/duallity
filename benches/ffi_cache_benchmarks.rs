//! Real dictionary-backed state expansion through the exported WFST callbacks.
//!
//! Construction and discovery are outside timing. A fixed breadth-first prefix
//! of reachable states is replayed continuously, so capacity-plus-one LRU is
//! genuine steady eviction, not repeated clearing or restarted warm samples.
//! Compare policies within each family; these are not whole-query benchmarks.

#[path = "../tests/support/wfst_walk.rs"]
mod wfst_walk;

use std::collections::{HashSet, VecDeque};
use std::ffi::c_void;
use std::hint::black_box;
use std::num::NonZeroUsize;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use duallity::bindings::{create_wfst, WfstKind};
use libdictenstein::bindings::{BindingUnitDomain, DynamicDawgBinding};
use liblevenshtein::transducer::Algorithm;
use lling_llang::bindings::OwnedWfstResource;
use lling_llang::wfst::SharedCachePolicy;
use vinary_tree_interop::{
    VtStatus, VtWfstArc, VtWfstVTable, VT_WFST_INTERFACE_ID, VT_WFST_INTERFACE_VERSION,
};
use wfst_walk::{StateExpansion, WfstView};

const CAPACITY: usize = 64;

fn dictionary() -> DynamicDawgBinding {
    let dictionary = DynamicDawgBinding::new(BindingUnitDomain::UnicodeScalar);
    for number in 0..4096 {
        let mut value = number;
        let mut term = [b'a'; 4];
        for unit in term.iter_mut().rev() {
            *unit += (value % 8) as u8;
            value /= 8;
        }
        dictionary
            .insert_text(&term, None)
            .expect("fixture insertion");
    }
    dictionary
}

/// Holds a real resource borrow for the complete lifetime of its raw callbacks.
struct Requests<'a> {
    resource: &'a OwnedWfstResource,
    table: &'a VtWfstVTable,
    buffer: Vec<VtWfstArc>,
}

impl<'a> Requests<'a> {
    fn new(resource: &'a OwnedWfstResource, maximum_degree: usize) -> Self {
        let raw = resource.as_raw();
        let mut interface: *const c_void = std::ptr::null();
        // SAFETY: resource owns the context and table throughout this borrow.
        let table = unsafe {
            assert_eq!(
                (*raw.vtable).query_interface.expect("query_interface")(
                    raw.context,
                    &VT_WFST_INTERFACE_ID,
                    VT_WFST_INTERFACE_VERSION,
                    &mut interface,
                ),
                VtStatus::Ok.to_raw()
            );
            interface
                .cast::<VtWfstVTable>()
                .as_ref()
                .expect("WFST table")
        };
        Self {
            resource,
            table,
            buffer: vec![VtWfstArc::default(); maximum_degree.max(1)],
        }
    }

    fn info(&self, state: u64) -> (u8, f64) {
        let (mut valid, mut finality, mut weight) = (0, 0, 0.0);
        // SAFETY: resource is retained; all output pointers are live and distinct.
        let status = unsafe {
            self.table.state_info.expect("state_info")(
                self.resource.as_raw().context,
                state,
                &mut valid,
                &mut finality,
                &mut weight,
            )
        };
        assert_eq!(status, VtStatus::Ok.to_raw());
        assert_eq!(valid, 1, "discovered immutable state stays valid");
        (finality, weight)
    }

    fn arcs(&mut self, state: u64) -> &[VtWfstArc] {
        let (mut written, mut total) = (0, 0);
        // SAFETY: buffer owns capacity initialized elements; outputs are live.
        let status = unsafe {
            self.table.state_arcs.expect("state_arcs")(
                self.resource.as_raw().context,
                state,
                0,
                self.buffer.as_mut_ptr(),
                self.buffer.len(),
                &mut written,
                &mut total,
            )
        };
        assert_eq!(status, VtStatus::Ok.to_raw());
        assert_eq!(written, total, "the pre-sized page contains every arc");
        assert!(written <= self.buffer.len());
        &self.buffer[..written]
    }

    fn verify(&mut self, state: u64, expected: &StateExpansion) {
        assert!(expected.valid);
        let (finality, weight) = self.info(state);
        assert_eq!(finality == 1, expected.is_final);
        assert_eq!(weight.to_bits(), expected.final_weight.to_bits());
        let actual = self.arcs(state);
        assert_eq!(actual.len(), expected.arcs.len());
        for (actual, expected) in actual.iter().zip(&expected.arcs) {
            assert_eq!(actual.input_label, expected.input_label);
            assert_eq!(actual.output_label, expected.output_label);
            assert_eq!(actual.target_state, expected.target_state);
            assert_eq!(actual.weight.to_bits(), expected.weight.to_bits());
            assert_eq!(actual.has_input, expected.has_input);
            assert_eq!(actual.has_output, expected.has_output);
            assert_eq!(actual.reserved, expected.reserved);
        }
    }
}

fn discover(resource: &OwnedWfstResource) -> Vec<(u64, StateExpansion)> {
    let view = WfstView::new(resource.as_raw());
    let start = view.start();
    let mut seen = HashSet::from([start]);
    let mut frontier = VecDeque::from([start]);
    let mut states = Vec::with_capacity(CAPACITY + 1);
    while states.len() <= CAPACITY {
        let state = frontier
            .pop_front()
            .expect("fixture has at least 65 states");
        let expansion = view.expand(state).expect("fixture state expands");
        assert!(expansion.valid);
        for arc in &expansion.arcs {
            if seen.insert(arc.target_state) {
                frontier.push_back(arc.target_state);
            }
        }
        states.push((state, expansion));
    }
    states
}

fn retained_adapter_policies(criterion: &mut Criterion) {
    let dictionary = dictionary();
    let source = dictionary.resource();
    let mut group = criterion.benchmark_group("dictionary_cache_abi");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    for (family, kind) in [
        ("classic", WfstKind::Levenshtein),
        ("universal", WfstKind::UniversalStandard),
        ("generalized", WfstKind::GeneralizedStandard),
        ("fzf", WfstKind::Fzf),
    ] {
        // SAFETY: source owns a live dictionary resource throughout construction.
        let resource =
            unsafe { create_wfst(source.as_raw(), "abcd", 2, Algorithm::Standard, kind) }
                .expect("dictionary-backed adapter");
        let states = discover(&resource);
        let maximum_degree = states
            .iter()
            .map(|(_, state)| state.arcs.len())
            .max()
            .expect("nonempty fixture");
        eprintln!("fixture family={family} dictionary_terms=4096 query=abcd bound=2 states={} arcs={} max_degree={maximum_degree}",
            states.len(), states.iter().map(|(_, state)| state.arcs.len()).sum::<usize>());
        let control = resource
            .provider_cache()
            .expect("authoritative exporter cache");
        let mut requests = Requests::new(&resource, maximum_degree);
        for (policy_name, policy) in [
            ("all", SharedCachePolicy::CacheAll),
            ("none", SharedCachePolicy::NoCache),
            (
                "lru64",
                SharedCachePolicy::Lru {
                    capacity: NonZeroUsize::new(CAPACITY).expect("positive capacity"),
                },
            ),
        ] {
            control.set_policy(policy);
            for (state, expected) in &states {
                requests.verify(*state, expected);
            }
            for working_set in [CAPACITY, CAPACITY + 1] {
                for paired in [false, true] {
                    control.clear();
                    for (state, _) in states.iter().take(working_set) {
                        black_box(requests.info(*state));
                    }
                    let before = control.statistics();
                    let mut index = 0;
                    let mut count = 0u64;
                    let operation = if paired { "info_arcs" } else { "info" };
                    group.bench_function(
                        BenchmarkId::new(
                            family,
                            format!("{policy_name}/{operation}/{working_set}"),
                        ),
                        |b| {
                            b.iter(|| {
                                let state = states[index].0;
                                index = (index + 1) % working_set;
                                count += 1;
                                black_box(requests.info(black_box(state)));
                                if paired {
                                    black_box(requests.arcs(black_box(state)));
                                }
                            });
                        },
                    );
                    let after = control.statistics();
                    let calls = count * if paired { 2 } else { 1 };
                    let misses = match policy {
                        SharedCachePolicy::NoCache => calls,
                        SharedCachePolicy::Lru { .. } if working_set > CAPACITY => count,
                        _ => 0,
                    };
                    assert_eq!(after.misses - before.misses, misses);
                    assert_eq!(after.hits - before.hits, calls - misses);
                    assert_eq!(after.faults - before.faults, 0);
                    assert_eq!(after.uncacheable_results - before.uncacheable_results, 0);
                    let (resident, evictions) = match policy {
                        SharedCachePolicy::CacheAll => (working_set, 0),
                        SharedCachePolicy::NoCache => (0, 0),
                        SharedCachePolicy::Lru { .. } => (CAPACITY, misses),
                    };
                    assert_eq!(after.resident_states, resident);
                    assert_eq!(after.evictions - before.evictions, evictions);
                }
            }
        }
    }
    group.finish();
}

criterion_group!(benches, retained_adapter_policies);
criterion_main!(benches);
