# fzf capacity-bound verification and benchmark ledger

## Question and method

The question is whether a prefix-sensitive upper bound can prune fzf
`FuzzyMatchV2` search without changing exact top-`` $`k`$ `` results. The bound
uses the configured maximum candidate length as explicit remaining capacity and
takes the maximum of feasible completed, active, and unstarted local-alignment
alternatives. The benchmark compares independent per-path scoring with a
balanced depth-first traversal of an acyclic, benchmark-only character trie.
Trie construction is outside both timed loops.

The corpus is `tests/fixtures/fzf_real_paths.txt`: 42 paths copied from the
tracked liblevenshtein, lling-llang, and duallity repository trees. The fixed
query is `src`, the scheme is the default fzf scheme, `top_k` is 1, and the
candidate ceiling is the measured corpus maximum of 59 Unicode scalar values.
The benchmark asserts that trie and flat maximum scores are identical before
timing.

## Environment and command

- Date: 2026-08-02, America/New_York.
- Revision before the result commit: `95aae6b`.
- Rust: `rustc 1.97.1 (8bab26f4f 2026-07-14)`.
- Host: Linux 7.1.5 x86-64; AMD Ryzen Threadripper PRO 5975WX, 32 cores.
- Command: `cargo bench --bench fzf_trie_vs_flat -- --sample-size 10 --measurement-time 1 --warm-up-time 1 --noplot`.

## Observations

| Metric | Flat | Prefix-shared DFS |
|---|---:|---:|
| DP columns | 1,289 | 910 |
| Trie nodes visited | not applicable | 909 |
| Trie edges enumerated | not applicable | 910 |
| Exact candidates scored | 28 | 26 |
| Score-pruned subtrees | not applicable | 2 |
| Length-pruned subtrees | not applicable | 0 |
| Median time | 27.796 microseconds | 42.913 microseconds |

The trie evaluates 29.4% fewer DP columns and the score bound is observably
non-vacuous. On this small 42-path corpus, allocation and balanced-DFS overhead
outweigh the saved recurrence work, so the trie is 1.54 times slower. This is a
negative latency result, not evidence of a speedup. Larger production corpora
and backend-specific allocation work remain measurement tasks; the semantic
claim is limited to exactness and avoided DP work.

## Verification evidence

- Fifteen score constants from fzf's published algorithm tests agree with both
  the independent batch oracle and duallity's incremental scorer.
- Every query/path pair in the checked-in corpus agrees score-for-score between
  those two implementations.
- Property tests establish exact trie/brute-force top-`` $`k`$ `` equality,
  descendant-score domination, and parent-to-child upper-bound monotonicity.
- A targeted example observes score pruning with zero length-limit pruning.
- Formal artifacts derive successor domination from the gap, match, completed,
  and newly-started recurrence cases rather than assuming the desired bound.

## Reproduction and falsification

Run `cargo test --all-features`, then the benchmark command above. A mismatch in
the asserted flat/trie maximum, any published fixture, any corpus pair, or any
generated descendant invalidates soundness. A run with
`score_bound_prefixes_pruned == 0` invalidates the benchmark's non-vacuity
observation but does not by itself invalidate bound soundness.
