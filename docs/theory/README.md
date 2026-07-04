# Theory

This section builds, from first principles, the theory behind `duallity`: weighted finite-state
transducers (WFSTs), semirings, Levenshtein automata, composition, universal automata, the
WallBreaker algorithm, and the expressivity limits of regular transducers. It is the conceptual
foundation the [architecture](../architecture/) and [design](../design/) sections build on.

Read it in order if you are new to WFSTs; jump to a numbered chapter if you only need one idea.

| # | Document | What you will learn |
|---|----------|---------------------|
| 01 | [Semirings and WFSTs](01-semirings-and-wfsts.md) | What a WFST is, the semiring abstraction, and why duallity uses the tropical `(min, +)` semiring. |
| 02 | [Edit distance and Levenshtein automata](02-edit-distance-and-levenshtein-automata.md) | Levenshtein distance, the edit lattice, and the automaton that accepts everything within distance `k`. |
| 03 | [The Levenshtein automaton as a transducer](03-levenshtein-as-transducer.md) | How the four edit operations become labelled, weighted transitions with query-side input and dictionary-side output. |
| 04 | [Composition](04-composition.md) | `T₁ ∘ T₂`, lazy composition, and why a fuzzy matcher must *be* a WFST to participate. |
| 05 | [Universal automata](05-universal-automata.md) | The query-agnostic automaton, characteristic vectors, and reuse across queries. |
| 06 | [WallBreaker and the wall effect](06-wallbreaker-and-the-wall-effect.md) | The combinatorial "wall" at large `k`, the pigeonhole split, exact-substring seeding, and bidirectional extension. |
| 07 | [Regular-language limits](07-regular-language-limits.md) | What a Levenshtein/phonetic WFST can and cannot express, positioned in the Chomsky hierarchy. |

---

## Master notation

Every symbol used across the documentation is defined **here, once**, and referenced thereafter.
Mathematical expressions are written with Unicode and quoted in backticks.

### Strings and alphabets

| Symbol | Meaning |
|--------|---------|
| `Σ` | the **alphabet** — the set of symbols. duallity works per Unicode scalar value (`char`), so `Σ ⊆ char`. |
| `ε` | the **empty string** / the **epsilon** label on a transition tape (consumes/produces nothing). |
| `q` | the **query** string (the misspelled input being corrected). |
| `n` | the length of the query, `n = |q|` (counted in Unicode scalars). |
| `w` | a **dictionary term** (a candidate correction / word in the dictionary `D`). |
| `m` | the length of a dictionary term, `m = |w|`. |
| `D` | the **dictionary** — a `libdictenstein` container (DAWG, SCDAWG, …) of terms. |
| `k` | the **maximum edit distance** (error bound); also written `max_distance`. |

### Edit distance

| Symbol | Meaning |
|--------|---------|
| `dₗₑᵥ(q, w)` | the **Levenshtein (edit) distance** between `q` and `w`: the minimum number of single-character insertions, deletions, and substitutions that turn one into the other. |
| `dₜ(q, w)` | the **Damerau–Levenshtein distance**: as above, plus adjacent **transposition** as a unit-cost operation. |

### Semirings and weights

| Symbol | Meaning |
|--------|---------|
| `𝕂` | a **semiring** `(K, ⊕, ⊗, 0̄, 1̄)` — the algebra of weights. |
| `⊕` | the semiring **plus** (combines *alternative* paths). |
| `⊗` | the semiring **times** (combines weights *along* one path). |
| `0̄` | the **additive identity** / annihilator (`a ⊕ 0̄ = a`, `a ⊗ 0̄ = 0̄`) — "no path / forbidden". |
| `1̄` | the **multiplicative identity** (`a ⊗ 1̄ = a`) — "a free step". |
| **tropical** | the semiring duallity uses: `(ℝ ∪ {+∞}, min, +, +∞, 0)`. Here `⊕ = min`, `⊗ = +`, `0̄ = +∞`, `1̄ = 0`. |

> ⚠️ **Naming gotcha.** In `lling_llang`, `TropicalWeight::zero()` is the value **`+∞`** (the
> additive identity `0̄`, meaning "no path"), and `TropicalWeight::one()` is the value **`0`** (the
> multiplicative identity `1̄`, a free step). The method names follow the *algebraic* role, not the
> numeric value. This is the single most common point of confusion; it is called out wherever it matters.

### Transducers and composition

| Symbol | Meaning |
|--------|---------|
| `T` | a **weighted finite-state transducer** (WFST): an automaton whose transitions carry `input : output / weight`. |
| `T(x, y)` | the weight `T` assigns to the input/output string pair `(x, y)` — the `⊕`-sum over all paths reading `x` and writing `y`. |
| `∘` | **composition**: `(T₁ ∘ T₂)` reads what `T₁` reads, writes what `T₂` writes, and matches `T₁`'s output tape against `T₂`'s input tape. |

### State encoding (the product automaton)

| Symbol | Meaning |
|--------|---------|
| `StateId` | a single `u32` identifying a state of a duallity WFST. |
| `d` | a **dictionary-node id** (one component of a product state). |
| `a` | an **automaton-state id** (the other component — e.g. the query position, a universal-automaton state, or a product-automaton state). |
| `M` | `max_automaton_states`, the **radix** of the encoding. For the parameterized standard Levenshtein WFST, `M = (n+1)·(k+1)`; edit variants with continuation states reserve additional disjoint ranges. |
| encode | `StateId = d · M + a`. |
| decode | If `M > 0`, `d = StateId / M` and `a = StateId mod M`; otherwise decoding is invalid. |

### Universal automata

| Symbol | Meaning |
|--------|---------|
| `V` | a **position variant** (`Standard`, `Transposition`, `MergeAndSplit`) — the type parameter selecting the metric. |
| `s_n(w, i)` | the **relevant subword** of `w` around position `i` for distance `n`: `w[i−n .. min(|w|, i+n+1)]`, padded with `$` for out-of-range positions (Schulz & Mihov). |
| `χ(c, s)` | the **characteristic vector** of a character `c` over a window `s`: the bit vector whose `j`-th bit is 1 iff `s[j] = c`. |

---

## Diagram conventions

All diagrams in this documentation use one shared color legend (see
[`../diagrams/README.md`](../diagrams/README.md)): `liblevenshtein` = red-pink, `libdictenstein` =
green, `duallity` = blue, `lling-llang` = yellow, output = purple; query/input tape = orange,
dictionary/output tape = teal; match = green, substitute = red, insert = blue, delete = orange;
accepting states = gold.
