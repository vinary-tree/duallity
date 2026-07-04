# 01 · Semirings and weighted finite-state transducers

> **Prerequisites:** none. **Defines:** WFST, semiring, the tropical `(min, +)` semiring.
> **Symbols** are from the [master notation](README.md#master-notation).

## 1. From automata to transducers

A **finite automaton** reads a string and answers *yes/no* — does it belong to the language? That
is too little information for spelling correction: we do not just want to know *whether* a word is
within edit distance `k`, we want to know *which* word, and *how far* away it is.

A **finite-state transducer (FST)** generalizes the automaton in two ways:

1. Each transition carries **two** labels — an **input** label and an **output** label — so the
   machine relates an *input tape* to an *output tape* instead of merely accepting one tape.
2. A **weighted** FST (WFST) additionally attaches a **weight** to each transition (and to each
   final state), drawn from a **semiring** `𝕂`. The weight records *how good* a path is.

A WFST is therefore a 6-tuple `T = (Σᵢ, Σₒ, Q, I, F, E)` where `Σᵢ`/`Σₒ` are the input/output
alphabets (here both `⊆ char`, with `ε` allowed), `Q` is a finite set of states, `I ⊆ Q` the start
state(s), `F ⊆ Q` the final states (each with a final weight), and `E ⊆ Q × (Σᵢ∪{ε}) × (Σₒ∪{ε}) × 𝕂 × Q`
the weighted transitions. We write a transition as `input : output / weight`.

<img src="../diagrams/transducer-two-tape.svg" alt="A single transducer transition carries an input label, an output label, and a weight" width="760"/>

In `duallity` the input tape is the **query** `q` (the misspelled word) and the output tape is the
**dictionary term** `w` (the candidate correction). A path from a start state to a final state spells
out an alignment of `q` against `w`, and its weight is the cost of that alignment.

## 2. Why a *semiring*?

We need two operations on weights, and they must interact in well-understood ways:

- One operation, `⊗` (**times**), combines the weights **along a single path** — it accumulates a
  path's cost as we step through its transitions.
- The other, `⊕` (**plus**), combines the weights of **alternative paths** to the same place — it
  chooses or sums over the different ways of relating the same `(input, output)` pair.

A **semiring** `𝕂 = (K, ⊕, ⊗, 0̄, 1̄)` is exactly the algebraic structure that makes this precise.
It satisfies these axioms (Mohri, 1997 [[1]](#references)):

| Axiom | Statement |
|-------|-----------|
| `⊕` is a commutative monoid | associative, commutative, identity `0̄`:  `a ⊕ 0̄ = a`. |
| `⊗` is a monoid | associative, identity `1̄`:  `a ⊗ 1̄ = 1̄ ⊗ a = a`. |
| `⊗` distributes over `⊕` | `a ⊗ (b ⊕ c) = (a⊗b) ⊕ (a⊗c)` and `(a ⊕ b) ⊗ c = (a⊗c) ⊕ (b⊗c)`. |
| `0̄` annihilates `⊗` | `a ⊗ 0̄ = 0̄ ⊗ a = 0̄`. |

With this structure, the weight a transducer assigns to a pair `(x, y)` is defined uniformly:

```
                       ⊕              ⊗
   T(x, y)  =      over all paths π    of the transition weights along π
                  reading x, writing y
```

That is: the weight along one path is the `⊗`-product of its edge weights; the weight of the pair
`(x, y)` is the `⊕`-sum of all paths carrying that pair.

## 3. The tropical `(min, +)` semiring

`duallity` works exclusively in the **tropical semiring**, `lling_llang`'s `TropicalWeight`:

```
𝕂 = (ℝ ∪ {+∞},  min,  +,  +∞,  0)
a ⊕ b = min(a, b)     the best (cheapest) alternative wins
a ⊗ b = a + b         costs add up along a path
0̄ = +∞                the additive identity — "no path"
1̄ = 0                 the multiplicative identity — "a free step"
```

<img src="../diagrams/tropical-semiring-algebra.svg" alt="The tropical (min, +) semiring: plus is min, times is plus, zero is +infinity, one is 0" width="720"/>

Substituting `(min, +)` into the uniform definition above collapses two abstract operations into
one familiar idea:

- "`⊗`-product along a path" becomes "**sum of edge costs along the path**";
- "`⊕`-sum over all paths" becomes "**minimum-cost path**".

So in the tropical semiring,

```
T(x, y)  =  min over paths reading x, writing y  of  (sum of edge weights)
```

**A shortest path is the best answer.** This is precisely why duallity can phrase fuzzy matching,
phonetic rewriting, and language-model rescoring as one search: build a WFST whose path weights are
the quantity you want to minimize, then run a shortest-path search. Mohri, Pereira & Riley (2002)
[[3]](#references) established the tropical semiring as the standard weight structure for exactly this
reason.

> ⚠️ **The `zero()` / `one()` gotcha (read this once).** `TropicalWeight::zero()` returns the value
> `+∞` and `TropicalWeight::one()` returns the value `0`. The names denote the *algebraic role*
> (`0̄` = additive identity, `1̄` = multiplicative identity), not the number printed. When a duallity
> state source returns `TropicalWeight::zero()` for a non-accepting state, it is saying "`+∞` — there
> is no accepting path here", **not** "cost zero". Internally, `lling_llang` verifies these laws
> against a machine-checked model.

The tropical semiring's laws are easy to confirm, and duallity's integration tests assert them
directly (`tests/wfst_integration.rs`):

```rust,ignore
use lling_llang::prelude::*;

assert_eq!(TropicalWeight::new(1.0).plus(&TropicalWeight::new(2.0)),  TropicalWeight::new(1.0)); // min
assert_eq!(TropicalWeight::new(1.0).times(&TropicalWeight::new(2.0)), TropicalWeight::new(3.0)); // +
assert!(TropicalWeight::zero().is_infinite());   // 0̄ = +∞
assert_eq!(TropicalWeight::one().value(), 0.0);  // 1̄ = 0
```

## 4. What duallity actually implements

Every WFST in duallity implements `lling_llang`'s `Wfst<char, TropicalWeight>` trait (and its lazy
extensions — see [architecture/02](../architecture/02-wfst-trait-surface.md)). The trait surface is
small:

```rust,ignore
pub trait Wfst<L, W: Semiring> {
    fn start(&self) -> StateId;
    fn is_final(&self, state: StateId) -> bool;
    fn final_weight(&self, state: StateId) -> W;
    fn transitions(&self, state: StateId) -> &[WeightedTransition<L, W>];
    fn num_states(&self) -> usize;
    // … provided helpers …
}
```

Because the weight type is a tropical `Semiring`, the moment a Levenshtein automaton satisfies this
trait it becomes an algebraic object you can compose (chapter [04](04-composition.md)) and search.

## References

1. Mohri, M. (1997). *Finite-State Transducers in Language and Speech Processing.* Computational
   Linguistics 23(2), 269–311. [ACL Anthology J97-2003](https://aclanthology.org/J97-2003/).
2. Droste, M., & Kuich, W. (2009). *Semirings and Formal Power Series.* In *Handbook of Weighted
   Automata*, 3–28. Springer. [doi:10.1007/978-3-642-01492-5_1](https://doi.org/10.1007/978-3-642-01492-5_1).
3. Mohri, M., Pereira, F., & Riley, M. (2002). *Weighted Finite-State Transducers in Speech
   Recognition.* Computer Speech & Language 16(1), 69–88.
   [doi:10.1006/csla.2001.0184](https://doi.org/10.1006/csla.2001.0184).
