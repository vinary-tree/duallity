//! Every exported adapter uses one policy-controlled residency owner.
#![cfg(feature = "ffi")]

mod support;

use duallity::bindings::{create_wfst, WfstKind};
use libdictenstein::bindings::{BindingUnitDomain, DynamicDawgBinding};
use liblevenshtein::transducer::Algorithm;
use lling_llang::wfst::SharedCachePolicy;
use std::num::NonZeroUsize;
use std::sync::Barrier;
use support::wfst_walk::WfstView;

const KINDS: [WfstKind; 8] = [
    WfstKind::UniversalStandard,
    WfstKind::UniversalTransposition,
    WfstKind::UniversalMergeAndSplit,
    WfstKind::GeneralizedStandard,
    WfstKind::GeneralizedTransposition,
    WfstKind::GeneralizedMergeAndSplit,
    WfstKind::GeneralizedPhonetic,
    WfstKind::Fzf,
];

fn family_algorithms() -> impl Iterator<Item = (WfstKind, Algorithm)> {
    KINDS
        .into_iter()
        .map(|kind| (kind, Algorithm::Standard))
        .chain(
            [
                Algorithm::Standard,
                Algorithm::Transposition,
                Algorithm::MergeAndSplit,
            ]
            .into_iter()
            .map(|algorithm| (WfstKind::Levenshtein, algorithm)),
        )
}

#[test]
fn every_family_preserves_snapshot_language_through_eviction_clear_and_no_cache() {
    for (kind, algorithm) in family_algorithms() {
        let dictionary = DynamicDawgBinding::new(BindingUnitDomain::UnicodeScalar);
        for term in ["cat", "car", "cot", "dog"] {
            dictionary
                .insert_text(term.as_bytes(), None)
                .expect("insert");
        }
        let source = dictionary.resource();
        let wfst = unsafe { create_wfst(source.as_raw(), "cat", 2, algorithm, kind) }
            .expect("construct adapter");
        let retained = wfst.clone();
        let control = wfst.provider_cache().expect("sole exporter cache");
        let minimize = kind != WfstKind::Fzf;
        let expected = WfstView::new(wfst.as_raw()).language(minimize);
        assert!(!expected.is_empty(), "{kind:?}: nonempty fixture language");
        let warmed = control.statistics();
        assert!(warmed.resident_states > 0);
        assert_eq!(
            WfstView::new(retained.as_raw()).language(minimize),
            expected
        );
        assert_eq!(
            control.statistics().misses,
            warmed.misses,
            "{kind:?}: warm reuse"
        );

        // Mutation affects the live dictionary, not the captured resource.
        dictionary
            .insert_text(b"bat", None)
            .expect("live-source mutation");
        drop(source);
        drop(dictionary);
        drop(wfst);

        for policy in [
            SharedCachePolicy::NoCache,
            SharedCachePolicy::Lru {
                capacity: NonZeroUsize::new(1).expect("one"),
            },
            SharedCachePolicy::Lru {
                capacity: NonZeroUsize::new(2).expect("two"),
            },
            SharedCachePolicy::CacheAll,
        ] {
            control.set_policy(policy);
            assert_eq!(control.statistics().resident_states, 0);
            for _ in 0..2 {
                assert_eq!(
                    WfstView::new(retained.as_raw()).language(minimize),
                    expected,
                    "{kind:?}, {policy:?}: identical weighted language after source drop"
                );
                match policy {
                    SharedCachePolicy::NoCache => {
                        assert_eq!(control.statistics().resident_states, 0)
                    }
                    SharedCachePolicy::Lru { capacity } => {
                        assert_eq!(control.statistics().resident_states, capacity.get());
                        assert!(control.statistics().evictions > 0);
                        let view = WfstView::new(retained.as_raw());
                        let state = view.start();
                        view.expand(state).expect("read warms state");
                        let warmed = control.statistics().misses;
                        view.expand(state).expect("second read is cached");
                        assert_eq!(control.statistics().misses, warmed);
                    }
                    SharedCachePolicy::CacheAll => {
                        assert!(control.statistics().resident_states > 0)
                    }
                }
                control.clear();
            }
        }
    }
}

