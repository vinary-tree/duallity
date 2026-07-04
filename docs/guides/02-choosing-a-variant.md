# 02 · Choosing a variant

duallity ships several WFST variants. This guide picks one from your situation. The full API for each
is in [design](../design/README.md).

## Decision flow

```
Are you matching phonetically (sound-alike)?
├─ yes, with rules (ph→f)          → RewriteWfst            (compose in front of a Levenshtein WFST)
├─ yes, with a regex ((ph|f)one)   → PhoneticWfst           (feature phonetic-rules)
└─ no ↓

Is the edit-distance bound k large (say ≥ 4–5) over a big dictionary?
├─ yes                             → WallBreakerWfst         (needs an SCDAWG / SubstringDictionary)
└─ no ↓

Will you run many queries against the SAME dictionary and k?
├─ yes                             → BoundUniversalWfst      (build the automaton once, reuse it)
└─ no ↓

Do you need a runtime-configurable operation set (merge/split, custom)?
├─ yes                                  → GeneralizedWfst   (runtime OperationSet)
└─ otherwise                            → LevenshteinWfst    (the default — start here)
```

## At a glance

| Variant | Best for | `max_distance` type | Needs | Caveat |
|---------|----------|---------------------|-------|--------|
| [`LevenshteinWfst`](../design/levenshtein-wfst.md) | the common case; small–moderate `k` | `usize` | any `char` dictionary | fixed operation set |
| [`BoundUniversalWfst`](../design/universal-wfst.md) | many queries, one dictionary | `u8` | any `char` dictionary | edit cost is final-weighted |
| [`WallBreakerWfst`](../design/wallbreaker-wfst.md) | large `k`, big dictionary | `usize` | `SubstringDictionary` (SCDAWG) | eager; `LazyWfst`-only |
| [`GeneralizedWfst`](../design/generalized-wfst.md) | configurable operation set | `u8` | `Unit = char` dictionary | operation arities count Unicode scalar values |
| [`RewriteWfst`](../design/phonetic-rewrite-wfst.md) | rule-based phonetics | — | none | unconditional rules; expand context into explicit rules |
| [`PhoneticWfst`](../design/phonetic-wfst.md) | regex phonetics over a dictionary | `u8` | feature `phonetic-rules` | wide labels are finite-alphabet-relative |

## Notes on the thresholds

- **Why WallBreaker only at large `k`?** Its strength is jumping the "wall" that grows with `k`
  ([theory/06](../theory/06-wallbreaker-and-the-wall-effect.md)). At small `k` the plain automaton's
  band is already narrow, so `LevenshteinWfst`/universal are simpler and avoid the SCDAWG requirement.
- **Why universal for many queries?** It builds the query-agnostic automaton once and reuses it
  ([theory/05](../theory/05-universal-automata.md)); per-query cost is the dictionary walk plus
  final-weight extraction from the active universal positions.
- **Damerau–Levenshtein (adjacent transpositions)** works in `LevenshteinWfst`,
  `BoundUniversalWfst::<Transposition, _>`, and `GeneralizedWfstBuilder::with_transposition()`.
  Choose `LevenshteinWfst` for one query over one dictionary and the universal variant when many
  queries can reuse the same automaton.
- **Merge/split edits** work in `LevenshteinWfst::with_algorithm(..., Algorithm::MergeAndSplit)` for
  the fixed OCR-style arities. Choose `GeneralizedWfst` when those arities or weights need to be
  composed at runtime with custom operations.

## Dictionary backends

All variants take a libdictenstein dictionary. Common choices:

| Backend | Unit | Good for |
|---------|------|----------|
| `DynamicDawgChar` | `char` (u32) | general-purpose Unicode; updatable at runtime |
| `DoubleArrayTrieChar` | `char` | static, read-heavy dictionaries |
| `Scdawg` / SCDAWG | — | **required** by `WallBreakerWfst` (substring search) |

See [guides/05 · Performance and tuning](05-performance-and-tuning.md) for the read/update trade-offs.
