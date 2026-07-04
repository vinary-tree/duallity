//! Criterion benchmarks for duallity WFST construction and lazy expansion.
//!
//! These establish the empirical baseline the optimization campaign never had:
//! every prior pass was driven by code inspection and static risk signals, with
//! no runtime measurement. The benchmarks here exercise the two operations that
//! dominate real usage and that the campaign spent most of its effort on:
//!
//! 1. **Construction** — `Variant::new(&dict, query, max_distance)`, which seeds
//!    the registries and precomputes the query-side automaton.
//! 2. **Lazy BFS expansion** — repeatedly calling `transitions_lazy(state)` while
//!    walking the reachable product-state graph. This is the hot path that the
//!    registry-batching, lock-scope, preallocation, and state-encoding passes all
//!    targeted.
//!
//! The four non-phonetic variants are covered (the phonetic variants require the
//! `phonetic-rules` feature and rule configuration; they can be added later):
//! standard `LevenshteinWfst`, `UniversalLevenshteinWfst`, `GeneralizedWfst`, and
//! `WallBreakerWfst`.
//!
//! The dictionary is generated deterministically (no RNG, no external corpus) so
//! measurements are reproducible across runs and machines. For stable numbers,
//! pin to isolated cores at a fixed frequency, e.g.
//!
//! ```sh
//! taskset -c 2,3 cargo bench --bench wfst_expansion
//! ```

use std::collections::{HashSet, VecDeque};
use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};

use duallity::{
    GeneralizedWfst, LazyWfst, LevenshteinWfst, StateId, TropicalWeight, UniversalLevenshteinWfst,
    WallBreakerWfst, Wfst,
};
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use libdictenstein::scdawg::Scdawg;
use liblevenshtein::transducer::universal::Standard;
use liblevenshtein::transducer::OperationSet;

/// Consonants used for even character slots (21 letters).
const CONSONANTS: &[u8] = b"bcdfghjklmnpqrstvwxyz";
/// Vowels used for odd character slots (5 letters).
const VOWELS: &[u8] = b"aeiou";

/// Deterministically map an index to a pronounceable CVCV… word.
///
/// The length alternates over 5–7 characters and the letters form a mixed-radix
/// (alternating 21/5) encoding of `index`, so distinct indices yield distinct
/// words for every index range used here (different lengths never collide; equal
/// lengths are injective while the index stays below the radix product, which is
/// ≥ 231 525 even for the shortest words — far above the corpus sizes below).
fn word_for(index: usize) -> String {
    let len = 5 + (index % 3); // 5..=7 characters
    let mut remaining = index;
    let mut word = String::with_capacity(len);
    for slot in 0..len {
        let alphabet = if slot % 2 == 0 { CONSONANTS } else { VOWELS };
        word.push(alphabet[remaining % alphabet.len()] as char);
        remaining /= alphabet.len();
    }
    word
}

/// Deterministic corpus shared by every dictionary backend.
fn make_terms(word_count: usize) -> Vec<String> {
    let mut terms: Vec<String> = Vec::with_capacity(word_count);
    for index in 0..word_count {
        terms.push(word_for(index));
    }
    terms
}

/// Build a deterministic DAWG dictionary of `word_count` distinct words (used by
/// the Levenshtein, universal, and generalized variants).
fn make_dictionary(word_count: usize) -> DynamicDawgChar<()> {
    DynamicDawgChar::<()>::from_terms(make_terms(word_count))
}

/// Build the same corpus as a suffix-compacted DAWG, which the WallBreaker
/// variant requires (its dictionary must implement `SubstringDictionary` with
/// bidirectional nodes).
fn make_scdawg(word_count: usize) -> Scdawg<()> {
    Scdawg::<()>::from_terms(make_terms(word_count))
}

/// A query one substitution away from a mid-corpus word, so fuzzy expansion
/// explores real near-match paths instead of dead-ending on an exact hit.
fn make_query(word_count: usize) -> String {
    let mut word = word_for(word_count / 2);
    let replacement = if word.ends_with('z') { 'y' } else { 'z' };
    word.pop();
    word.push(replacement);
    word
}

