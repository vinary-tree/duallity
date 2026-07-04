# 02 · Edit distance and Levenshtein automata

> **Prerequisites:** [01 · Semirings and WFSTs](01-semirings-and-wfsts.md).
> **Defines:** Levenshtein distance, the edit lattice, the Levenshtein automaton.

## 1. Levenshtein distance

The **Levenshtein distance** `dₗₑᵥ(q, w)` between two strings is the minimum number of single-character
edits — **insertions**, **deletions**, and **substitutions** — needed to transform one into the other
(Levenshtein, 1966 [[1]](#references)). For example:

```
dₗₑᵥ("helo", "hello") = 1     insert one 'l'
dₗₑᵥ("tset", "test")  = 2     substitute, substitute   (one transposition if we allow it — see below)
dₗₑᵥ("kitten", "sitting") = 3 substitute k→s, substitute e→i, insert g
```

If we additionally allow **adjacent transposition** (swapping two neighbouring characters) as a
single unit-cost edit, we obtain the **Damerau–Levenshtein distance** `dₜ`, under which
`dₜ("tset", "test") = 1`. duallity exposes both through the `Algorithm` enum and the universal
`PositionVariant` types (chapters [03](03-levenshtein-as-transducer.md) and [05](05-universal-automata.md)).

## 2. The edit lattice

Edit distance has a classic dynamic-programming characterization (Wagner & Fischer, 1974
[[2]](#references)). Lay out a grid whose rows are positions in the query `q` (`i = 0 … n`) and whose
columns are positions in the term `w` (`j = 0 … m`). A node `(i, j)` means "`i` characters of the
query and `j` characters of the term have been consumed". Three kinds of edge leave each node:

| Edge | From → to | Operation | Cost |
|------|-----------|-----------|------|
| diagonal | `(i, j) → (i+1, j+1)` | **match** if `q[i] = w[j]`, else **substitute** | `0` / `1` |
| horizontal | `(i, j) → (i, j+1)` | **insert** a term character (query stays) | `1` |
| vertical | `(i, j) → (i+1, j)` | **delete** (term stays) | `1` |

The minimum-cost path from `(0, 0)` to `(n, m)` has total cost exactly `dₗₑᵥ(q, w)`. This is the
**edit lattice**:

<img src="../diagrams/levenshtein-edit-lattice.svg" alt="The edit lattice for query 'ac' versus term 'abc', with the minimum-cost path highlighted" width="820"/>

The example above aligns `q = "ac"` against `w = "abc"`: **match** `a`, **insert** `b`, **match** `c`
— total cost `1`. The diagonal/horizontal/vertical edges correspond precisely to the
substitute/insert/delete operations duallity emits as transitions; chapter
[03](03-levenshtein-as-transducer.md) makes the correspondence exact.

This grid orientation — query on one axis, term on the other — is the same one `duallity` uses
internally: a product state pairs a **query position** with a **dictionary node** (a position in the
term being traversed). The tropical weight of a path *is* the sum of edge costs, so by chapter
[01](01-semirings-and-wfsts.md) the **minimum path weight equals the edit distance**.

## 3. The Levenshtein automaton

Computing `dₗₑᵥ(q, w)` for *one* term `w` is the DP grid. But a spell checker must compare `q` against
an entire dictionary `D` of millions of terms. Re-running the grid per term is wasteful, because all
terms share prefixes.

The **Levenshtein automaton** for a query `q` and bound `k` is a finite automaton that accepts
**exactly** the set of strings within edit distance `k` of `q`:

```
L(q, k) = { w ∈ Σ*  :  dₗₑᵥ(q, w) ≤ k }
```

Schulz & Mihov (2002) [[3]](#references) showed this automaton can be built and run so that checking a
term takes time linear in `|w|`, independent of `n` once `k` is fixed. Crucially, by walking the
automaton **in lockstep with a trie/DAWG traversal of the dictionary**, every term that shares a
prefix shares the work of matching that prefix — the whole dictionary is filtered in one pass.

As a *weighted* automaton in the tropical semiring it does more than accept or reject:

> **The minimum path weight from the start state to an accepting state for a term `w` is exactly
> `dₗₑᵥ(q, w)`**, capped at `k`.

So the Levenshtein automaton is not a black box that merely lists "all terms within distance `k`";
it is a **transducer** that, for each accepted term, also reports the distance as a tropical weight.
That is what lets duallity expose it as a `Wfst<char, TropicalWeight>` and compose it with other
transducers.

### State space, the `2k+1` band, and the compact implementation radix

For a query of length `n` and bound `k`, only a **diagonal band** of the edit lattice is reachable:
a path can never stray more than `k` cells from the main diagonal without exceeding the budget. There
are therefore `O((n+1)·(2k+1))` reachable `(position, offset)` states in the classical banded view.

duallity's parameterized Levenshtein adapter uses a tighter concrete encoding for ordinary states:
it stores `(query_position, edit_cost)`, so the standard-edit radix is `(n+1)·(k+1)`. Algorithms
with one-step continuations, such as adjacent transposition or merge/split, reserve additional
disjoint ranges over the same normal lattice:

```rust,ignore
// lib.rs — state_encoding
pub fn bounded_levenshtein_states(query_len: usize, max_distance: usize) -> u32 {
    let positions = query_len + 1; // n + 1 query positions
    let costs = max_distance + 1;  // edit costs 0..k
    (positions * costs) as u32     // normal M = (n+1)·(k+1)
}
```

This `M` is the radix of the product-state encoding (chapter [03](03-levenshtein-as-transducer.md)
and [architecture/03](../architecture/03-state-encoding-and-product-space.md)). The older
`estimate_automaton_states` helper remains useful for components that need a conservative generic
band estimate; the parameterized adapter uses the compact exact lattice where it can.

## References

1. Levenshtein, V. I. (1966). *Binary codes capable of correcting deletions, insertions, and
   reversals.* Soviet Physics Doklady 10(8), 707–710.
2. Wagner, R. A., & Fischer, M. J. (1974). *The String-to-String Correction Problem.* Journal of the
   ACM 21(1), 168–173. [doi:10.1145/321796.321811](https://doi.org/10.1145/321796.321811).
3. Schulz, K. U., & Mihov, S. (2002). *Fast String Correction with Levenshtein Automata.*
   International Journal on Document Analysis and Recognition (IJDAR) 5(1), 67–85.
   [doi:10.1007/s10032-002-0082-8](https://doi.org/10.1007/s10032-002-0082-8).
