//! Reentrancy and concurrent-reconciliation tests against the real registries.

use super::*;
use libdictenstein::SyncStrategy;
use liblevenshtein::transducer::{OperationSetBuilder, OperationType};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

type Observer = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct Hooks {
    observer: Mutex<Option<Observer>>,
    on_drop: Mutex<Option<Observer>>,
    calls: AtomicUsize,
}

impl Hooks {
    fn check(&self) {
        let observer = self.observer.lock().expect("observer lock").clone();
        if let Some(observer) = observer {
            self.calls.fetch_add(1, Ordering::Relaxed);
            observer();
        }
    }
}

struct ProbeNode {
    depth: u8,
    hooks: Arc<Hooks>,
}

impl Clone for ProbeNode {
    fn clone(&self) -> Self {
        self.hooks.check();
        Self {
            depth: self.depth,
            hooks: Arc::clone(&self.hooks),
        }
    }
}

impl Drop for ProbeNode {
    fn drop(&mut self) {
        self.hooks.check();
        let callback = self.hooks.on_drop.lock().expect("drop callback").take();
        if let Some(callback) = callback {
            callback();
        }
    }
}

impl DictionaryNode for ProbeNode {
    type Unit = char;
    type SnapshotCursor = ();
    type SnapshotGraphValueHandle = ();
    fn is_final(&self) -> bool {
        self.depth == 2
    }
    fn transition(&self, label: char) -> Option<Self> {
        (label == 'a' && self.depth < 2).then(|| Self {
            depth: self.depth + 1,
            hooks: Arc::clone(&self.hooks),
        })
    }
    fn edges(&self) -> Box<dyn Iterator<Item = (char, Self)> + '_> {
        Box::new(self.transition('a').map(|node| ('a', node)).into_iter())
    }
}

#[derive(Clone)]
struct ProbeDictionary(Arc<Hooks>);

impl Dictionary for ProbeDictionary {
    type Node = ProbeNode;
    fn root(&self) -> Self::Node {
        ProbeNode {
            depth: 0,
            hooks: Arc::clone(&self.0),
        }
    }
    fn len(&self) -> Option<usize> {
        Some(1)
    }
    fn sync_strategy(&self) -> SyncStrategy {
        SyncStrategy::Persistent
    }
}

fn observed_wfst(state_limit: usize) -> GeneralizedWfst<ProbeDictionary> {
    let hooks = Arc::new(Hooks::default());
    let wfst = GeneralizedWfst::try_new_with_limits(
        &ProbeDictionary(Arc::clone(&hooks)),
        "",
        2,
        OperationSetBuilder::new()
            .with_operation(OperationType::new(1, 0, 1.0, "delete_one"))
            .with_operation(OperationType::new(2, 0, 1.0, "delete_two"))
            .build(),
        GeneralizedWfstLimits {
            max_retained_wfst_states: state_limit,
            ..Default::default()
        },
    )
    .expect("valid bounded probe");
    let nodes = Arc::downgrade(&wfst.node_registry);
    let states = Arc::downgrade(&wfst.state_registry);
    *hooks.observer.lock().expect("install observer") = Some(Arc::new(move || {
        if let Some(nodes) = nodes.upgrade() {
            assert!(
                nodes.try_write().is_ok(),
                "node Clone/Drop under node registry lock"
            );
        }
        if let Some(states) = states.upgrade() {
            assert!(
                states.try_write().is_ok(),
                "node Clone/Drop under state registry lock"
            );
        }
    }));
    wfst
}

#[test]
fn node_lifecycle_is_outside_locks_on_success_reuse_and_rollback() {
    for state_limit in [1, 100] {
        let mut wfst = observed_wfst(state_limit);
        for _ in 0..3 {
            let result = wfst.try_transitions(0);
            if state_limit == 1 {
                assert!(result.is_err());
                assert_eq!(crate::read_lock(&wfst.node_registry).len(), 1);
                assert_eq!(wfst.num_states(), 1);
            } else {
                assert_eq!(result.expect("full expansion").len(), 2);
                let _ = wfst.final_weight_for_state(1);
            }
            wfst.clear_cache();
        }
        assert!(wfst.dictionary.0.calls.load(Ordering::Relaxed) > 0);
    }
}

fn stage_then_publish_other_clone(
    wfst: &GeneralizedWfst<ProbeDictionary>,
) -> (ExpansionStaging<ProbeNode>, Vec<PendingOperationArc>) {
    let mut budget = ExpansionBudget::new(wfst.limits, None);
    let mut staging = ExpansionStaging::new();
    let paths = wfst
        .dictionary_paths_exact_chars(0, 2, &mut staging, &mut budget, &|| Ok(()))
        .expect("staging");
    assert!(matches!(
        wfst.expand_state(0),
        StateExpansion::Expanded { .. }
    ));
    let arcs = paths
        .into_iter()
        .map(|path| PendingOperationArc {
            target_node: path.target_node,
            query_byte_pos: 0,
            cost: 2,
            input: LabelBuffer::new(),
            output: path.output,
            weight: 2.0,
        })
        .collect();
    (staging, arcs)
}

fn registry_counts(wfst: &GeneralizedWfst<ProbeDictionary>) -> (usize, usize) {
    (
        crate::read_lock(&wfst.node_registry).len(),
        wfst.num_states(),
    )
}

