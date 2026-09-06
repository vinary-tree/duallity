//! Foreign callback isolation through native dictionary decorators and threads.

#![cfg(feature = "ffi")]

mod support;

use duallity::bindings::{ResourceDictionary, ResourceNode};
use duallity::{GeneralizedWfst, LazyWfst, Wfst};
use libdictenstein::{Dictionary, DictionaryNode};
use liblevenshtein::transducer::{OperationSet, OperationSetBuilder, OperationType};
use std::sync::Arc;
use support::counting_dictionary::{CountingDictionary, Misbehavior};
use vinary_tree_interop::{VtStatus, VT_RECOMMENDED_EDGE_BATCH};

type EdgeHook = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone)]
struct DecoratedNode {
    inner: ResourceNode,
    on_edges: EdgeHook,
}

impl DictionaryNode for DecoratedNode {
    type Unit = char;
    type SnapshotCursor = ();
    type SnapshotGraphValueHandle = ();
    fn is_final(&self) -> bool {
        self.inner.is_final()
    }
    fn transition(&self, label: char) -> Option<Self> {
        self.inner.transition(label).map(|inner| Self {
            inner,
            on_edges: Arc::clone(&self.on_edges),
        })
    }
    fn edges(&self) -> Box<dyn Iterator<Item = (char, Self)> + '_> {
        (self.on_edges)();
        Box::new(self.inner.edges().map(|(label, inner)| {
            (
                label,
                Self {
                    inner,
                    on_edges: Arc::clone(&self.on_edges),
                },
            )
        }))
    }
}

#[derive(Clone)]
struct DecoratedDictionary(DecoratedNode);

impl Dictionary for DecoratedDictionary {
    type Node = DecoratedNode;
    fn root(&self) -> Self::Node {
        self.0.clone()
    }
    fn len(&self) -> Option<usize> {
        None
    }
}

fn capture(fixture: &CountingDictionary) -> ResourceDictionary {
    let raw = fixture.resource();
    let dictionary = unsafe { ResourceDictionary::capture(raw) }.expect("capture");
    duallity::ffi::duallity_resource_release(raw);
    dictionary
}

fn faulty() -> ResourceDictionary {
    capture(&CountingDictionary::misbehaving(
        VT_RECOMMENDED_EDGE_BATCH + 4,
        Misbehavior::LatePageFailure,
    ))
}

fn first_leaf(dictionary: &ResourceDictionary) -> ResourceNode {
    // One successful page is enough to own a leaf; only the later page fails.
    dictionary.root().edges().next().expect("first leaf").1
}

fn leaf_wfst(node: ResourceNode, on_edges: EdgeHook) -> GeneralizedWfst<DecoratedDictionary> {
    GeneralizedWfst::new(
        &DecoratedDictionary(DecoratedNode {
            inner: node,
            on_edges,
        }),
        "",
        1,
        OperationSetBuilder::new()
            .with_operation(OperationType::new(1, 0, 1.0, "delete"))
            .build(),
    )
}

#[test]
fn native_decorators_preserve_transactional_foreign_failure_without_diagnostic_overrides() {
    let source = faulty();
    let dict = DecoratedDictionary(DecoratedNode {
        inner: source.root(),
        on_edges: Arc::new(|| {}),
    });
    let mut wfst = GeneralizedWfst::new(&dict, "a", 1, OperationSet::standard());
    for _ in 0..3 {
        assert!(wfst.try_transitions(0).is_err());
        assert_eq!(wfst.num_states(), 1);
        assert_eq!(wfst.computed_states(), 0);
    }
}

#[test]
fn handled_nested_callbacks_and_computations_do_not_poison_the_outer_expansion() {
    for same_provider in [false, true] {
        for mode in 0..3 {
            let source = faulty();
            let outer_source = if same_provider {
                source.clone()
            } else {
                capture(&CountingDictionary::high_degree(1))
            };
            let leaf = first_leaf(&outer_source);
            let mut wfst = leaf_wfst(
                leaf,
                Arc::new(move || match mode {
                    0 => {
                        let result = source.with_checked(|| Ok(source.root().edges().count()));
                        assert_eq!(result, Err(VtStatus::IoError));
                    }
                    1 | 2 => {
                        let mut inner =
                            GeneralizedWfst::new(&source, "a", 1, OperationSet::standard());
                        if mode == 1 {
                            assert!(inner.try_transitions(0).is_err());
                        } else {
                            let result = source.with_checked(|| {
                                inner
                                    .try_transitions(0)
                                    .map(|arcs| arcs.len())
                                    .map_err(|_| VtStatus::ProviderError)
                            });
                            assert_eq!(result, Err(VtStatus::IoError));
                        }
                        assert_eq!(inner.num_states(), 1);
                        assert_eq!(inner.computed_states(), 0);
                    }
                    _ => unreachable!(),
                }),
            );
            assert!(wfst
                .try_transitions(0)
                .expect("nested failure was handled")
                .is_empty());
            assert!(wfst.is_final(0));
            assert_eq!(wfst.computed_states(), 1);
        }
    }
}

#[test]
fn an_unrelated_preexisting_fault_does_not_reject_a_healthy_computation() {
    let source = faulty();
    let healthy = capture(&CountingDictionary::high_degree(1));
    let mut wfst = leaf_wfst(first_leaf(&healthy), Arc::new(|| {}));
    let result = source.with_checked(|| {
        let _ = source.root().edges().count();
        assert!(wfst
            .try_transitions(0)
            .expect("owns an independent sink")
            .is_empty());
        assert!(wfst.is_final(0));
        Ok(())
    });
    assert_eq!(
        result,
        Err(VtStatus::IoError),
        "checked scope still latches its own failure"
    );
    assert_eq!(wfst.computed_states(), 1);
}

#[test]
fn concurrent_callbacks_from_the_same_provider_have_independent_results() {
    let source = faulty();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let good_barrier = Arc::clone(&barrier);
    let mut good = leaf_wfst(
        first_leaf(&source),
        Arc::new(move || {
            good_barrier.wait();
        }),
    );
    let mut bad = GeneralizedWfst::new(
        &DecoratedDictionary(DecoratedNode {
            inner: source.root(),
            on_edges: Arc::new(move || {
                barrier.wait();
            }),
        }),
        "a",
        1,
        OperationSet::standard(),
    );
    let successful = std::thread::spawn(move || {
        assert!(good
            .try_transitions(0)
            .expect("other thread's fault is isolated")
            .is_empty());
        assert!(good.is_final(0));
    });
    let failing = std::thread::spawn(move || {
        assert!(bad.try_transitions(0).is_err());
        assert_eq!(bad.num_states(), 1);
        assert_eq!(bad.computed_states(), 0);
    });
    successful.join().expect("successful invocation");
    failing.join().expect("failing invocation");
}