#[test]
fn concurrent_family_resources_preserve_language_during_cache_generation_changes() {
    const READERS: usize = 4;
    const TRAVERSALS: usize = 4;
    for (kind, algorithm) in family_algorithms() {
        for policy in [
            SharedCachePolicy::CacheAll,
            SharedCachePolicy::NoCache,
            SharedCachePolicy::Lru {
                capacity: NonZeroUsize::new(1).expect("one"),
            },
            SharedCachePolicy::Lru {
                capacity: NonZeroUsize::new(2).expect("two"),
            },
            SharedCachePolicy::Lru {
                capacity: NonZeroUsize::new(17).expect("cross a link-block boundary"),
            },
        ] {
            let dictionary = DynamicDawgBinding::new(BindingUnitDomain::UnicodeScalar);
            for term in ["cat", "car", "cot", "dog"] {
                dictionary
                    .insert_text(term.as_bytes(), None)
                    .expect("insert");
            }
            let source = dictionary.resource();
            // The independent oracle must not pre-discover the tested resource's
            // registry: simultaneous callers also exercise first-time discovery.
            let oracle = unsafe { create_wfst(source.as_raw(), "cat", 2, algorithm, kind) }
                .expect("oracle adapter");
            let minimize = kind != WfstKind::Fzf;
            let expected = WfstView::new(oracle.as_raw()).language(minimize);
            assert!(!expected.is_empty(), "{kind:?}: nonempty oracle language");
            drop(oracle);
            let resource = unsafe { create_wfst(source.as_raw(), "cat", 2, algorithm, kind) }
                .expect("cold concurrent adapter");
            let control = resource.provider_cache().expect("one shared cache owner");
            control.set_policy(policy);
            let retained: Vec<_> = (0..READERS).map(|_| resource.clone()).collect();
            dictionary.insert_text(b"bat", None).expect("live mutation");
            drop(resource);
            drop(source);
            drop(dictionary);

            let start = Barrier::new(READERS + 1);
            std::thread::scope(|scope| {
                let workers: Vec<_> = retained
                    .into_iter()
                    .map(|resource| {
                        let (start, expected) = (&start, &expected);
                        scope.spawn(move || {
                            start.wait();
                            let view = WfstView::new(resource.as_raw());
                            for _ in 0..TRAVERSALS {
                                assert_eq!(
                                    &view.language(minimize),
                                    expected,
                                    "{kind:?}, {algorithm:?}, {policy:?}: retained snapshot language"
                                );
                            }
                        })
                    })
                    .collect();
                start.wait();
                for _ in 0..TRAVERSALS {
                    control.clear();
                    control.set_policy(policy);
                    let stats = control.statistics();
                    match policy {
                        SharedCachePolicy::CacheAll => assert_eq!(stats.recency_records, 0),
                        SharedCachePolicy::NoCache => {
                            assert_eq!((stats.resident_states, stats.recency_records), (0, 0));
                        }
                        SharedCachePolicy::Lru { capacity } => {
                            assert!(stats.resident_states <= capacity.get());
                            assert_eq!(stats.recency_records, stats.resident_states);
                        }
                    }
                }
                for worker in workers {
                    worker.join().expect("retained dictionary-backed reader");
                }
            });
            let stats = control.statistics();
            assert_eq!((stats.faults, stats.uncacheable_results), (0, 0));
            assert!(stats.misses > 0, "cold resource must expand its source");
            assert_eq!(stats.clears, 1 + (TRAVERSALS * 2) as u64);
            control.clear();
            assert_eq!(control.statistics().resident_states, 0);
        }
    }
}
