# 05 · Universal automata

> **Prerequisites:** [02 · Edit distance and Levenshtein automata](02-edit-distance-and-levenshtein-automata.md).
> **Defines:** the query-agnostic universal automaton, the characteristic vector `χ`, the relevant
> subword `s_n(w, i)`.

## 1. Two ways to build a Levenshtein automaton

The automaton of chapter [02](02-edit-distance-and-levenshtein-automata.md) is **parameterized by the
query**: its transitions mention specific query characters, so a fresh automaton must be built for
every query. That is fine for a one-off correction but wasteful for a service that answers thousands
of queries against the same dictionary and the same bound `k`.

Mihov & Schulz (2004) [[1]](#references) introduced the **universal Levenshtein automaton**: a single
automaton, built **once per `max_distance`**, that is **independent of the query**. Its states are not
"query position `i`" but abstract sets of *positions-with-errors*; the query and the dictionary term
enter only through a small bit vector computed on the fly. duallity wraps it as
`UniversalLevenshteinWfst<V, D>` and the reuse factory `BoundUniversalWfst<V, D>`.

## 2. The characteristic vector and the relevant subword

To take a transition, the universal automaton needs to know, for the next dictionary character `c`,
*where `c` matches the query in the neighbourhood of the current position*. That information is the
**characteristic vector** `χ(c, s)`: a bit vector over a window `s` of the query, whose `j`-th bit is
1 iff `s[j] = c`.

The window `s` is the **relevant subword** `s_n(w, i)` — the slice of the word around position `i`
that a distance-`n` automaton can possibly care about (Schulz & Mihov; the implementation follows the
thesis definition):

```
s_n(w, i) = w[i − n  ..  v]        where  v = min(|w|, i + n + 1)
```

Positions below `1` are padded with the sentinel `$` (out of bounds). The window therefore spans at
most `2n+1` characters centred on the current position — exactly the diagonal band of chapter
[02](02-edit-distance-and-levenshtein-automata.md), now slid along the term.

<img src="../diagrams/characteristic-vector-window.svg" alt="The relevant-subword window over a term and the characteristic bit vector for a character" width="780"/>

In the figure, `w = "hello"`, `n = 1`, position `i = 2`. The window is `s₁("hello", 2) = "ell"`, and
for the candidate character `c = 'l'` the characteristic vector is `χ('l', "ell") = [0, 1, 1]`. The
universal automaton consumes this `[0, 1, 1]`, not the literal characters — which is precisely why
the *same* automaton serves every query: only the bit vector changes.

## 3. Reuse across queries: `BoundUniversalWfst`

duallity exposes the reuse explicitly. `BoundUniversalWfst<V, D>` holds a dictionary, a position
variant `V`, and a `max_distance`; its `UniversalAutomaton<V>` is built once. Each call to
`with_query(q)` mints a fresh lazy `UniversalLevenshteinWfst` that **shares** that automaton:

<img src="../diagrams/universal-bound-factory.svg" alt="One UniversalAutomaton built once; many per-query WFSTs share it" width="820"/>

```rust,ignore
use duallity::BoundUniversalWfst;
use liblevenshtein::transducer::universal::Standard;
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;

let dict  = DynamicDawgChar::<()>::from_terms(vec!["hello", "world", "help"]);
let bound = BoundUniversalWfst::<Standard, _>::new(dict, 2);   // automaton built ONCE
let w1 = bound.with_query("helo");   // lazy WFST, shares the automaton
let w2 = bound.with_query("wrld");   // another query, same automaton
```

## 4. Position variants

The type parameter `V: PositionVariant` selects the metric the universal automaton encodes:

| `V` | Metric | Adds |
|-----|--------|------|
| `Standard` | Levenshtein | match, substitute, insert, delete |
| `Transposition` | Damerau–Levenshtein | adjacent-swap as a unit-cost operation |
| `MergeAndSplit` | merge/split | one↔two character merges and splits (useful for OCR) |

These are the same families catalogued for the generalized automaton in chapter
[07](07-regular-language-limits.md).

## 5. How duallity binds the theory to a WFST

The universal automaton is an acceptor over bit-vector sequences; duallity adds WFST labels around
that acceptor. For a query `q` and a dictionary path prefix of depth `d`, duallity treats `q` as the
fixed word `w` and the dictionary path as the processed input. The next dictionary character `c`
therefore uses:

```
s_n(q, d + 1)
χ(c, s_n(q, d + 1))
```

The product state stores the dictionary node, the universal state, and the exact consumed
query-label cursor. The dictionary depth is stored in `DepthDictionaryNodeRegistry`; the query-label
cursor is part of the `UniversalStateRegistry` key. This is why the implementation no longer
estimates a query position from abstract universal offsets.

Edit cost is attached to the final weight, not to each dictionary-edge transition. The universal
state's active positions determine the minimum accepting error count using the same Proposition 11
criterion as the underlying automaton: remaining fixed-word characters must fit within the remaining
error budget. Dictionary-edge and deletion-continuation transitions carry zero local weight and only
spell the input/output label pair.

## References

1. Mihov, S., & Schulz, K. U. (2004). *Fast Approximate Search in Large Dictionaries.* Computational
   Linguistics 30(4), 451–477. [doi:10.1162/0891201042544938](https://doi.org/10.1162/0891201042544938).
2. Schulz, K. U., & Mihov, S. (2002). *Fast String Correction with Levenshtein Automata.* IJDAR 5(1),
   67–85. [doi:10.1007/s10032-002-0082-8](https://doi.org/10.1007/s10032-002-0082-8).
