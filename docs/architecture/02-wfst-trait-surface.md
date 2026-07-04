# 02 · The WFST trait surface

> **Defines:** the `lling_llang` traits every duallity WFST implements — `Wfst`, `LazyWfst`,
> `StateSource`, `LatticeBackend` — and `CachePolicy`.

duallity implements four `lling_llang` traits. Three describe a transducer (eager view, lazy view,
state-computation kernel); one adapts a dictionary to the lattice infrastructure. All are generic
over a label type `L` (always `char` here) and a `Semiring` weight `W` (always `TropicalWeight`).

## 1. `Wfst<L, W>` — the eager view

```rust,ignore
pub trait Wfst<L, W: Semiring>: Clone + Send + Sync {
    fn start(&self) -> StateId;
    fn is_final(&self, state: StateId) -> bool;
    fn final_weight(&self, state: StateId) -> W;
    fn transitions(&self, state: StateId) -> &[WeightedTransition<L, W>];
    fn num_states(&self) -> usize;
    // provided: is_valid_state, num_transitions, total_transitions, is_empty, state(...)
}
```

`start` returns the initial state; `is_final`/`final_weight` report acceptance and its tropical cost;
`transitions` returns the outgoing arcs of a state as a slice; `num_states` reports how many states
exist (for lazy WFSTs, how many have been *computed* so far). A `WeightedTransition<char, TropicalWeight>`
bundles `from`, `input: Option<char>`, `output: Option<char>`, `target`, and `weight` — the
`input : output / weight` of [theory/03](../theory/03-levenshtein-as-transducer.md).

> Because duallity's WFSTs are **lazy**, the eager `transitions`/`is_final`/`final_weight` read only
> the cache: they return `&[]` / `false` / `zero()` for a state that has not been expanded yet. Drive
> expansion through `LazyWfst` (below) before reading, or use a `LazyWfstWrapper` around a
> `StateSource`.

## 2. `LazyWfst<L, W>` — the lazy view

```rust,ignore
pub trait LazyWfst<L, W>: Wfst<L, W> {
    fn is_expanded(&self, state: StateId) -> bool;
    fn expand(&mut self, state: StateId);
    fn transitions_lazy(&mut self, state: StateId) -> &[WeightedTransition<L, W>];
    fn cache_policy(&self) -> CachePolicy;
    fn set_cache_policy(&mut self, policy: CachePolicy);
    fn computed_states(&self) -> usize;
    fn clear_cache(&mut self);
}
```

`expand(s)` computes state `s` and stores it in the cache; `transitions_lazy(s)` expands *then*
returns the arcs. These take `&mut self` because expansion mutates the cache (and, for some variants,
a state registry). This is the interface most callers use directly.

<img src="../diagrams/lazy-expand-sequence.svg" alt="The first touch of a state flows through the wrapper, state source, registry, and cache; the second touch is a cache hit" width="820"/>

## 3. `StateSource<L, W>` — the computation kernel

```rust,ignore
pub trait StateSource<L, W: Semiring>: Clone + Send + Sync {
    fn compute_state(&self, state: StateId) -> LazyState<L, W>;
    fn start(&self) -> StateId;
    fn num_states_hint(&self) -> Option<usize> { None }  // provided
}
```

A `StateSource` is the **pure, immutable** core that knows how to compute *one* state on demand. It
returns a `LazyState`, which is either `Computed { is_final, final_weight, transitions }` or
`Pending`. Because `compute_state` takes `&self`, a state source can be wrapped in a
`LazyWfstWrapper<S, L, W>` and dropped straight into `compose` — the composition search calls
`compute_state` as it visits product states, with no `&mut` plumbing.

`num_states_hint()` is a reservation hint, not a semantic bound. duallity returns `Some(n)` only when
the underlying dictionary can report `len()` efficiently; unknown-size backends propagate `None`
instead of inventing a fallback size that could over-reserve memory.

duallity's parameterized, universal, phonetic, generalized, and WallBreaker engines are state
sources (`LevenshteinStateSource`, `UniversalLevenshteinStateSource`, `PhoneticStateSource`,
`GeneralizedWfst`, and `WallBreakerWfst`). `GeneralizedWfst` uses interior registries
(`Arc<RwLock<_>>`) so `compute_state(&self, s)` can still mint dictionary/product/continuation
states and return `Computed`. `WallBreakerWfst` takes the opposite route: WallBreaker already
materializes finite result terms, so construction pre-registers the dense result-path state forest.
That makes `compute_state(&self, s)` immutable and fully functional while the `LazyWfst` path still
provides transition caching (see [architecture/04](04-lazy-evaluation-and-caching.md)).

## 4. `CachePolicy`

```rust,ignore
pub enum CachePolicy {
    CacheAll,                       // default — keep every computed state
    Lru { max_states: usize },      // evict when the cache exceeds the bound
    NoCache,                        // recompute every time
}
```

`CacheAll` is the default and is right for one-shot queries. `Lru { max_states }` bounds memory for
long-lived or streaming use by evicting the least recently touched cached state. `NoCache` keeps only
a one-state scratch buffer so callers can still borrow a transition slice from `transitions_lazy`
without growing the persistent cache.

## 5. `LatticeBackend` — adapting a dictionary

The odd one out: `DictionaryBackend` implements `lling_llang`'s `LatticeBackend`, which is a
**vocabulary** adapter, not a transducer. It maps interned terms to `VocabId`s (`u32`) for
lling-llang's lattice infrastructure:

```rust,ignore
pub trait LatticeBackend: Clone + Send + Sync {
    fn intern(&mut self, word: &str) -> VocabId;          // existing or fresh sequential id
    fn lookup(&self, id: VocabId) -> Option<&str>;
    fn vocab_size(&self) -> usize;
    fn contains(&self, word: &str) -> bool;               // cache ∪ underlying dictionary
    fn get_id(&self, word: &str) -> Option<VocabId>;      // cache only, no auto-intern
    fn iter(&self) -> impl Iterator<Item = (VocabId, &str)>;
    fn supports_sharing(&self) -> bool { false }
}
```

`DictionaryBackend` interns terms lazily, backing the forward map with `FxHashMap<Arc<str>, VocabId>`
and the reverse map with `Vec<Arc<str>>`, so the whole backend clones cheaply. Its `contains` checks
**both** the interning cache and the underlying `Dictionary::contains`. See
[design/levenshtein-wfst · DictionaryBackend](../design/levenshtein-wfst.md#dictionarybackend).
