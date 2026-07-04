# 04 · Lazy evaluation and caching

> **Defines:** the `expand → compute_state → cache` pipeline, deterministic LRU caching, and the
> immutable-`StateSource` vs mutable-`LazyWfst` split.

## 1. Why lazy

The product state space (dictionary × automaton) is astronomically large and almost entirely
unvisited by any single query. Materializing it would be impossible; duallity therefore computes a
state **only on first touch** and memoizes it. A query that explores a few hundred states pays for a
few hundred states, regardless of how many million the dictionary could in principle induce.

## 2. The expansion pipeline

The first time a state `s` is needed, it flows through three layers; the second time, it is served
straight from the cache:

<img src="../diagrams/lazy-expand-sequence.svg" alt="First touch: wrapper → state source → registry → cache; second touch: cache hit" width="820"/>

1. The **wrapper** (`LevenshteinWfst`, `UniversalLevenshteinWfst`, …) receives `expand(s)` /
   `transitions_lazy(s)` and checks its cache.
2. On a **miss**, it calls the **state source**'s `compute_state(s)`, which decodes `s` into
   `(dict_node, automaton_state)`, resolves the dictionary node and automaton state (consulting a
   registry — [architecture/05](05-registries-and-interning.md)), and returns a
   `LazyState::Computed { is_final, final_weight, transitions }`.
3. The wrapper stores a `CachedState` and returns the transition slice.

The cached state is a compact record:

```rust,ignore
struct CachedState {
    is_final: bool,
    final_weight: TropicalWeight,
    transitions: SmallVec<[WeightedTransition<char, TropicalWeight>; 4]>,
}
```

`SmallVec<[_; 4]>` inlines up to four transitions — the exact branching of a Levenshtein cell
(match/substitute/insert/delete) — so the common case allocates nothing on the heap. The cache itself
is a `rustc_hash::FxHashMap<StateId, CachedState>` (fast, non-cryptographic hashing keyed by the dense
`u32` `StateId`).

## 3. Cache policy and deterministic eviction

The `CachePolicy` ([architecture/02](02-wfst-trait-surface.md#4-cachepolicy)) chooses the memory
trade-off:

| Policy | Behaviour | Default cache bound |
|--------|-----------|---------------------|
| `CacheAll` | keep every computed state (the default) | — |
| `Lru { max_states }` | evict the least recently touched state when the cache reaches `max_states` | `DEFAULT_MAX_CACHE_SIZE` per variant |
| `NoCache` | keep only a one-state scratch slot for the last expanded state | — |

The per-variant default bounds (used when `Lru { max_states: 0 }` falls back to the configured size):

| Variant | `DEFAULT_MAX_CACHE_SIZE` |
|---------|--------------------------|
| `LevenshteinWfst`, `UniversalLevenshteinWfst`, `PhoneticWfst` | `100_000` |
| `PhoneticNfaWfst` | `50_000` |
| `RewriteWfst` | `10_000` |

`Lru { max_states }` records a monotonic access tick for each cached state and evicts the smallest
tick before inserting a new state. `max_states = 0` falls back to the variant's configured bound
where that variant exposes one; otherwise it is clamped to one. `NoCache` still keeps the last
expanded state in a scratch slot so `transitions_lazy` can return a borrowed transition slice without
increasing `computed_states()`.

## 4. The immutable / mutable split

There are **two ways** to drive a duallity WFST, and the difference matters:

| Path | Method | Mutability | Used by |
|------|--------|------------|---------|
| **StateSource** | `compute_state(&self, s) -> LazyState` | immutable (`&self`) | `compose`, via `LazyWfstWrapper` |
| **LazyWfst** | `expand(&mut self, s)`, `transitions_lazy(&mut self, s)` | mutable (`&mut self`) | direct callers |

The immutable `StateSource` path is what makes composition cheap: `compose` holds a shared reference
and calls `compute_state` as the search visits product states, never needing `&mut`. The
parameterized, universal, phonetic, generalized, and WallBreaker engines implement a **fully
functional** `compute_state`. `GeneralizedWfst` keeps its node/product/continuation registries behind
`Arc<RwLock<_>>`, so it can register newly discovered states while satisfying the immutable trait.
`WallBreakerWfst` instead pre-registers its finite result-chain state forest at construction time,
because WallBreaker has already materialized the accepted candidate terms.

The mutable `LazyWfst` path remains useful for direct callers: it layers cache policies over the same
state ids that `StateSource` uses. This is documented in
[design/wallbreaker-wfst](../design/wallbreaker-wfst.md).
