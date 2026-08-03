//! Prefix-shared fzf DP versus independent per-term scoring.

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use duallity::{FzfConfig, FzfScorer, FzfStats};
use liblevenshtein::transducer::PrefixPruner;
use std::collections::BTreeMap;
use std::hint::black_box;

#[derive(Default)]
struct BuildNode {
    final_node: bool,
    edges: BTreeMap<char, usize>,
}

struct BenchTrieNode {
    nodes: Vec<BuildNode>,
}

impl BenchTrieNode {
    fn from_terms(terms: &[&str]) -> Self {
        let mut nodes = vec![BuildNode::default()];
        for term in terms {
            let mut index = 0;
            for character in term.chars() {
                let next = if let Some(&next) = nodes[index].edges.get(&character) {
                    next
                } else {
                    let next = nodes.len();
                    nodes.push(BuildNode::default());
                    nodes[index].edges.insert(character, next);
                    next
                };
                index = next;
            }
            nodes[index].final_node = true;
        }
        Self { nodes }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct TrieWalkStats {
    nodes_visited: usize,
    edges_enumerated: usize,
    subtrees_pruned: usize,
}

fn trie_scores(
    trie: &BenchTrieNode,
    query: &str,
    config: FzfConfig,
) -> (Vec<i32>, FzfStats, TrieWalkStats) {
    fn dfs(
        trie: &BenchTrieNode,
        node: usize,
        depth: usize,
        scorer: &mut FzfScorer,
        scores: &mut Vec<i32>,
        stats: &mut TrieWalkStats,
    ) {
        stats.nodes_visited += 1;
        if trie.nodes[node].final_node && scorer.permits_accept(&[]) {
            if let Some(score) = scorer.accept(&[]) {
                scores.push(score as i32);
            }
        }

        for (&unit, &child) in &trie.nodes[node].edges {
            stats.edges_enumerated += 1;
            let child_depth = depth + 1;
            if scorer.enter(unit, child_depth) {
                dfs(trie, child, child_depth, scorer, scores, stats);
            } else {
                stats.subtrees_pruned += 1;
            }
            scorer.leave(unit, child_depth);
        }
    }

    let mut scorer = FzfScorer::with_config(query, config).expect("fixed query is bounded");
    let mut scores = Vec::new();
    let mut stats = TrieWalkStats::default();
    dfs(trie, 0, 0, &mut scorer, &mut scores, &mut stats);
    (scores, scorer.stats(), stats)
}

fn corpus() -> Vec<&'static str> {
    include_str!("../tests/fixtures/fzf_real_paths.txt")
        .lines()
        .filter(|line| !line.is_empty())
        .collect()
}

fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("fzf_prefix_shared_dp");
    let terms = corpus();
    let size = terms.len();
    let dictionary = BenchTrieNode::from_terms(&terms);
    let corpus_max_candidate_chars = terms
        .iter()
        .map(|term| term.chars().count())
        .max()
        .expect("the checked-in path corpus is nonempty");
    let config = FzfConfig {
        top_k: 1,
        max_candidate_chars: corpus_max_candidate_chars,
        ..FzfConfig::default()
    };
    let query = "src";
    let scorer = FzfScorer::with_config(query, config).expect("fixed query is bounded");
    let flat_columns: usize = terms.iter().map(|term| term.chars().count()).sum();
    let (measured_scores, scorer_stats, trie_stats) = trie_scores(&dictionary, query, config);
    let measured_matches = measured_scores.len();
    let flat_scores: Vec<_> = terms
        .iter()
        .filter_map(|term| scorer.score(term).expect("corpus path is bounded"))
        .map(|matched| matched.score)
        .collect();
    assert_eq!(measured_scores.iter().max(), flat_scores.iter().max());
    eprintln!(
        "fzf-work corpus=checked-in-real-paths terms={size} corpus_max_chars={corpus_max_candidate_chars} configured_max_chars={} flat_columns={flat_columns} flat_matches={} trie_columns={} nodes={} edges={} candidates={} yielded_matches={} subtrees_pruned={} score_pruned={} length_pruned={} bounds={}",
        config.max_candidate_chars,
        flat_scores.len(),
        scorer_stats.columns_computed,
        trie_stats.nodes_visited,
        trie_stats.edges_enumerated,
        scorer_stats.candidates_scored,
        measured_matches,
        trie_stats.subtrees_pruned,
        scorer_stats.score_bound_prefixes_pruned,
        scorer_stats.length_prefixes_pruned,
        scorer_stats.upper_bounds_computed,
    );
    group.throughput(Throughput::Elements(size as u64));

    group.bench_function("flat_checked_in_real_paths", |b| {
        b.iter(|| {
            let scores: Vec<_> = terms
                .iter()
                .filter_map(|term| scorer.score(black_box(term)).unwrap())
                .collect();
            black_box(scores)
        });
    });

    group.bench_function("trie_checked_in_real_paths", |b| {
        b.iter(|| {
            let (scores, _, _) = trie_scores(&dictionary, query, config);
            black_box(scores)
        });
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
