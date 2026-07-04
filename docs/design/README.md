# Design — the WFST variants

duallity ships a family of WFST wrappers, every one implementing `Wfst<char, TropicalWeight>` (and
`LazyWfst`), so every one is composable. This section documents each variant in depth: its API, its
exact semantics, its honest limitations, and worked examples. Pick a variant by **what you are
matching**.

## Variant selection matrix

| Variant | Type(s) | Pick it when | Notes |
|---------|---------|--------------|-------|
| [**Levenshtein**](levenshtein-wfst.md) | `LevenshteinWfst<D>` | you need the core edit-distance matcher; small-to-moderate `k`; one query at a time | the place to start |
| [**Universal**](universal-wfst.md) | `UniversalLevenshteinWfst<V, D>`, `BoundUniversalWfst<V, D>` | you run **many queries** against the same dictionary and `k` | builds the automaton once, reuses it; variant `V` ∈ {Standard, Transposition, MergeAndSplit} |
| [**WallBreaker**](wallbreaker-wfst.md) | `WallBreakerWfst<'a, D>`, `WallBreakerWfstBuilder` | `k` is **large** over a big dictionary | needs a `SubstringDictionary` (SCDAWG); runs the query eagerly |
| [**Generalized**](generalized-wfst.md) | `GeneralizedWfst<D>`, `GeneralizedWfstBuilder` | you want a **runtime-configurable** operation set | lazy product graph with continuation states for multi-symbol operations |
| [**Rewrite**](phonetic-rewrite-wfst.md) | `RewriteWfst`, `RewriteRule`, `CommonPhoneticRules` | rule-based phonetic rewriting (`ph→f`); no feature needed | unconditional rules; expand context into explicit rules |
| [**Phonetic NFA**](phonetic-nfa-wfst.md) | `PhoneticNfaWfst` | a bare phonetic-regex transducer | feature `phonetic-rules` |
| [**Phonetic**](phonetic-wfst.md) | `PhoneticWfst<D>`, `PhoneticWfstBuilder` | sound-alike matching over a dictionary from a regex | feature `phonetic-rules` |
| [**Pipeline builder**](phonetic-pipeline-builder.md) | `PhoneticPipelineBuilder`, `PhoneticPipelineConfig`, `PhoneticMatch` | a fluent front-end that emits any of the above stages | ⚠ does not itself compose/search |

A task-oriented decision guide (with thresholds and trade-offs) is in
[guides/02 · Choosing a variant](../guides/02-choosing-a-variant.md).

## Shared shape

Every variant follows the same contract, established in [theory](../theory/) and
[architecture](../architecture/):

- **labels** — input = query side, output = dictionary side ([theory/03](../theory/03-levenshtein-as-transducer.md));
- **weights** — tropical, lower is better ([theory/01](../theory/01-semirings-and-wfsts.md));
- **state ids** — `(dict_node, automaton_state)` packed into a `u32`
  ([architecture/03](../architecture/03-state-encoding-and-product-space.md));
- **laziness** — states computed on first touch, cached
  ([architecture/04](../architecture/04-lazy-evaluation-and-caching.md)).

Each page below states where a variant *departs* from this shape — most importantly, the **honest
limitations** flagged with ⚠.
