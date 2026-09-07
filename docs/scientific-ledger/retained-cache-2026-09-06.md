# Retained exporter-cache qualification — 6 September 2026

## Question and scope

Does one policy-controlled expansion cache preserve the exported automata while
avoiding duplicate native-wrapper and exporter cache ownership? Which policies
pay for their metadata cost when expanding real dictionary-backed states?

The exported **weighted finite-state transducer (WFST)** describes states by
their validity, finality, final weight and ordered outgoing arcs. An **expansion**
is that complete immutable result. Cache eviction may discard the expansion,
but must preserve the state identifier and the captured dictionary revision.
The **application binary interface (ABI)** exposes information and arc-page
callbacks separately, so one logical consumer request can cause multiple
lookups or computations.

Before this change, lling-llang already retained exported expansions in an
unbounded lock-protected map. The defect was split policy ownership, not a lack
of warm reuse. Duallity now borrows the direct state source inside the checked
dictionary callback scope; the exporter owns the sole expansion cache on that
path. Native wrapper APIs retain their existing policies. Semantic registries
and composition-consumer caches are separate and are not cleared by exporter
cache controls.

## Correctness evidence

The [policy suite](../../tests/ffi_cache_policies.rs) compares complete accepted
weighted languages before and after repeated eviction, policy replacement,
clear, dictionary mutation and source-resource destruction. It exercises all
nine exported kinds, including the three classic edit algorithms: eleven
family/algorithm cases in total. Cloned resources retain the same cache owner.
CacheAll, true NoCache, and exact least-recently-used (LRU) capacities one and
two obey their residency, warm-reuse and eviction counters.

The [native zero-policy suite](../../tests/cache_policy.rs) exercises the existing
native boundary separately from that positive-capacity exporter policy. A
shared test checks the three classic edit algorithms, three universal variants
and generalized standard: zero selects the configured fallback, three distinct
expansions evict at bound two, a hit changes the retained state when shrinking to
one, and a zero fallback remains clamped to one. Recomputed finality, weight and
ordered arcs equal the CacheAll results, including after clear.

FZF deliberately has a different native contract: zero retains only the current
transient expansion. Alternating states recomputes them, while an immediate
same-state request reuses the transient. The test verifies both behavior and
state outputs. FZF's native `computed_states()` counts cumulative computations,
not current residents; clear leaves that lifetime count intact. Treating it as
a residency counter would give an incorrect test oracle. The targeted suite's
seven tests pass in debug and release, including the existing WallBreaker check;
logs are `shared-cache-zero-wrapper-debug.log` and
`shared-cache-zero-wrapper-release.log`.

`SharedCachePolicy::Lru` continues to require a positive `NonZeroUsize`. A future
versioned configurable ABI is responsible for interpreting raw zero using the
chosen family's contract. No family-blind zero-to-NoCache conversion or new
constructor is introduced as part of this ownership change.

After adding these wrapper regressions, the complete duallity all-feature suite
passes all 426 tests in debug and release, zero skips, and strict all-target
Clippy passes. These supersede the earlier 421-test qualification below without
changing production cache code. Logs are `shared-cache-zero-full-debug.log`,
`shared-cache-zero-full-release.log` and `shared-cache-zero-clippy.log`.

The consolidated exporter-cache implementation passes all 421 duallity tests
with all features in both debug and release, with zero skips, and strict
all-target/all-feature Clippy. Logs are under `target/agent-logs`:

- `shared-cache-duallity-consolidated-debug.log`;
- `shared-cache-duallity-consolidated-release.log`;
- `shared-cache-duallity-consolidated-clippy.log`.

These are integration-worktree results, not a published-artifact claim. The
validation manifest temporarily selects the lling-llang integration and
liblevenshtein variant-semantics worktrees. Canonical sibling paths must be
restored after integration and validated before committing the final graph.

