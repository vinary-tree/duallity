# Python binding construction and lazy-expansion experiment — 2026-09-04

## Question and boundary

The Python facade receives a borrowed two-word `VtResource`, asks native
duallity to capture one immutable dictionary revision, and adopts the returned
WFST resource without copying dictionary terms or state arrays through Python.
This experiment asks whether that construction phase remains independent of
dictionary cardinality in practice, and measures the first lazy state expansion
as a separate phase.

It does **not** compare Python with Rust call overhead, measure whole-query
throughput, or characterize later graph traversal. Those require distinct
controls and are outside this experiment's claim.

## Preregistered hypothesis

The `pgmcp` experiment
`duallity-python-lazy-construction-scale-separation` (experiment 326,
hypothesis 326) froze this criterion before the confirmatory run:

> Construction latency at 100,000 terms is statistically equivalent within
> 15% to construction latency at 100 terms for the same query and edit bound.

The decision rule used two one-sided tests (TOST) for equivalence at
$`\alpha = 0.05`$, with a margin of 15% of the control mean. The protocol required
at least 64 post-warmup samples per arm; each arm supplied 100.

## Reproduction protocol

The host was an AMD Ryzen Threadripper PRO 5975WX with 32 online physical
cores, `amd-pstate-epp`, the `performance` governor, and boost enabled. The
benchmark process was pinned to CPU 0 and bounded by a 2 GiB memory limit and a
one-core CPU quota. Pre-run load averages were 6.05, 7.19, and 7.84 on the
32-core host.

Both arms used the 13-scalar query `term-00000042`, maximum edit distance 2,
20 warmups, and 100 recorded iterations. Dictionary population occurred before
the timed region.

```sh
python3 bindings/python/benchmark/compare.py \
  --iterations 100 --terms 100 --warmups 20 --json

python3 bindings/python/benchmark/compare.py \
  --iterations 100 --terms 100000 --warmups 20 --json
```

The construction clock surrounds `duallity.wfst(...)`. The second clock starts
after construction and surrounds `graph.arcs(graph.start)`, so the facade's
lazy first expansion is not silently charged to construction.

## Results

| Phase | 100 terms | 100,000 terms | Treatment change |
|---|---:|---:|---:|
| construction median | 43,347.5 ns | 43,372 ns | +0.0565% |
| construction mean | 44,240.69 ns | 43,936.26 ns | -0.688% |
| first-expansion median | 30,923 ns | 30,518 ns | -1.310% |
| first-expansion mean | 31,779.63 ns | 31,031.95 ns | -2.353% |

For construction, TOST accepted equivalence with
$`p = 1.9161569515963424 \times 10^{-52}`$; the 90% confidence interval for the treatment-minus-
control mean was -831.40 ns to 222.54 ns, entirely inside the preregistered 15%
margin. The observed mean difference was -304.43 ns. Both samples departed
from normality, so pgmcp also reported the distribution-free robustness
diagnostic: Mann–Whitney $`p = 0.2647`$ and Cliff's delta 0.0914. That diagnostic
does not replace the preregistered equivalence decision.

## Interpretation

The accepted result supports the intended constant-size resource handoff:
increasing the dictionary from 100 to 100,000 terms did not produce a material
construction-latency increase. The separately reported first-expansion phase
also remained stable, as expected for expanding only the start state of a lazy
product.

The evidence is platform- and workload-specific. It establishes cardinality
independence for this controlled boundary operation, not universal latency
bounds. The committed benchmark emits every raw sample with `--json`, enabling
the same preregistered design on other architectures and release artifacts.
