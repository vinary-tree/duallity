# Universal Levenshtein WFST

> **`UniversalLevenshteinWfst<V, D>`** and the reuse factory **`BoundUniversalWfst<V, D>`** — a
> Levenshtein WFST backed by liblevenshtein's **query-agnostic** universal automaton. The structure
> is built once per `max_distance` and reused across queries. Supporting type:
> `UniversalLevenshteinStateSource<V, D>`. Always available (no feature flag).

## 1. Intuition

The [Levenshtein WFST](levenshtein-wfst.md) rebuilds a fresh automaton for every query. The universal
WFST builds **one** automaton per `max_distance` and feeds each query through it as a stream of
characteristic vectors ([theory/05](../theory/05-universal-automata.md)). When you correct thousands
of queries against the same dictionary and bound, this amortizes construction to near zero.

<img src="../diagrams/universal-bound-factory.svg" alt="One UniversalAutomaton built once; many per-query WFSTs share it" width="820"/>

## 2. Types and bounds

```rust,ignore
pub struct UniversalLevenshteinWfst<V, D>
where
    V: PositionVariant + Clone + Send + Sync,
    V::State: Send + Sync,
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
{ /* state_source, cache, max_distance: u8, … */ }

pub struct BoundUniversalWfst<V, D> where /* same bounds */ { /* dictionary, max_distance: u8, V */ }
```

The position variant `V` selects the metric:

| `V` (from `liblevenshtein::transducer::universal`) | Metric |
|----|--------|
| `Standard` | Levenshtein |
| `Transposition` | Damerau–Levenshtein (real adjacent-swap support) |
| `MergeAndSplit` | one↔two character merge/split (OCR) |

> Note: `max_distance` here is a **`u8`** (the universal automaton is parameterized at the type/byte
> level), whereas `LevenshteinWfst` takes a `usize`.

## 3. Constructors and methods

```rust,ignore
impl<V, D> UniversalLevenshteinWfst<V, D> {
    pub fn new(dictionary: &D, query: &str, max_distance: u8) -> Self;
    pub fn max_distance(&self) -> u8;
    pub fn query(&self) -> &str;
    pub fn set_max_cache_size(&mut self, size: usize);
}

impl<V, D> BoundUniversalWfst<V, D> {
    pub fn new(dictionary: D, max_distance: u8) -> Self;              // builds the automaton ONCE
    pub fn with_query(&self, query: &str) -> UniversalLevenshteinWfst<V, D>;  // mint a per-query WFST
    pub fn max_distance(&self) -> u8;
}
```

Both implement `Wfst`/`LazyWfst<char, TropicalWeight>` with the same surface as the parameterized
wrapper; the difference is entirely in the state source.

## 4. Semantics

`UniversalLevenshteinStateSource` walks the dictionary and treats the **query** as the fixed word
`w` from the universal-automaton formulation. At dictionary depth `d`, it builds the relevant-subword
window `s_n(query, d+1)` and the characteristic vector `χ(c, ·)` for the next dictionary character
`c` ([theory/05](../theory/05-universal-automata.md)), asks the universal automaton for its successor,
and emits a transition with the canonical orientation:

- **input** = the query character at the current position (`None` once the query is exhausted — an
  insertion-from-`ε`); **output** = the dictionary character `c`;
- **weight** = `0`; the edit distance is carried by the accepting state's final weight.

A product state is accepting iff the dictionary node is final and the universal state's active
positions satisfy the Proposition 11 acceptance criterion using:

- `|w| = query.len()`;
- processed input length `= dictionary depth`.

The final weight is the minimum accepting error count over the active positions. This avoids
double-counting: universal states expose the aggregate edit cost at acceptance, not a locally
attributable cost for each dictionary edge.

## 5. Exact cursors

The encoded `StateId` still has two packed components `(dictionary_node_id, automaton_state_id)`, but
the registries carry two exact cursors:

- `DepthDictionaryNodeRegistry` stores the dictionary depth for each dictionary-node id.
- `UniversalStateRegistry` keys each universal state by both its serialized position set and the
  consumed query-label cursor.

That means no query position is recovered from abstract universal offsets. Query labels advance
explicitly, and deletion-continuation transitions `(Some(q), None, 0)` let callers spell the full
input string even when the dictionary path is already at a final node. The relevant tests are
`test_universal_state_source_tracks_exact_query_position`,
`test_universal_state_source_weights_paths_by_final_edit_distance`, and
`test_universal_state_source_can_spell_full_label_pairs`.

## 6. Example

```rust,ignore
use duallity::BoundUniversalWfst;
use liblevenshtein::transducer::universal::{Standard, Transposition};
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use lling_llang::prelude::*;

let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "world", "help"]);

// Build the automaton ONCE, then mint many per-query WFSTs.
let bound = BoundUniversalWfst::<Standard, _>::new(dict.clone(), 2);
let mut w1 = bound.with_query("helo");
let mut w2 = bound.with_query("wrld");
w1.expand(w1.start());
w2.expand(w2.start());

// Real Damerau–Levenshtein via the Transposition variant.
let _swap = BoundUniversalWfst::<Transposition, _>::new(dict, 2).with_query("tset");
```

## See also

- [theory/05 · Universal automata](../theory/05-universal-automata.md)
- [design/levenshtein-wfst](levenshtein-wfst.md) (the parameterized counterpart)
- [guides/02 · Choosing a variant](../guides/02-choosing-a-variant.md)
- [guides/05 · Performance and tuning](../guides/05-performance-and-tuning.md)