/// Breadth-first expansion of the lazy product-state graph, bounded by
/// `max_states` computed states. Returns the number of states expanded so the
/// caller can `black_box` a value that depends on the whole traversal.
fn expand_bfs<W>(wfst: &mut W, max_states: usize) -> usize
where
    W: Wfst<char, TropicalWeight> + LazyWfst<char, TropicalWeight>,
{
    let start = wfst.start();
    let mut visited: HashSet<StateId> = HashSet::new();
    let mut frontier: VecDeque<StateId> = VecDeque::new();
    visited.insert(start);
    frontier.push_back(start);

    let mut expanded = 0usize;
    while let Some(state) = frontier.pop_front() {
        if expanded >= max_states {
            break;
        }
        // Copy the target ids out so the mutable borrow of `wfst` ends before we
        // enqueue (transitions_lazy borrows `&mut self`).
        let targets: Vec<StateId> = wfst
            .transitions_lazy(state)
            .iter()
            .map(|transition| transition.to)
            .collect();
        expanded += 1;
        for target in targets {
            if visited.insert(target) {
                frontier.push_back(target);
            }
        }
    }
    expanded
}

/// Corpus sizes exercised by both benchmark groups.
const CORPUS_SIZES: [usize; 2] = [1_000, 10_000];
/// Edit distance used for every variant (a realistic fuzzy-match radius).
const MAX_DISTANCE: usize = 2;
/// Upper bound on states expanded per BFS iteration (keeps runtime bounded and
/// the traversal comparable across variants and corpus sizes).
const MAX_EXPANDED_STATES: usize = 2_000;

fn bench_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("construction");
    for &size in &CORPUS_SIZES {
        let dict = make_dictionary(size);
        let scdawg = make_scdawg(size);
        let query = make_query(size);

        group.bench_with_input(BenchmarkId::new("levenshtein", size), &size, |b, _| {
            b.iter(|| black_box(LevenshteinWfst::new(&dict, &query, MAX_DISTANCE)));
        });
        group.bench_with_input(BenchmarkId::new("universal", size), &size, |b, _| {
            b.iter(|| {
                black_box(UniversalLevenshteinWfst::<Standard, _>::new(
                    &dict,
                    &query,
                    MAX_DISTANCE as u8,
                ))
            });
        });
        group.bench_with_input(BenchmarkId::new("generalized", size), &size, |b, _| {
            b.iter(|| {
                black_box(GeneralizedWfst::new(
                    &dict,
                    &query,
                    MAX_DISTANCE as u8,
                    OperationSet::standard(),
                ))
            });
        });
        group.bench_with_input(BenchmarkId::new("wallbreaker", size), &size, |b, _| {
            b.iter(|| black_box(WallBreakerWfst::new(&scdawg, &query, MAX_DISTANCE)));
        });
    }
    group.finish();
}

fn bench_expansion(c: &mut Criterion) {
    let mut group = c.benchmark_group("expansion_bfs");
    // Expansion allocates and computes far more than construction; trim the
    // sampling budget so the whole suite stays in the low-minutes range.
    group.sample_size(30);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(5));

    for &size in &CORPUS_SIZES {
        let dict = make_dictionary(size);
        let scdawg = make_scdawg(size);
        let query = make_query(size);

        group.bench_with_input(BenchmarkId::new("levenshtein", size), &size, |b, _| {
            b.iter_batched(
                || LevenshteinWfst::new(&dict, &query, MAX_DISTANCE),
                |mut wfst| black_box(expand_bfs(&mut wfst, MAX_EXPANDED_STATES)),
                BatchSize::SmallInput,
            );
        });
        group.bench_with_input(BenchmarkId::new("universal", size), &size, |b, _| {
            b.iter_batched(
                || UniversalLevenshteinWfst::<Standard, _>::new(&dict, &query, MAX_DISTANCE as u8),
                |mut wfst| black_box(expand_bfs(&mut wfst, MAX_EXPANDED_STATES)),
                BatchSize::SmallInput,
            );
        });
        group.bench_with_input(BenchmarkId::new("generalized", size), &size, |b, _| {
            b.iter_batched(
                || {
                    GeneralizedWfst::new(
                        &dict,
                        &query,
                        MAX_DISTANCE as u8,
                        OperationSet::standard(),
                    )
                },
                |mut wfst| black_box(expand_bfs(&mut wfst, MAX_EXPANDED_STATES)),
                BatchSize::SmallInput,
            );
        });
        group.bench_with_input(BenchmarkId::new("wallbreaker", size), &size, |b, _| {
            b.iter_batched(
                || WallBreakerWfst::new(&scdawg, &query, MAX_DISTANCE),
                |mut wfst| black_box(expand_bfs(&mut wfst, MAX_EXPANDED_STATES)),
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

criterion_group!(benches, bench_construction, bench_expansion);
criterion_main!(benches);
