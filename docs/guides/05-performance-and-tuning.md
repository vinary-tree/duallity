# 05 · Performance and tuning

duallity is built around **laziness** and **preallocation**: it computes the corner of the product
state space a query actually visits, memoizes it, and sizes every buffer up front. This guide explains
the knobs — cache policy, the per-variant cost model, dictionary backend, preallocation — and how to
measure them, so you can trade memory against latency deliberately. It builds on
[architecture/04 · Lazy evaluation and caching](../architecture/04-lazy-evaluation-and-caching.md) and
[architecture/05 · Registries and interning](../architecture/05-registries-and-interning.md); symbols
($`n`$, $`k`$, $`\delta`$, radix $`M`$, …) follow the
[master notation table](../theory/README.md#master-notation).

## 1. The lazy contract — what you actually pay for

Every WFST computes a state **only on first touch** and caches it: `expand(s)` misses the cache, calls
the state source's allocation-conscious `expand_state(s)` kernel, and stores a compact `CachedState`;
the second touch is served
from the cache ([architecture/04 §2](../architecture/04-lazy-evaluation-and-caching.md#2-the-expansion-pipeline),
diagram [`lazy-expand-sequence`](../diagrams/lazy-expand-sequence.svg)). The consequences:

- A query that visits $`N`$ states costs $`\mathcal{O}(N)`$ expansions — **independent of dictionary
  size**. You pay for the region the search explores, not the whole dictionary
  ([theory/04 §3](../theory/04-composition.md#3-the-lazy-product)).
- **Composition is lazy too**: product states are formed only as the shortest-path search reaches them,
  so `compose(rewrite, lev)` never materializes the full product.
- The one deliberate exception is [`WallBreakerWfst`](../design/wallbreaker-wfst.md), which runs the
  whole query *eagerly* at construction (§4).

## 2. Cost model

### 2.1 The automaton band

The parameterized Levenshtein automaton for a query of length $`n = \lvert q\rvert`$ at edit bound
$`k`$ has

```math
O\bigl((n{+}1)(2k{+}1)\bigr)
```

reachable states — the diagonal **band of width $`2k{+}1`$** over the $`n{+}1`$ query
positions ([theory/02](../theory/02-edit-distance-and-levenshtein-automata.md),
[theory/07 §4](../theory/07-regular-language-limits.md#4-placement-everything-duallity-ships-is-type-3)).
This is the *query-side* factor; the WFST is that band **crossed with the dictionary trie**, and the
cross product is what the lazy search walks one state at a time. Because the band is
$`\mathcal{O}(nk)`$-narrow, small $`k`$ is cheap and the cost grows only where the dictionary and the
band overlap.

### 2.2 The state-encoding radix

A product state $`(d, a)`$ — dictionary node $`d`$, automaton state $`a`$ — is packed
into one `u32` `StateId`. Two id regimes exist
([architecture/03](../architecture/03-state-encoding-and-product-space.md)): the **arithmetic radix**
$`\mathrm{StateId} = d \cdot M + a`$ (Levenshtein, universal, phonetic-triple) and **dense
registry ids** (WallBreaker, generalized, phonetic-NFA, rewrite). The radix $`M`$ bounds how many
automaton states share one dictionary node:

| Variant | Radix $`M`$ | Grows as |
|---------|-------------------|----------|
| `LevenshteinWfst` | $`M_{\mathrm{lev}} = (n{+}1)(k{+}1)(1{+}c)`$, $`c \in \{0,1,2\}`$ continuation classes | linear in $`n`$ |
| `UniversalLevenshteinWfst` | $`M_{\mathrm{uni}} = (n{+}1)^2(2k{+}1)`$ | **quadratic** in $`n`$ |
| `PhoneticWfst` | $`M_{\mathrm{phon}} = \max\bigl((k{+}1)\cdot 1000,\ 10000\bigr)`$ | step in $`k`$ |
| WallBreaker / Generalized / PhoneticNfa / Rewrite | — (dense registry ids) | — |

The `u32` ceiling is a real bound: once the reachable node count times $`M`$ reaches
$`2^{32}`$, `try_encode` returns `None` and the offending edge is **silently pruned** rather than
mis-encoded. Keep $`\lvert D_{\text{reg}}\rvert \cdot M < 2^{32}`$ for very long queries over very
large dictionaries.

### 2.3 Per-variant complexity (consolidated)

$`\delta`$ = dictionary-node out-degree; $`c`$ = enabled continuation classes (0/1/2);
$`\lvert\mathcal{O}\rvert`$ = operation-set size, $`F`$ = branching factor, $`x \le 2`$
= largest operation arity; $`L = \sum_r s_r`$ = total rule length, $`R`$ = rule/result
count, $`C = \sum_r m_r`$ = total matched-term length; $`\lvert S\rvert`$ = NFA state-set
size, $`\deg_N`$ = NFA out-degree, $`\lvert\Sigma\rvert`$ = alphabet size,
$`\lvert\Phi\rvert`$ = product-frontier size.

| Variant | Construction (per query) | Per-state expansion | Traversal |
|---------|--------------------------|---------------------|-----------|
| [`LevenshteinWfst`](../design/levenshtein-wfst.md) | $`\mathcal{O}(n)`$ + one dict-handle clone; **no** dictionary walk | $`O\bigl(\delta(1{+}c)\bigr)`$ | **lazy** |
| [`BoundUniversalWfst`](../design/universal-wfst.md) (factory) | $`\mathcal{O}(1)`$ — captures $`(D, k)`$ | — | — |
| └ `with_query` → `UniversalLevenshteinWfst` | $`O\bigl(n(n{+}k)\bigr)`$; **no automaton rebuild**, $`\lvert\Sigma\rvert`$-independent | $`\mathcal{O}(\delta)`$ | **lazy** |
| [`GeneralizedWfst`](../design/generalized-wfst.md) | dict clone + `bounded_operation_set` normalize | $`O\bigl(\lvert\mathcal{O}\rvert \cdot F^{x}\bigr)`$ | **lazy** |
| [`WallBreakerWfst`](../design/wallbreaker-wfst.md) | **runs the whole query**: $`\mathcal{O}(k)`$-piece split, SCDAWG seed (linear in piece length, $`k`$-independent), $`k`$-bounded extend, verify, + $`\mathcal{O}(R)`$ dedup + $`\mathcal{O}(C)`$ arena | $`q_0 \to {\le}R`$ arcs; interior $`\to {\le}1`$ arc | **eager** |
| [`RewriteWfst`](../design/phonetic-rewrite-wfst.md) | $`\mathcal{O}(L)`$ tokenize (preallocated) + $`\mathcal{O}(R \log R)`$ priority sort | home: $`R{+}95`$ (or $`R`$) edges, then prune; continuation: $`1`$ | **lazy** |
| [`PhoneticNfaWfst`](../design/phonetic-nfa-wfst.md) *(feature)* | seed registry with $`q_0`$ closure | $`O\bigl(\lvert S\rvert \deg_N + \lvert\Sigma\rvert\bigr)`$ | **lazy** (subset construction) |
| [`PhoneticWfst`](../design/phonetic-wfst.md) *(feature)* | build $`\text{NFA}\times\text{Lev}`$ product, seed root | $`O\bigl(\delta(\lvert\Phi\rvert + \lvert\Phi'\rvert)\bigr)`$ | **lazy** |

The load-bearing rows: **`LevenshteinWfst` rebuilds per query** (construction $`m`$ times for
$`m`$ queries) — motivating the universal factory; **`BoundUniversalWfst` builds the
query-agnostic $`U_k`$ once** and mints per-query wrappers at $`\mathcal{O}(n(n{+}k))`$ with no
automaton rebuild (§5); and **`WallBreakerWfst` is eager** — building it *is* answering the query (§4).

## 3. Cache policy

Every WFST memoizes computed states in a per-WFST `LazyStateCache` guarded by `&mut self` (the *cache*
is not shared across threads; only the [registries](../architecture/05-registries-and-interning.md) are,
behind `Arc<RwLock>` — §6). The `CachePolicy` chooses the trade-off:

```rust,ignore
use lling_llang::prelude::*;   // brings CachePolicy into scope

let mut lev = /* a LevenshteinWfst */;
lev.set_cache_policy(CachePolicy::CacheAll);                    // default: keep everything
lev.set_cache_policy(CachePolicy::Lru { max_states: 50_000 }); // bound memory
lev.set_cache_policy(CachePolicy::NoCache);                    // recompute every time
```

| Policy | Memory | CPU | Use when |
|--------|--------|-----|----------|
| `CacheAll` | unbounded (grows with states visited) | lowest (never recompute) | one-shot queries, batch jobs |
| `Lru { max_states }` | bounded to $`\le`$ `max_states` | some recomputation on eviction | long-lived / streaming services |
| `NoCache` | one-state scratch slot | highest (recompute every touch) | memory-critical, rarely-revisited states |

Three details from `src/lazy_cache.rs` worth internalizing:

- **`CacheAll` allocates no LRU metadata** — entries carry `last_access = 0` and the eviction heap stays
  empty, so the default policy has zero LRU bookkeeping overhead.
- **`NoCache` still keeps the last expanded state in a scratch slot**, so `transitions_lazy` can return a
  borrowed slice; but `computed_states()` stays `0` and `is_expanded` is always `false`. Inserting a
  second state overwrites the first.
- **`Lru` preallocates bounded storage** and treats an enormous `max_states` as a *speculative
  reservation*, capped at `MAX_SPECULATIVE_PREALLOCATION` $`= 16{,}384`$ (§7) — so
  `Lru { max_states: usize::MAX }` does not attempt a multi-gigabyte allocation.

### 3.1 `set_max_cache_size` and the per-variant defaults

`set_max_cache_size(n)` sets the LRU **fallback bound** — the size used under
`Lru { max_states: 0 }` (and the target when that policy is later re-applied). It is exposed by every
wrapper **except `RewriteWfst`**, which tunes its cache only through `set_cache_policy`:

```rust,ignore
let mut lev = /* a LevenshteinWfst */;
lev.set_max_cache_size(20_000);                          // fallback bound for Lru { max_states: 0 }
lev.set_cache_policy(CachePolicy::Lru { max_states: 0 }); // now bounded at 20_000
```

Set it **before** driving, not after the cache has grown. The `DEFAULT_MAX_CACHE_SIZE` each wrapper
starts with (verified against the seven `src/*.rs` sources):

| Wrapper | `DEFAULT_MAX_CACHE_SIZE` |
|---------|--------------------------|
| `LevenshteinWfst`, `UniversalLevenshteinWfst`, `GeneralizedWfst`, `WallBreakerWfst`, `PhoneticWfst` | `100_000` |
| `PhoneticNfaWfst` | `50_000` |
| `RewriteWfst` | `10_000` |

### 3.2 Deterministic LRU eviction

`Lru { max_states }` is a **deterministic** least-recently-used policy — no randomness, reproducible
across runs:

- every cached state records a **monotonic access tick** (`last_access`); a cache hit under `Lru`
  stamps a fresh tick;
- insertion into a full cache evicts the state with the **smallest tick**, found via a min-tick binary
  heap `BinaryHeap<Reverse<(tick, StateId)>>` whose stale entries (tick $`\ne`$ the state's current
  `last_access`) are skipped on pop;
- the heap is **compacted** by rebuild once it exceeds $`\max(4\lvert\text{entries}\rvert,\ 64)`$,
  so repeated touches cannot grow it without bound;
- at clock rollover ($`\text{tick} = 2^{64}{-}1`$) ticks are **renumbered** preserving recency
  order, so correctness survives astronomically long runs.

> ⚠ **NEW diagram (pending central render):** a state/sequence diagram
> [`cache-policy-lru-eviction`](../diagrams/cache-policy-lru-eviction.svg) — the tick clock, the min-tick
> heap, an insert-when-full evicting the smallest tick, the touch-restamps-and-skips-stale mechanism, and
> the compaction/rollover guards — belongs here and in the
> [diagram catalog](../diagrams/README.md#catalog) (next free id **D19**). Render from
> `docs/diagrams/src/cache-policy-lru-eviction.*` per the [rendering recipe](../diagrams/README.md#rendering).

<img src="../diagrams/cache-policy-lru-eviction.svg" alt="Deterministic LRU eviction: each cached state carries a monotonic access tick; a min-tick binary heap orders states by recency; inserting into a full cache pops the smallest-tick victim (skipping stale heap entries whose tick no longer matches the state's last_access); a cache hit restamps the state with a fresh tick and pushes a new heap entry; the heap is rebuilt when it exceeds four times the entry count, and ticks are renumbered at clock rollover (diagram pending central render)" width="820"/>

## 4. `WallBreakerWfst` is eager

[`WallBreakerWfst`](../design/wallbreaker-wfst.md) is the exception to laziness: **constructing it runs
the whole query** (split → seed → extend → verify → normalize) up front and caches the finite answer
forest ([theory/06](../theory/06-wallbreaker-and-the-wall-effect.md)). So:

- the cost is paid at `new` / `with_algorithm` / `build`, **not** lazily during traversal — building the
  WFST is exactly as expensive as answering the query;
- `num_results()` tells you how many terms matched (after $`k`$-bound + dedup normalization);
- traversal of the built forest is cheap and $`k`$-independent: expanding the super-start yields
  $`\le R`$ arcs and any interior state $`\le 1`$, so the whole view is $`\mathcal{O}(C)`$;
- **reuse the constructed WFST** for repeated traversals of the *same* query rather than rebuilding it.

It is the right tool precisely when large $`k`$ would make the lazy automaton's *wall* expensive —
at small $`k`$ prefer `LevenshteinWfst` or `BoundUniversalWfst`, which avoid the SCDAWG requirement
([guides/02](02-choosing-a-variant.md)).

## 5. Reuse across queries

For many queries against one dictionary, build the automaton **once** with
[`BoundUniversalWfst`](../design/universal-wfst.md) and mint per-query WFSTs with `with_query` — the
query-agnostic $`U_k`$ is shared and needs no rebuild, so per-query setup is only the
relevant-subword precompute and registry seeding ($`\mathcal{O}(n(n{+}k))`$), and it is
$`\lvert\Sigma\rvert`$-independent ([theory/05](../theory/05-universal-automata.md)):

```rust,ignore
use duallity::BoundUniversalWfst;
use liblevenshtein::transducer::universal::Standard;
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;

let dict  = DynamicDawgChar::<()>::from_terms(vec!["hello", "help", "world"]);
let bound = BoundUniversalWfst::<Standard, _>::new(dict, 2);   // build U_2 once

let mut w1 = bound.with_query("helo");   // no automaton rebuild — only per-query wiring
let mut w2 = bound.with_query("wrld");
```

Contrast `LevenshteinWfst`, which bakes the query into the automaton and so rebuilds the whole machine
per query — fine for one-shot use, wasteful at high query volume against a fixed $`(D, k)`$.

## 6. Choosing a dictionary backend

The container affects both memory and read/update speed. All variants take a libdictenstein dictionary;
the registries that intern its nodes are shared behind `Arc<RwLock>` — **reads** (resolve a node by id)
take a shared read lock, **writes** (register a newly-seen child) take a brief exclusive write lock, and
poisoned locks are recovered rather than re-panicked
([architecture/05 §7](../architecture/05-registries-and-interning.md#7-concurrency-model)):

| Backend | Reads | Updates | Notes |
|---------|-------|---------|-------|
| `DynamicDawgChar` | fast (SIMD / bloom-filter optimized) | **yes** | general-purpose, runtime-updatable |
| `DoubleArrayTrieChar` | **fastest** | treat as read-only once built | best for static dictionaries |
| `Scdawg` / SCDAWG | substring search | build then query | **required** by `WallBreakerWfst` |

The trade-off: `DynamicDawgChar` when the dictionary changes at runtime; `DoubleArrayTrieChar` for a
static, read-heavy dictionary; `Scdawg` when a variant needs substring search (only WallBreaker does).

## 7. Preallocation

duallity preallocates wherever a size is known — "a deliberate best practice, not a premature
optimization":

- **Transitions inline.** A cached state stores its edges in a `SmallVec<[WeightedTransition<…>; 4]>`,
  which inlines up to **four** transitions — the exact branching of a Levenshtein cell
  (match / substitute / insert / delete) — so the common case allocates **nothing** on the heap
  ([architecture/04 §2](../architecture/04-lazy-evaluation-and-caching.md#2-the-expansion-pipeline)).
- **Caches and registries** use `rustc_hash::FxHashMap` (fast non-cryptographic hashing) keyed by the
  dense `u32` `StateId`, each a forward map + reverse `Vec`.
- **Exact-size rule buffers.** `RewriteWfst` sizes its continuation-lookup and priority-order vectors to
  their **exact** final lengths at construction (`PreparedRuleMetadata::from_rules`), never growing them
  incrementally.
- **Bounded speculation.** Cache and hint preallocation are capped so a pathological bound cannot request
  an enormous allocation: `capped_size_hint_capacity(n) = min(n, MAX_SPECULATIVE_PREALLOCATION)` with
  `MAX_SPECULATIVE_PREALLOCATION` $`= 16{,}384`$, and `num_states_hint` is capped at
  `MAX_NUM_STATES_HINT` $`= 1{,}000{,}000`$.

When you know your working set, call `set_max_cache_size` (or set `Lru { max_states }`) **up front** so
the LRU cache reserves its bounded storage once, rather than letting it grow and evict.

## 8. Benchmarking

The empirical baseline lives in [`benches/wfst_expansion.rs`](../../benches/wfst_expansion.rs) — a
Criterion suite over a **deterministic** CVCV corpus (no RNG, no external corpus, reproducible across
machines). It has two groups, both at `MAX_DISTANCE = 2`:

| Group | Measures | Method |
|-------|----------|--------|
| `construction` | `Variant::new(&dict, query, k)` — registry seeding + query-side automaton | timed directly |
| `expansion_bfs` | the hot path: repeated `transitions_lazy` over a bounded BFS of the reachable product graph | `iter_batched` with a fresh WFST per iteration (`BatchSize::SmallInput`), so construction cost is **not** folded in |

Both groups cover the four non-phonetic variants — `LevenshteinWfst`, `UniversalLevenshteinWfst`,
`GeneralizedWfst`, `WallBreakerWfst` (the phonetic variants need the `phonetic-rules` feature and rule
configuration; they can be added later) — over corpus sizes $`1{,}000`$ and $`10{,}000`$.
The `expansion_bfs` BFS is bounded to `MAX_EXPANDED_STATES = 2_000` computed states so the traversal is
comparable across variants and the suite stays in the low-minutes range (`sample_size 30`, 2 s warm-up,
5 s measurement). WallBreaker is fed an `Scdawg`; the others a `DynamicDawgChar`.

The benchmark is registered in `Cargo.toml` (`[[bench]] name = "wfst_expansion"`, `harness = false`).
Run it, pinning to isolated cores at a fixed frequency for stable numbers, and tee the output so it is
analyzed once rather than re-run:

```sh
taskset -c 2,3 cargo bench --bench wfst_expansion | tee bench-$(date +%Y%m%d).txt
```

Read `construction` as the per-query rebuild cost that `BoundUniversalWfst` reuse (§5) amortizes, and
`expansion_bfs` as the lazy-expansion hot path that the registry-batching, lock-scope, preallocation, and
state-encoding work targeted. Profile before optimizing (`perf record --call-graph lbr`), form a
hypothesis, and re-measure — the same corpus makes before/after runs directly comparable.

## See also

- [architecture/04 · Lazy evaluation and caching](../architecture/04-lazy-evaluation-and-caching.md) — the `expand → compute_state → cache` pipeline and the policy semantics.
- [architecture/05 · Registries and interning](../architecture/05-registries-and-interning.md) — the shared `Arc<RwLock>` registries and the read/write lock model.
- [architecture/03 · State encoding and the product space](../architecture/03-state-encoding-and-product-space.md) — the arithmetic-radix vs. dense-registry id regimes of §2.2.
- [engineering/concurrency-and-locking](../engineering/concurrency-and-locking.md) — lock scope and lock-free alternatives.
- [guides/02 · Choosing a variant](02-choosing-a-variant.md) — the variant decision the cost model informs.
