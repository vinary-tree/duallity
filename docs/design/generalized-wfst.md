# Generalized WFST

> **`GeneralizedWfst<D>`** and **`GeneralizedWfstBuilder<'a, D>`** — a runtime-configurable
> Levenshtein WFST whose **operation set** (standard, transposition, merge/split, phonetic digraphs)
> is chosen at build time. Always available (no feature flag).

> ## Implementation note — character-counted operation arities
>
> `GeneralizedWfst` now expands a real lazy product graph. Product states track
> `(dictionary node, query byte offset, accumulated weighted cost)`, and multi-symbol operations use
> continuation states so a `char` WFST can emit digraphs such as `ph → f`. The operation arities come
> from liblevenshtein's `OperationType` API and count Unicode scalar values. Restricted operations
> are still matched against the UTF-8 byte slices selected by those character counts, so custom
> Unicode substitutions can use `consume_x = 1` and `consume_y = 1` for a single non-ASCII character.

## 1. Intuition

Where [`LevenshteinWfst`](levenshtein-wfst.md) hardcodes the four standard edits, `GeneralizedWfst`
lets you assemble an **operation set** at runtime — adding transpositions, merges/splits, or phonetic
digraph rewrites (`ph↔f`, `ck↔k`) — and hands it to liblevenshtein's `GeneralizedAutomaton`.

<img src="../diagrams/generalized-builder-flow.svg" alt="The fluent builder selects an OperationSet and builds a GeneralizedWfst backed by a lazy product graph" width="860"/>

## 2. Type and bounds

```rust,ignore
pub struct GeneralizedWfst<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: DictionaryNode<Unit = char>,     // NOTE: the unit must BE char (stricter than other variants)
{ /* dictionary, query, OperationSet, product/continuation registries, cache, … */ }

pub struct GeneralizedWfstBuilder<'a, D>
where D: Dictionary + Clone + Send + Sync, D::Node: DictionaryNode<Unit = char>
{ /* dictionary, query, max_distance: u8 (default 2), operations: OperationSet (default standard) */ }
```

## 3. Constructor, builder, and methods

```rust,ignore
impl<D> GeneralizedWfst<D> {
    pub fn new(dictionary: &D, query: &str, max_distance: u8, operations: OperationSet) -> Self;
    pub fn query(&self) -> &str;
    pub fn max_distance(&self) -> u8;
}

impl<'a, D> GeneralizedWfstBuilder<'a, D> {
    pub fn new(dictionary: &'a D) -> Self;                 // defaults: max_distance 2, OperationSet::standard()
    pub fn query(self, query: &str) -> Self;
    pub fn max_distance(self, distance: u8) -> Self;
    pub fn with_standard_ops(self) -> Self;                // match/substitute/insert/delete
    pub fn with_transposition(self) -> Self;               // + adjacent swap
    pub fn with_merge_split(self) -> Self;                 // + merge / split
    pub fn with_phonetic_digraphs(self) -> Self;           // + ph↔f, ck↔k, … (restricted ops)
    pub fn with_operations(self, operations: OperationSet) -> Self;
    pub fn build(self) -> Result<GeneralizedWfst<D>, String>;   // Err("Query not set") if no query
}
```

## 4. The operation set

`OperationSet` (from `liblevenshtein::transducer`) is a collection of `OperationType`s, each tagged
`⟨consume_x, consume_y, weight, restriction⟩`: how many Unicode scalar values it consumes from the
dictionary side (`x`) and query side (`y`), at what cost, optionally restricted to a named UTF-8
byte-string pair set.

<img src="../diagrams/operationtype-taxonomy.svg" alt="The OperationType taxonomy: standard, transposition, merge/split, and phonetic digraphs" width="880"/>

| Preset | Operations |
|--------|-----------|
| `OperationSet::standard()` | match `⟨1,1,0⟩`, substitute `⟨1,1,1⟩`, insert `⟨0,1,1⟩`, delete `⟨1,0,1⟩` |
| `with_transposition()` | standard + transpose `⟨2,2,1⟩` |
| `with_merge_split()` | standard + merge `⟨2,1,1⟩` + split `⟨1,2,1⟩` |
| phonetic digraphs (`consonant_digraphs()`) | `2→1 ⟨2,1,0.15⟩` (`ch→k, sh→s, ph→f, th→t`), `1→2 ⟨1,2,0.15⟩`, `2→2 ⟨2,2,0.15⟩` (`qu↔kw`) |

## 5. Lazy product graph

`GeneralizedWfst` registers two kinds of state:

| State kind | Meaning |
|------------|---------|
| **Product** | `(dictionary node id, query byte offset, accumulated cost)` |
| **Continuation** | remaining input/output chars for one multi-symbol operation, plus the target product state |

Expansion enumerates operation-compatible dictionary paths of exactly `consume_x` characters and
query segments of exactly `consume_y` characters. The product state stores the query position as a
byte offset only so UTF-8 slicing stays O(1) after the segment end has been found. If the operation
applies and the accumulated cost remains within `max_distance`, the target product state is interned.
Single-symbol operations emit one transition directly; multi-symbol operations emit the first aligned
pair immediately and then use zero-cost continuation states for the remaining chars. This preserves
the `Wfst<char, TropicalWeight>` surface while supporting operations such as `ph : f / 0.15`.

In literate pseudocode:

```text
To expand product state (node, q, cost):
  for each operation op in OperationSet:
    for each dictionary path p from node with char_count(p) = op.consume_x:
      let (s, q_next) = next op.consume_y characters of query starting at byte offset q
      if op applies to (bytes(p), bytes(s)) and cost + op.weight ≤ max_distance:
        target = intern_product(last_node(p), q_next, cost + op.weight)
        emit p and s as either one arc or a continuation chain toward target
```

## 6. Example

```rust,ignore
use duallity::{GeneralizedWfst, GeneralizedWfstBuilder};
use liblevenshtein::transducer::OperationSet;
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use lling_llang::prelude::*;

let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help"]);

// Direct construction with an explicit operation set.
let mut g = GeneralizedWfst::new(&dict, "helo", 2, OperationSet::standard());
assert_eq!(g.query(), "helo");
g.expand(g.start());          // lazily registers reachable product states as transitions are read

// Builder with phonetic digraphs configured.
let g2 = GeneralizedWfstBuilder::new(&dict)
    .query("fone").max_distance(2).with_phonetic_digraphs()
    .build()
    .expect("query was set");
assert_eq!(g2.max_distance(), 2);
```

## See also

- [theory/07 · Regular-language limits](../theory/07-regular-language-limits.md) (the operation taxonomy in context)
- [design/universal-wfst](universal-wfst.md) (compile-time operation variants for many-query reuse)
- [design/phonetic-rewrite-wfst](phonetic-rewrite-wfst.md) (rule-based phonetic rewriting)
