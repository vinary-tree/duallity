# 05 · Performance and tuning

duallity is built around laziness and preallocation. This guide explains the knobs and the costs so
you can tune memory and latency deliberately.

## 1. Cache policy

Every WFST memoizes computed states ([architecture/04](../architecture/04-lazy-evaluation-and-caching.md)).
The `CachePolicy` chooses the trade-off:

```rust,ignore
use lling_llang::prelude::*;

let mut lev = /* a LevenshteinWfst */;
lev.set_cache_policy(CachePolicy::CacheAll);                 // default: keep everything
lev.set_cache_policy(CachePolicy::Lru { max_states: 50_000 }); // bound memory
lev.set_cache_policy(CachePolicy::NoCache);                  // recompute every time
```

| Policy | Memory | CPU | Use when |
|--------|--------|-----|----------|
| `CacheAll` | unbounded (grows with states visited) | lowest (never recompute) | one-shot queries, batch jobs |
| `Lru { max_states }` | bounded | some recomputation | long-lived / streaming services |
| `NoCache` | one-state scratch slot | highest | memory-critical, rarely revisited states |

`set_max_cache_size(n)` sets the bound used under `Lru` (and as the fallback when
`Lru { max_states: 0 }`). Defaults: `100_000` for Levenshtein / Universal / Phonetic, `50_000` for
`PhoneticNfaWfst`, `10_000` for `RewriteWfst`.

`Lru { max_states }` is a deterministic least-recently-used policy: every cached state records its
last touch tick, and insertion evicts the smallest tick when the cache is full. `NoCache` does not add
states to the persistent cache, but it keeps the most recently expanded state in scratch storage so
`transitions_lazy` can still return a borrowed slice.

## 2. Lazy expansion costs

- Expanding a Levenshtein state computes up to **four** transitions
  (`SmallVec<[_; 4]>`, no heap allocation in the common case) and consults a registry under a read or
  write lock ([architecture/05](../architecture/05-registries-and-interning.md)).
- A query that visits *N* states costs *O(N)* expansions, independent of dictionary size — you pay for
  the corner of the product space the search explores, not the whole dictionary
  ([theory/04 §3](../theory/04-composition.md#3-lazy-composition)).
- Composition is lazy too: product states are formed only as the shortest-path search reaches them.

## 3. WallBreaker is eager

[`WallBreakerWfst`](../design/wallbreaker-wfst.md) is the exception: **constructing it runs the whole
query** (split → seed → extend → verify) up front and caches the results. So:

- the cost is paid at `new`/`build`, not lazily during traversal;
- `num_results()` tells you how many terms matched;
- reuse the constructed WFST for repeated traversals of the *same* query rather than rebuilding it.

It is the right tool precisely when large `k` would make the lazy automaton's wall expensive
([theory/06](../theory/06-wallbreaker-and-the-wall-effect.md)).

## 4. Reuse across queries

For many queries against one dictionary, build the automaton **once** with
[`BoundUniversalWfst`](../design/universal-wfst.md) and mint per-query WFSTs with `with_query` — the
query-agnostic automaton is shared, so per-query setup is just the dictionary walk
([theory/05](../theory/05-universal-automata.md)).

## 5. Choosing a dictionary backend

The container affects both memory and read/update speed:

| Backend | Reads | Updates | Notes |
|---------|-------|---------|-------|
| `DynamicDawgChar` | fast (SIMD / bloom-filter optimized) | yes | general-purpose, runtime-updatable |
| `DoubleArrayTrieChar` | fastest | treat as read-only once built | best for static dictionaries |
| `Scdawg` / SCDAWG | substring search | build then query | **required** by WallBreaker |

## 6. Preallocation

duallity preallocates wherever a size is known (transition `SmallVec`s, registry vectors, the cache's
`FxHashMap`). When you know your working set, set `set_max_cache_size` up front rather than letting the
cache grow and evict. Preallocation here is a deliberate best practice, not a premature optimization.

## See also

- [architecture/04 · Lazy evaluation and caching](../architecture/04-lazy-evaluation-and-caching.md)
- [engineering/concurrency-and-locking](../engineering/concurrency-and-locking.md)
- [guides/02 · Choosing a variant](02-choosing-a-variant.md)