After the liblevenshtein feature merges, the validation manifest again selects
the canonical `../liblevenshtein-rust` dependency at commit `919c9935`. The
lling-llang integration path remains temporary; this is not yet the final
canonical source graph. Revalidation uses the separate
`target/canonical-graph` directory so fixed-name native libraries from different
dependency graphs cannot overwrite one another. All 426 all-feature debug
tests pass with zero skips, followed by strict all-feature/all-target Clippy.
The logs are `shared-cache-canonical-liblevenshtein-debug.log` and
`shared-cache-canonical-liblevenshtein-clippy.log`. The liblevenshtein worktree
also contains another owner's six-line rustdoc indentation change in
`src/ffi/distance.rs`; it was preserved untouched, so the commit alone is not a
claim of a clean dependency tree.

Release-mode revalidation of that same graph also passes all 426 tests with
zero skips. The optimized real-adapter benchmark then passes all 48
correctness-only cases, retaining the same state and arc counts listed below.
Logs are `shared-cache-canonical-liblevenshtein-release.log` and
`shared-cache-canonical-liblevenshtein-bench-smoke.log`. The release build used
four Cargo jobs in a 12 GiB, no-swap systemd scope with a four-CPU aggregate
quota. No latency samples were taken during compilation or these checks.

## Real-adapter benchmark design

### Concurrent retained-resource qualification on 7 September

The policy suite now also runs four simultaneous consumers through cloned
resources for every one of the eleven exported family/algorithm cases under
CacheAll, NoCache and exact LRU capacities 1, 2, and 17: 55 configurations.
Each cold tested resource is compared with a separately constructed serial oracle, so
the oracle cannot pre-discover the tested resource's semantic registry. After
mutating the live dictionary, the test drops the original dictionary, source
and adapter handles. The retained readers each traverse the complete weighted
language four times while a controller clears and replaces cache generations.
The test checks exact language equality, bounded coherent residency and recency
counts, and absence of provider faults or rejected valid states. It does not
assume that a particular callback overlaps a particular clear.

Both policy tests pass in debug and release. The complete all-feature workspace
then passes 427 tests in each profile with zero skips, followed by strict
all-target/all-feature Clippy. Logs use
`shared-cache-concurrent-family-{debug,release,clippy}.log` and
`shared-cache-concurrent-expanded-{debug,release,clippy}.log` in
`target/agent-logs`. These extend the prior 426-test qualification; they are
correctness results, not a concurrency-throughput or published-artifact claim.

### Workload and operation boundaries

The registered [benchmark](../../benches/ffi_cache_benchmarks.rs) builds the
4,096 distinct four-unit terms over the eight ASCII letters `a` through `h` in
a Unicode-scalar dictionary. Every adapter receives query `abcd` and edit bound
two. FZF uses its existing scoring semantics, rather than pretending its score
is an edit distance. Construction, snapshot capture and discovery are outside
timing. Discovery takes the first 65 distinct reachable states in breadth-first
order; it is not an exhaustive query traversal.

| Adapter | Selected states | Outgoing arcs across selected states | Maximum state degree |
|---|---:|---:|---:|
| Classic standard | 65 | 641 | 17 |
| Universal standard | 65 | 449 | 9 |
| Generalized standard | 65 | 711 | 18 |
| FZF | 65 | 520 | 8 |

For each adapter, CacheAll, NoCache and LRU64 replay either 64 or 65 states in a
continuous cycle. The cursor survives Criterion warmup and sample boundaries.
Every timed operation is either `state_info` alone or `state_info` immediately
followed by `state_arcs` for the same ID. One pre-sized reusable arc buffer holds
the entire state, avoiding consumer-buffer allocation in the timed loop.

| Policy and working set | Info-only operation | Paired info/arc operation |
|---|---|---|
| CacheAll, either set | one hit | two hits |
| NoCache, either set | one computation | two computations |
| LRU64, 64 states | one non-MRU hit | one non-MRU hit, then one MRU hit |
| LRU64, 65 states | one computation and eviction | one computation and eviction, then one MRU hit |

Here **MRU** means most recently used. Exact LRU moves a non-MRU hit to the
tail; the immediately following same-state call already addresses the tail.
Every case asserts the exact hit, miss and eviction deltas after Criterion
finishes, plus bounded residency and absence of faults or rejected admissions.
Before measurement, each policy's complete state fields, ordered labels,
targets, weight bits and reserved arc bytes must equal the discovered fixture.

