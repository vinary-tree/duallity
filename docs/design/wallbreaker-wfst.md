# WallBreaker WFST

> **`WallBreakerWfst<'a, D>`** and **`WallBreakerWfstBuilder<'a, D>`** — defeat the **wall effect** at
> large `k` by seeding on exact substrings and extending bidirectionally. Requires a
> `SubstringDictionary` (an SCDAWG). Always available (no feature flag).

## 1. Intuition

At large edit distance the plain Levenshtein automaton hits a combinatorial **wall**: nothing can be
pruned until the first `k` characters are consumed, so every short dictionary prefix stays live
([theory/06](../theory/06-wallbreaker-and-the-wall-effect.md)). `WallBreakerWfst` jumps the wall: it
splits the query so that at least one piece is uncorrupted, finds that piece as an **exact substring**
in the dictionary, and extends outward to recover the full match.

<img src="../diagrams/wallbreaker-pipeline.svg" alt="WallBreaker pipeline: split, seed, extend, verify" width="900"/>

The split-seed-extend-verify **algorithm lives upstream** in `liblevenshtein::wallbreaker`; this
wrapper *invokes* it (eagerly, at construction) and re-presents the results as a composable WFST.

## 2. Type and bounds

```rust,ignore
pub struct WallBreakerWfst<'a, D>
where
    D: Dictionary + SubstringDictionary + Clone + Send + Sync,
    D::Node: BidirectionalDictionaryNode,
    <D::Node as DictionaryNode>::Unit: Into<u32>,
{ /* query, max_distance, algorithm, results: Vec<WallBreakerResult>, state_map, cache, … */ }
```

The bounds are stricter than the other variants and encode the algorithm's requirements:

- `SubstringDictionary` — the dictionary must answer `find_exact_substring` (the **SCDAWG** does);
- `BidirectionalDictionaryNode` — nodes must expose `parent()`/`parent_label()` (for left extension)
  as well as `edges()` (for right extension);
- `Unit: Into<u32>` — the same code serves byte and Unicode dictionaries.

The dictionary itself is **not stored** in the WFST — only a `PhantomData<&'a D>` lifetime; the query
is run against it at construction and the results are cached.

## 3. Constructors, builder, and methods

```rust,ignore
impl<'a, D> WallBreakerWfst<'a, D> {
    pub fn new(dictionary: &'a D, query: &str, max_distance: usize) -> Self;          // Algorithm::Standard
    pub fn with_algorithm(dictionary: &'a D, query: &str, max_distance: usize, algorithm: Algorithm) -> Self;
    pub fn query(&self) -> &str;
    pub fn max_distance(&self) -> usize;
    pub fn algorithm(&self) -> Algorithm;
    pub fn num_results(&self) -> usize;     // how many matched terms
}

impl<'a, D> WallBreakerWfstBuilder<'a, D> {
    pub fn new(dictionary: &'a D) -> Self;
    pub fn query(self, query: &str) -> Self;
    pub fn max_distance(self, distance: usize) -> Self;          // default 2
    pub fn algorithm(self, algorithm: Algorithm) -> Self;        // default Standard
    pub fn standard(self) -> Self;
    pub fn transposition(self) -> Self;
    pub fn merge_and_split(self) -> Self;
    pub fn build(self) -> Result<WallBreakerWfst<'a, D>, String>; // Err("Query not set") if no query
}
```

> **Eager construction.** `new`/`with_algorithm`/`build` run the whole WallBreaker query immediately:
> internally `WallBreaker::with_algorithm(dict, k, algo).query(query).collect()` populates
> `results: Vec<WallBreakerResult>`. Constructing a `WallBreakerWfst` therefore *does the work*; the
> WFST is a view over the answer, not a lazy search of the dictionary. `num_results()` tells you how
> many terms matched.

## 4. The state forest

The cached results are presented as a lazy WFST shaped like a **forest of linear chains**:

<img src="../diagrams/wallbreaker-state-forest.svg" alt="A super-start state fans out one identity-labelled chain per matched term; terminals carry the edit distance" width="820"/>

- a single **super-start** state (sentinel key `result_index = u32::MAX, char_position = 0`);
- one **identity-labelled linear chain** per matched term — each transition is `c : c` on the term's
  characters;
- each chain's accepting **terminal** carries `final_weight = TropicalWeight(distance)`.

So an accepted path's tropical weight equals the term's edit distance, and the WFST composes like any
other. State ids are dense `u32`s keyed by `WallBreakerStateKey { result_index, char_position }`.
Because WallBreaker has already materialized the finite result set, construction pre-registers every
reachable chain state: the super-start plus one state per emitted character position in every
non-empty result.

## 5. StateSource and LazyWfst compatibility

`WallBreakerWfst` supports both WFST driving paths:

- `StateSource::compute_state(&self, state)` computes a `LazyState::Computed` value from the
  pre-registered state forest and can be used by immutable composition wrappers.
- `LazyWfst::expand(&mut self, state)` and `transitions_lazy(&mut self, state)` use the same
  pre-registered state ids, adding only a transition-cache layer for direct mutable traversal.

This split keeps composition pure while preserving cache policies for direct use. `clear_cache()`
clears only computed transition/finality caches; it retains the state-key registry because the
registry is part of the immutable result forest, not a cache.

## 6. Example

```rust,ignore
use duallity::{WallBreakerWfst, WallBreakerWfstBuilder};
use liblevenshtein::transducer::Algorithm;
use libdictenstein::scdawg::Scdawg;
use lling_llang::prelude::*;

// WallBreaker needs a substring dictionary (SCDAWG).
let scdawg = Scdawg::<()>::from_terms(vec!["hello", "help", "world"]);

let mut wb = WallBreakerWfst::new(&scdawg, "helo", 2);     // runs the query eagerly
assert_eq!(wb.query(), "helo");
assert!(wb.num_results() > 0);

let s0 = wb.start();
wb.expand(s0);            // drive via LazyWfst (not StateSource)
assert!(wb.is_expanded(s0));

// Builder form, with a metric choice:
let wb2 = WallBreakerWfstBuilder::new(&scdawg)
    .query("helo").max_distance(2).transposition()
    .build()
    .expect("query was set");
assert_eq!(wb2.algorithm(), Algorithm::Transposition);
```

## See also

- [theory/06 · WallBreaker and the wall effect](../theory/06-wallbreaker-and-the-wall-effect.md)
- [guides/02 · Choosing a variant](../guides/02-choosing-a-variant.md) (when large `k` justifies WallBreaker)
- [architecture/04 · Lazy evaluation](../architecture/04-lazy-evaluation-and-caching.md)