#[test]
fn retirement_cancellation_and_retry_exhaustion_leave_outer_ids_unpublished() {
    for cancel_on_drop in [false, true] {
        let wfst = observed_wfst(100);
        let (staging, arcs) = stage_then_publish_other_clone(&wfst);
        let before = registry_counts(&wfst);
        let cancellation = Arc::new(CancellationToken::new());
        if cancel_on_drop {
            let token = Arc::clone(&cancellation);
            *wfst
                .dictionary
                .0
                .on_drop
                .lock()
                .expect("install cancellation") = Some(Arc::new(move || {
                token.cancel(lling_llang::prelude::CancellationReason::Requested);
            }));
        }
        let limits = GeneralizedWfstLimits {
            // Two nodes fit the first reconciliation pass; a retry cannot start.
            max_work_units_per_expansion: if cancel_on_drop { 1000 } else { 2 },
            ..wfst.limits
        };
        let mut budget = ExpansionBudget::new(limits, Some(&cancellation));
        let result = wfst.commit_expansion(0, staging, arcs, &mut budget, &|| Ok(()));
        if cancel_on_drop {
            assert!(matches!(result, Err(ExpansionError::Cancelled(_))));
        } else {
            assert!(matches!(result, Err(ExpansionError::Failure(_))));
        }
        assert_eq!(registry_counts(&wfst), before);
        assert_eq!(wfst.computed_states(), 0);
    }
}

#[test]
fn retirement_can_reenter_and_publish_before_outer_ids_are_recomputed() {
    let wfst = observed_wfst(100);
    let (staging, arcs) = stage_then_publish_other_clone(&wfst);
    let before = registry_counts(&wfst);
    let nested = wfst.clone();
    *wfst.dictionary.0.on_drop.lock().expect("install reentry") = Some(Arc::new(move || {
        assert_eq!(
            registry_counts(&nested),
            before,
            "outer transaction has not published yet"
        );
        assert!(matches!(
            nested.expand_state(1),
            StateExpansion::Expanded { .. }
        ));
    }));
    let mut budget = ExpansionBudget::new(wfst.limits, None);
    let transitions = wfst
        .commit_expansion(0, staging, arcs, &mut budget, &|| Ok(()))
        .expect("reconciled after reentry");
    assert_eq!(registry_counts(&wfst), (before.0, before.1 + 2));
    let RegisteredState::Emit(emit) = wfst
        .registered_state(transitions[0].to)
        .expect("whole chain")
    else {
        panic!("continuation");
    };
    let RegisteredState::Product(target) = wfst.registered_state(emit.next).expect("target") else {
        panic!("product");
    };
    assert_eq!(target.cost, 2);
    assert!(wfst.is_final(emit.next));
}

#[cfg(feature = "ffi")]
#[path = "../tests/support/counting_dictionary.rs"]
mod counting_dictionary;

#[cfg(feature = "ffi")]
#[test]
fn retired_node_foreign_callback_failure_precedes_publication() {
    use crate::bindings::{DictionaryComputationScope, ResourceDictionary};
    use counting_dictionary::{CountingDictionary, Misbehavior};
    let fixture = CountingDictionary::misbehaving(300, Misbehavior::LatePageFailure);
    let raw = fixture.resource();
    let provider = unsafe { ResourceDictionary::capture(raw) }.expect("capture");
    crate::ffi::duallity_resource_release(raw);
    let wfst = observed_wfst(100);
    let (staging, arcs) = stage_then_publish_other_clone(&wfst);
    let before = registry_counts(&wfst);
    *wfst
        .dictionary
        .0
        .on_drop
        .lock()
        .expect("install foreign destructor callback") = Some(Arc::new(move || {
        let _ = provider.root().edges().count();
    }));
    let scope = DictionaryComputationScope::enter().expect("owned invocation");
    let mut budget = ExpansionBudget::new(wfst.limits, None);
    assert!(wfst
        .commit_expansion(0, staging, arcs, &mut budget, &|| scope.check())
        .is_err());
    assert!(scope.check().is_err());
    assert_eq!(registry_counts(&wfst), before);
    assert_eq!(wfst.computed_states(), 0);
}

#[test]
fn concurrently_staged_exact_fit_reconciles_before_retained_limits() {
    let dictionary = libdictenstein::dynamic_dawg::char::DynamicDawgChar::<()>::from_terms(["aa"]);
    let wfst = GeneralizedWfst::try_new_with_limits(
        &dictionary,
        "",
        1,
        OperationSetBuilder::new()
            .with_operation(OperationType::new(2, 0, 1.0, "delete_two"))
            .build(),
        GeneralizedWfstLimits {
            max_retained_dictionary_nodes: 3,
            max_retained_wfst_states: 3,
            ..Default::default()
        },
    )
    .expect("exact fit");
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let wfst = wfst.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let mut budget = ExpansionBudget::new(wfst.limits, None);
                let mut staging = ExpansionStaging::new();
                let paths = wfst
                    .dictionary_paths_exact_chars(0, 2, &mut staging, &mut budget, &|| Ok(()))
                    .expect("stage paths");
                assert_eq!(staging.nodes.len(), 2);
                // Both workers must have staged the same identities before either commits.
                barrier.wait();
                let arcs = paths
                    .into_iter()
                    .map(|path| PendingOperationArc {
                        target_node: path.target_node,
                        query_byte_pos: 0,
                        cost: 1,
                        input: LabelBuffer::new(),
                        output: path.output,
                        weight: 1.0,
                    })
                    .collect();
                wfst.commit_expansion(0, staging, arcs, &mut budget, &|| Ok(()))
                    .expect("duplicate identities fit without overcounting")[0]
                    .to
            })
        })
        .collect();
    let ids: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().expect("worker"))
        .collect();
    assert_eq!(ids[0], ids[1]);
    assert_eq!(crate::read_lock(&wfst.node_registry).len(), 3);
    assert_eq!(wfst.num_states(), 3);
}