All 48 cases pass Criterion's correctness-only `--test` mode. That mode is a
smoke check, not a performance result. Its output is retained in
`target/agent-logs/shared-cache-real-adapter-benchmark-smoke.log`.

## Interpretation and reproduction

This isolates retained expansion/recomputation costs for already-discovered
states. It does not measure constructor cost, first-time registry discovery,
whole-query latency, cross-thread scaling, or a Rust-versus-Java speedup. The
same fixture uses a common workload generator across families, but differing
arc counts mean raw family timings are not an algorithm-equivalence ranking.

For a single-callback workload, let $`h`$ be the hit fraction, $`H`$ the measured
hit cost, $`M`$ the steady miss cost including admission/eviction, and $`N`$ the
NoCache computation cost for that same workload. Caching pays when

```math
hH + (1-h)M < N.
```

Paired calls must be compared as pairs: NoCache computes twice while an LRU
miss is followed by a hit. Do not substitute a clear-and-refill batch for a
steady miss, and do not extrapolate one fixture's break-even rate to all queries.

The present matrix measures its hit endpoint on 64 states and its steady-miss
endpoint on 65 states. These are different state populations. It therefore does
not establish a measured hit-rate threshold for one fixed population. Report
LRU64 relative to NoCache and CacheAll relative to NoCache separately for each
family, operation and working set. Any interpolation using the equation above
must explicitly assume that the endpoint costs represent the intended mixture;
neither NoCache endpoint is a universal computation cost. A measured threshold
would require a fixed-population mixed-access experiment, which is not claimed
by this benchmark.

Build and check semantics before taking timing samples:

```sh
cargo bench --offline --features ffi --bench ffi_cache_benchmarks -- --test
cargo bench --offline --features ffi --bench ffi_cache_benchmarks --no-run
```

Run heavy commands in a memory-limited `systemd-run --user --scope`, with
`MemorySwapMax=0`, bounded build concurrency, and temporary files under the
repository's `target` directory. Run the emitted executable pinned to an idle
physical core under the performance governor; capture per-core load, exact
source/dependency graph, executable hash and raw Criterion samples. Keep the
same affinity and workload for paired comparisons, and do not compile or profile
concurrently with timing. The campaign's predeclared preflight threshold is at
least 95% idle on the selected core before each pass. Retain failed preflight
evidence without representing it as a benchmark sample.

No real-adapter timing result is claimed yet. The companion lling-llang ledger,
`docs/scientific-ledger/shared-state-cache-2026-09-06.md`, records the synthetic
cache representation experiments, allocation tradeoffs and selection criteria.

## Inline-cache requalification and matched source graphs

After the generic cache added inline storage for its first two recency rows,
duallity again passed all 427 all-feature tests in debug and all 427 in release,
with no skips, followed by strict all-target/all-feature Clippy. The logs are
`target/agent-logs/shared-cache-inline-{debug,release,clippy}.log`. No additional
duallity production specialization was needed: its exported families use the
shared generic cache implementation.

The pre-existing real-adapter benchmark executables were not a valid reference
pair. One used only `ffi` and an older liblevenshtein feature worktree, while
another used all features and the canonical dependency. Neither established
the frozen original-cache reference. They are not used to infer a cache-only
performance change.

A matched pair was instead built from six explicit source families. Both
graphs contain the same current duallity and lling-llang integration snapshots,
with duallity's dependency path normalized to `../lling-llang` inside the
snapshots only. The four other local dependencies come from exact commits:

| Repository | Source identity |
|---|---|
| lling-llang | Current cache changes on `d18a1236c5d7d43c6566e0f45ef54f617d5066da` |
| duallity | Current cache changes on `56bb0aa47629123b76658830e2bb079e21210766` |
| liblevenshtein-rust | `919c99352b74c0ab0ba8cbf45540fec982e7b7a4` |
| libdictenstein | `0c8b1da62c97b4b478c27c3ab552b6694cfbf226` |
| vinary-tree-interop | `2e087ab4ff1c822ecda7f652408105fd04da8683` |
| llattice | `c2005a4989d16a0b6d15f2993d6c315e97f938d4` |

