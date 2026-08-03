//! Prefix-shared fzf DP versus independent per-term scoring.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use duallity::{FzfConfig, FzfScorer};
use libdictenstein::Dictionary;
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use liblevenshtein::transducer::SubsequenceQueryIterator;
use std::hint::black_box;

fn corpus(size: usize) -> Vec<String> {
    (0..size)
        .map(|index| {
            format!(
                "workspace/crate_{:03}/src/module_{:03}/fuzzy_state_{index:06}.rs",
                index % 64,
                index % 256,
            )
        })
        .collect()
}

fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("fzf_prefix_shared_dp");
    for size in [1_000, 10_000] {
        let terms = corpus(size);
        let dictionary = DynamicDawgChar::<()>::from_terms(terms.iter().map(String::as_str));
        let config = FzfConfig {
            top_k: 20,
            ..FzfConfig::default()
        };
        let scorer = FzfScorer::with_config("wcsfs", config).expect("fixed query is bounded");
        let flat_columns: usize = terms.iter().map(|term| term.chars().count()).sum();
        let mut measured = SubsequenceQueryIterator::with_pruner(
            dictionary.root(),
            scorer.query_units(),
            FzfScorer::with_config("wcsfs", config).expect("fixed query is bounded"),
        );
        let measured_matches = measured.by_ref().count();
        let measured_stats = measured.pruner().stats();
        eprintln!(
            "fzf-work size={size} flat_columns={flat_columns} trie_columns={} candidates={} matches={} prefixes_pruned={}",
            measured_stats.columns_computed,
            measured_stats.candidates_scored,
            measured_matches,
            measured_stats.prefixes_pruned,
        );
        group.throughput(Throughput::Elements(size as u64));

        group.bench_with_input(BenchmarkId::new("flat", size), &size, |b, _| {
            b.iter(|| {
                let scores: Vec<_> = terms
                    .iter()
                    .filter_map(|term| scorer.score(black_box(term)).unwrap())
                    .collect();
                black_box(scores)
            });
        });

        group.bench_with_input(BenchmarkId::new("trie", size), &size, |b, _| {
            b.iter(|| {
                let scorer = FzfScorer::with_config("wcsfs", config).unwrap();
                let scores: Vec<_> = SubsequenceQueryIterator::with_pruner(
                    dictionary.root(),
                    scorer.query_units(),
                    scorer,
                )
                .collect();
                black_box(scores)
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
