# 04 · Composition

> **Prerequisites:** [01 · Semirings and WFSTs](01-semirings-and-wfsts.md),
> [03 · The Levenshtein automaton as a transducer](03-levenshtein-as-transducer.md).
> **Defines:** `T₁ ∘ T₂`, lazy composition, why a fuzzy matcher must *be* a WFST.

## 1. The composition operation

Composition is the operation that turns a *collection* of transducers into a *pipeline*. Given
`T₁` relating tape `x` to tape `y`, and `T₂` relating tape `y` to tape `z`, the composition
`T₁ ∘ T₂` relates `x` directly to `z` by matching `T₁`'s **output** against `T₂`'s **input**:

```
(T₁ ∘ T₂)(x, z)  =   ⊕   [ T₁(x, y) ⊗ T₂(y, z) ]
                    over y
```

In the tropical semiring (`⊕ = min`, `⊗ = +`) this is:

```
(T₁ ∘ T₂)(x, z)  =  min over y  [ T₁(x, y) + T₂(y, z) ]
```

— *the cheapest way to get from `x` to `z` through some intermediate tape `y`*. This is the algebraic
heart of duallity. A Levenshtein WFST maps a query `x` to dictionary terms `y` with edit-distance
weights; compose it with a phonetic-rewrite transducer or an n-gram language model `T₂` that scores
`y → z`, and the composite scores corrections by **edit distance + downstream cost** in a single
object (Mohri, Pereira & Riley, 2002 [[1]](#references)).

<img src="../diagrams/composition-pipeline.svg" alt="Query and dictionary become a Levenshtein WFST, composed with a downstream transducer, searched by shortest path" width="820"/>

## 2. Why the matcher must *be* a WFST

Before duallity, a Levenshtein matcher in this family was a closed procedure: hand it a query and a
dictionary, get back "all terms within distance `k`". That output is a *set*, not an *algebraic
object* — you cannot feed a set into the `min over y` fold above, because the intermediate weights
`T₁(x, y)` have been discarded.

By making the matcher satisfy `Wfst<char, TropicalWeight>`, duallity keeps the weights *attached to
the structure*. The matcher becomes a legitimate `T₁` that `lling_llang::composition::compose` can
fold against any `T₂`:

```rust,ignore
use duallity::LevenshteinWfst;
use lling_llang::composition::compose;

let lev      = LevenshteinWfst::new(&dict, "helo", 2);   // T₁ : query → term, weight = edit distance
let composed = compose(lev, language_model);             // T₁ ∘ T₂  (lazy)
for path in composed.accepting_paths() {                 // shortest paths = best corrections
    println!("{:?}  weight {:?}", path.labels(), path.weight());
}
```

This is the entire raison d'être of the crate: *make the Levenshtein matcher a `T₁` you can feed into
the `min-over-y` fold.*

## 3. Lazy composition

The composed transducer's state space is the **Cartesian product** of the two operands' state spaces.
Materializing it eagerly would be ruinous — millions of dictionary nodes times the states of the
downstream model. `lling_llang::composition::compose` instead returns a **lazy** composition:

```rust,ignore
pub fn compose<F1, F2, L, W>(fst1: F1, fst2: F2) -> LazyComposition<F1, F2, L, W>
where F1: Wfst<L, W>, F2: Wfst<L, W>, L: Clone + Eq + Hash, W: Semiring;
```

A product state `(s₁, s₂)` is computed only when a shortest-path search actually visits it. Because
each operand is *itself* lazy (chapter [architecture/04](../architecture/04-lazy-evaluation-and-caching.md)),
the pipeline never pays for the full product — it pays only for the corner of it that the search
explores. This laziness is what makes "compose a Levenshtein automaton over a million-word dictionary
with a language model" tractable.

The builder front-ends in duallity (e.g. `PhoneticPipelineBuilder`) produce the *stages* of such a
pipeline; the composition itself is performed by the caller with `compose` / `LazyWfstWrapper`:

<img src="../diagrams/composed-pipeline-typestate.svg" alt="A builder produces WFST stages; the caller composes and searches them" width="820"/>

## 4. Associativity and pipelines of three or more

Composition is associative: `(T₁ ∘ T₂) ∘ T₃ = T₁ ∘ (T₂ ∘ T₃)`. So an arbitrarily long pipeline —
*phonetic rewrite* `∘` *Levenshtein* `∘` *language model* — is well-defined regardless of grouping,
and the tropical weight of a complete path is simply the sum of the per-stage costs. A worked
end-to-end pipeline appears in [guides/03 · Composing pipelines](../guides/03-composing-pipelines.md).

## References

1. Mohri, M., Pereira, F., & Riley, M. (2002). *Weighted Finite-State Transducers in Speech
   Recognition.* Computer Speech & Language 16(1), 69–88.
   [doi:10.1006/csla.2001.0184](https://doi.org/10.1006/csla.2001.0184).
2. Mohri, M. (2009). *Weighted Automata Algorithms.* In *Handbook of Weighted Automata*, 213–254.
   Springer. [doi:10.1007/978-3-642-01492-5_6](https://doi.org/10.1007/978-3-642-01492-5_6).
