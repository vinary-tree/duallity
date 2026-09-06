# Exact generalized expansion: verification ledger

Date: 2026-09-06. Scope: pgmcp task
`make-duallity-generalized-costs-exact-and-work-bounded`.

This entry records correctness and bounded-resource evidence, not a speedup
claim. Read the [semantics](../design/generalized-wfst.md) and
[resource/transaction contract](../security/generalized-expansion-bounds.md)
for the complete design.

## 1. Observations, hypotheses, and tests

| Observed failure mechanism | Design hypothesis | Executable challenge |
|---|---|---|
| Floating-point accumulated cost affected pruning and product identity | Use the native exact decimal scale for both; convert only emitted weights | Thirty tenths fit budget three, thirty-one do not; integer-grid oracle and native-language differential |
| A final dictionary node could accept an unemitted query suffix | Finality requires both dictionary completion and full query consumption | Suffix/finality regressions in `tests/generalized_wfst.rs` |
| Multi-label rules could expose a partly registered continuation | Stage complete chains, count/reserve both registries, then publish | Exact-fit and one-below limits, cache eviction/recomputation, concurrent clones |
| Foreign pages could allocate or run based on untrusted totals | Pull fixed pages, validate totals/progress/labels, charge traversal work | Inflated/changing totals, late failure, work budget stopping after one page |
| Shared provider diagnostics could cross-contaminate nested or concurrent work | Give every computation its own thread-local fault owner | Real callbacks through decorators, nested recovery, same-provider concurrent good/bad calls |
| Node clone/destruction could run during registry locking or after publication | Own registered nodes through `Arc`; retire duplicates before committing, outside locks | Lifecycle lock probes, destructor-triggered fault/cancellation/reentry, bounded retry exhaustion |
| Width-cache scans could repeatedly inspect earlier widths or recompute absences | Assign compact width slots and cache missing/empty results | Width 4096 uses one slot; widths 1–90 have an exact 452-unit ledger |
| The maximum integer state ID is a sentinel, not a valid state | Validate product and full-chain endpoints against `NO_STATE` | Synthetic last-valid/sentinel preflight tests |
| Foreign construction copied an oversized query before validation and erased limit errors | Use borrowed-query validation and preserve typed classification | Exact and over-limit byte/scalar tests across all four generalized presets |
| Capture callback `LimitExceeded` became generic provider failure | Preserve the existing public limit status at each boundary | Snapshot/root/length failures across all nine selectors, with balanced retains |

The independent review found the sentinel, retirement-ordering, and constructor
classification defects after earlier test runs had passed. Those runs were
insufficient evidence for these invariants; the added tests and revised source
address the specific missing cases.

## 2. Source graph and environment

The native verification used Rust `1.95.0`, LLVM `22.1.2`, and Linux
`7.2.3-arch1-2` on `x86_64-unknown-linux-gnu`. Dependency source revisions:

| Repository | Commit |
|---|---|
| liblevenshtein-rust | `76c5f325` |
| lling-llang | `d18a1236c5d7d43c6566e0f45ef54f617d5066da` |
| libdictenstein | `463a894b838568985e7b6e6844d4a1f16a2b1e73` |
| vinary-tree-interop | `2c9e19ba42dd444af2362e5336f0927a6def1235` |
| llattice | `c2005a4989d16a0b6d15f2993d6c315e97f938d4` |

The duallity manifest retains canonical relative sibling paths. Integration
worktree names and workstation absolute paths are not package dependencies.
The lling-llang revision includes the companion provider-status correction and
its verified model in `proofs/coq/abi/StatusMapping.v`.

## 3. Recorded verification

| Gate | Observed result |
|---|---|
| Default-feature nextest, debug | 280 passed, none skipped |
| Default-feature nextest, release | 280 passed, none skipped |
| All-feature nextest, debug, including final constructor fixes | 420 passed, none skipped |
| All-feature nextest, release, including final constructor fixes | 420 passed, none skipped |
| All-feature doctests | 13 passed |
| All-target, all-feature Clippy with warnings denied | Passed |
| All-feature rustdoc with warnings denied | Passed |
| `examples/generalized_bounded.rs` | Built and ran successfully |
| Rocq `proofs/coq/StatusMapping.v` | Compiled |
| Unchecked-proof-escape gate | Passed |
| ABI invariant traceability gate | 14 invariants passed |
| Binding model/header/facade drift gate | 59 checks passed |
| Read-only Raku math scanner on changed documents | Passed |
| Updated transaction diagram | Rendered headlessly and visually inspected |

The full four-configuration matrix was repeated from commit `67bdc2e` in a
clean six-repository source graph using canonical sibling names and only the
committed revisions above. Default-feature debug/release each passed 280 tests;
all-feature debug/release each passed 420. The clean graph also passed all 13
doctests, strict Clippy, strict rustdoc, and the executable example. This excludes
reliance on uncommitted dependency edits or integration-worktree manifest paths.
The checkouts occupied 159 MiB and reused the existing on-disk Cargo target;
no tmpfs build directory or package publication was involved.

The status proof enumerates every interop status, generalized error category,
and scale-error category. Diagnostic strings and numeric payloads are erased
only because runtime classification does not inspect them. The model is not a
proof of arbitrary foreign callbacks or Rust memory safety.