The first two base commits alone do not identify their uncommitted additions;
per-file checksums and source archives preserve that distinction. Canonical
dirty dependency changes were excluded, recorded and left untouched. Graph A
replaces only `lling-llang/src/wfst/shared_cache.rs` with the frozen original
non-blocking exact-LRU implementation. Graph B retains the inline/block cache.
The candidate helper files exist identically in both graphs but are not
referenced by the original module in A. Recursive comparison confirms that
the cache module is the sole file-content difference.

Both binaries use Rust 1.95.0, all duallity features, identical lockfiles and
profiles, and exactly one effective `-C target-feature=+aes,+sse2` flag pair.
Normalized dependency metadata is identical; all six local packages resolve
inside their selected graph. Each binary passes all 48 correctness-only cases,
and source checksums remain unchanged after building. Their SHA-256 identities
are:

- Original cache: `d92c73e2da364c368f9bdf5c882acbee098d6267a5e08a9e3cd077e46c10af65`.
- Inline candidate: `5e64a950de559b665ec559ea0a82ecda7bfa0392d3094ff1329537230d71f07f`.

The disk-backed `cache-duallity-matched-20260907` diagnostic directory under
the canonical liblevenshtein-rust `target/agent-logs` contains source archives,
checksums, build streams, smoke results and `qualification.json`. The build
scope requested 8 GiB memory, no swap, four CPUs' aggregate quota and four
Cargo jobs. Its journal reports 93.088 seconds wall time and a 2.3 GiB memory
peak. A delayed property query saw an already-finished scope; its default
inactive values are not presented as active resource-limit measurements.

These results qualify the inputs for a complete 48-case B1–A1–A2–B2 timing
comparison; they do not supply that comparison. No real-adapter latency,
mixed-workload break-even rate or final cache selection is claimed here.

After recording those snapshots, the feature worktree's manifest was restored
to the canonical sibling path `../lling-llang` for its cache commit. This is
the same manifest adjustment already checked in both isolated graphs. It does
not imply that the canonical lling-llang primary worktree has received the
cache commit. Final validation must use a clean sibling layout containing the
exact committed cache changes, followed by separately authorized primary-branch
integration; it must not silently compile against another owner's live edits.

## Exact committed graph qualification

The clean sibling-layout gate subsequently passed for duallity commit
`c422e3b4a4d5fdc9bff81073eeda11b1945eb858` and lling-llang commit
`aea10b13fdf0f0aa5658e6efdfc4962386605b4a`, using the same four dependency
commits tabulated above. All six sources came from Git archives without
source or manifest patches. Offline locked metadata confined every local
dependency to that graph; source checksums matched before and after.

Duallity passed all 427 all-feature workspace tests in debug and all 427 in
release, with no skips, followed by strict all-target Clippy. Its companion
lling-llang root passed 3,132 tests in each configuration and strict Clippy.
The aggregate command exited zero at `2026-09-07T19:01:01Z`. These are tests
of the two root workspaces against the dependency graph, not independent
test-suite runs for every dependency. pgmcp progress 10250 and the companion
ledger's exact committed-graph section identify the logs and checksums.

To reduce time between paired observations under a busy host, the prospective
timing protocol now uses four predefined family blocks: classic, universal,
generalized and FZF, in that order. Each block contains all 12 original cases
and its own fresh B1–A1–A2–B2 comparison. Exact case-ID validation must prove
disjoint blocks and the full 48-case union. Incomplete block attempts are
preserved but never spliced together; only whole-block eligibility retries
are permitted, independently of measured ratios. Sample settings, fixed-core
checks and performance acceptance thresholds are unchanged. pgmcp progress
10253 records this amendment before execution; the companion ledger gives
the interruption and output-isolation rules.

Report cache-policy costs separately within each family, operation and working
set. The 64-state hot and 65-state eviction endpoints do not measure a single
fixed-population mixed-hit threshold. Neither the correctness gate nor this
protocol amendment supplies a real-adapter timing result or final selection.