## 4. Reproduction

Check out the listed revisions under canonical sibling names. From the duallity
checkout, create on-disk scratch/log directories, then run each command inside
a resource-limited scope. No tmpfs scratch space or graphical profiler is needed.

```sh
mkdir -p target/tmp target/agent-logs
systemd-run --user --scope \
  -p MemoryMax=12G -p MemorySwapMax=0 -p CPUQuota=400% \
  -p TasksMax=128 -p IOWeight=20 \
  env TMPDIR="$PWD/target/tmp" CARGO_INCREMENTAL=0 \
  CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 \
  CARGO_BUILD_JOBS=4 RUST_BACKTRACE=1 \
  cargo nextest run --workspace --all-features --no-fail-fast
```

Repeat with default features and with `--release`. The supplementary commands
are:

```sh
cargo test --all-features --doc
cargo run --all-features --example generalized_bounded
cargo clippy --all-features --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --all-features --no-deps
coqc -Q proofs/coq Duallity proofs/coq/StatusMapping.v
make -C proofs/coq proof-check
python3 scripts/check-abi-invariants.py
python3 scripts/check-bindings.py
scripts/doc-mathlint.sh docs/design/generalized-wfst.md \
  docs/security/generalized-expansion-bounds.md \
  docs/scientific-ledger/generalized-expansion-2026-09-06.md
```

Run compilation and proof commands with the same process limits; the status
proof needs only a 2 GiB ceiling. Capture output with `tee` and enable shell
`pipefail` so a failing command cannot be hidden by a successful log write.

## 5. Bounded runtime and memory observation

The existing `wfst_expansion` Criterion harness was built from the clean source
graph and run with filter `generalized`. It constructs deterministic dictionaries
of 1,000 and 10,000 distinct 5–7-character terms, chooses one substitution from
a mid-corpus term, and uses standard generalized operations at radius two.
Dictionary construction is outside the timed regions. The construction group
measures WFST construction and destruction; the expansion group measures
breadth-first traversal of at most 2,000 states, including traversal bookkeeping
and WFST cleanup, with WFST construction in Criterion's untimed batch setup.

| Workload | Criterion estimate | Reported confidence interval |
|---|---:|---:|
| Construct, 1,000 terms | 1.0254 microseconds | 1.0246–1.0264 microseconds |
| Construct, 10,000 terms | 1.0246 microseconds | 1.0239–1.0254 microseconds |
| Bounded expansion, 1,000 terms | 1.9699 milliseconds | 1.9666–1.9752 milliseconds |
| Bounded expansion, 10,000 terms | 3.2380 milliseconds | 3.2361–3.2407 milliseconds |

These are Criterion's default 95% confidence intervals, not independent
whole-process replications. Construction used 20 samples, one second of warm-up,
and a two-second measurement target. The expansion group's explicit settings
override those global settings: 30 samples, two seconds of warm-up, and a
five-second target (actual collections 5.4896 and 6.0145 seconds).

The host is an AMD Ryzen Threadripper PRO 5975WX with 32 physical cores and one
active hardware thread per core. The benchmark was pinned to CPU 4, which was
97.01% idle in the immediately preceding three-second observation; aggregate
idle was 83.52%. The performance governor and boost were enabled. Frequency
was not locked, and unrelated work continued on other cores. A separate
40-second per-core monitor covered the run and its tail; its average includes
post-benchmark idle time and must not be interpreted as benchmark utilization.

The complete benchmark process took 23.49 seconds. GNU time reported peak
resident memory of 52,804 KiB, 99% CPU utilization, zero swaps, 341 voluntary
and 368 involuntary context switches. Peak RSS includes Criterion, dictionary
fixtures and all four cases; it is not per-expansion heap usage or an allocation
count. User-mode perf counters reported 99,973,570,310 cycles and
259,804,032,388 instructions. The user-only context-switch/migration counters
were zero; they do not establish absence of scheduling interference, as GNU
time's switch counts demonstrate.

The measured invocation used the executable printed by
`cargo bench --all-features --bench wfst_expansion --no-run`, with:

```text
--bench generalized --noplot --sample-size 20 --measurement-time 2 --warm-up-time 1
```

It ran under `systemd-run` with a 4 GiB memory ceiling, no swap, a 200% CPU quota,
and `taskset -c 4`. The quota exceeds the pinned workload's one-core ceiling,
avoiding an intentional 100% cgroup-throttling boundary. `perf stat` and
`/usr/bin/time -v` captured counters and memory. All logs and Criterion data
were written beneath the repository's on-disk `target` directory.

This is a bounded native observation, not a Java comparison or a causal
before/after experiment. No old implementation was measured in the same
session, and the results do not justify a speedup percentage.

## 6. Interpretation and remaining campaign scope

The evidence supports exact pruning/identity, complete logical publication,
explicit resource exhaustion, and scoped callback failure handling for the
generalized adapter. It does not establish lock-free registry publication,
a wall-clock deadline for arbitrary callbacks, or elimination of process-wide
allocator aborts.

No Java/Rust performance comparison or before/after speed claim is inferred
from these tests. The deterministic work ledger is resource evidence, not
timing evidence. Retained cross-callback expansion caching and the configurable
foreign constructor ABI are separate approved tasks; this change does not
claim they are implemented or publish any RC.6 package.
