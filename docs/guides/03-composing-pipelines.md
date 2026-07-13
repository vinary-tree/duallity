# 03 · Composing pipelines

Composition is what makes duallity more than a fuzzy matcher: chain a phonetic rewriter, the
Levenshtein matcher, and a language model into **one** weighted transducer, then take the shortest path
([theory/04](../theory/04-composition.md)). Because every duallity variant that *is* a transducer
implements `Wfst<char, TropicalWeight>`, any of them can be a stage.

<img src="../diagrams/composition-pipeline.svg" alt="A query and a dictionary become a Levenshtein WFST, composed with a downstream transducer, and searched by shortest path" width="820"/>

---

## 1. `compose`

`lling_llang::composition::compose(t1, t2)` returns a **lazy** `LazyComposition` that matches `t1`'s
**output** tape against `t2`'s **input** tape (the *matched-tape rule*), and combines their weights in
the shared semiring. In general, for input `` $`x`$ ``, output `` $`z`$ ``, and every intermediate tape
`` $`y`$ ``:

```math
(T_1 \circ T_2)(x, z) \;=\; \bigoplus_{y}\,\bigl[\, T_1(x, y) \otimes T_2(y, z) \,\bigr].
```

In duallity's **tropical** semiring (`` $`\oplus = \min`$ ``, `` $`\otimes = +`$ ``) this specializes to
a shortest-path fold — the composition weight of `` $`(x, z)`$ `` is the least total cost over all
intermediates:

```math
(T_1 \circ T_2)(x, z) \;=\; \min_{y}\,\bigl[\, T_1(x, y) + T_2(y, z) \,\bigr].
```

```rust,ignore
use duallity::{LevenshteinWfst, RewriteWfst, CommonPhoneticRules};
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use lling_llang::composition::compose;
use lling_llang::prelude::*;

let dict    = DynamicDawgChar::<()>::from_terms(vec!["foto", "fotos", "photon"]);
let rewrite = RewriteWfst::with_rules(CommonPhoneticRules::english())   // "ph" → "f", …
    .expect("preset rules carry valid costs");
let lev     = LevenshteinWfst::new(&dict, "foto", 1);

// rewrite ∘ levenshtein : normalize the query's digraphs, then fuzzy-match the dictionary.
// `accepting_paths` takes &mut self, so bind the composition as `mut`.
let mut composed = compose(rewrite, lev);
for path in composed.accepting_paths() {                // best-first: shortest paths first
    // ComposedPath has PUBLIC FIELDS, not accessor methods:
    //   path.inputs : Vec<char>       — the composition's input tape
    //   path.outputs: Vec<char>       — the composition's output tape (the corrected term)
    //   path.weight : TropicalWeight  — total tropical cost; .value() reads the f64
    println!("{:?} -> {:?}  weight {}", path.inputs, path.outputs, path.weight.value());
}
```

Composition is **associative**, so a three-stage pipeline is just nested `compose`:

```rust,ignore
// rewrite ∘ levenshtein ∘ language_model
let pipeline = compose(compose(rewrite, lev), language_model);
```

The weight of a complete accepting path is the sum of the per-stage tropical costs — phonetic-rewrite
cost `` $`+`$ `` edit distance `` $`+`$ `` language-model score.

---

## 2. A three-stage worked example, with per-stage weight math

Take `Rewrite ∘ Levenshtein ∘ LanguageModel`. A user types `"photo"`; the dictionary holds `{ "foto",
"fotos" }`; the caller supplies a `language_model` that relabels each term to itself with a unigram
cost. The **matched-tape rule** ties the stages together: `rewrite`'s output must equal `lev`'s query
tape (`"foto"`), and `lev`'s output (a dictionary term) must equal `language_model`'s input.

```rust,ignore
use duallity::{LevenshteinWfst, RewriteWfst, RewriteRule};
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use lling_llang::composition::compose;
use lling_llang::prelude::*;

let dict    = DynamicDawgChar::<()>::from_terms(vec!["foto", "fotos"]);
let rewrite = RewriteWfst::with_rules(vec![RewriteRule::with_cost("ph", "f", 0.1)
    .expect("finite, non-negative cost")]).expect("valid rule set");
let lev     = LevenshteinWfst::new(&dict, "foto", 1);   // the normalized query

// Rewrite ∘ Levenshtein ∘ language_model
let mut pipeline = compose(compose(rewrite, lev), language_model);
let best = pipeline.accepting_paths().next();           // the minimum-weight path
```

The surviving best path — input `"photo"`, output `"foto"` — decomposes as one addition per stage:

| Stage | Transducer | Contribution | Weight |
|---|---|---|---|
| 1 | `rewrite` | apply `ph → f`: `"photo" → "foto"` | `` $`0.1`$ `` |
| 2 | `lev` | `"foto"` matches term `"foto"` exactly | `` $`0.0`$ `` |
| 3 | `language_model` | unigram cost of `"foto"` | `` $`0.5`$ `` |
| | | **total** `` $`= 0.1 + 0.0 + 0.5`$ `` | **`` $`0.6`$ ``** |

The runner-up, output `"fotos"`, pays `` $`0.1`$ `` (rewrite) `` $`+`$ `` `` $`1.0`$ `` (one insert)
`` $`+`$ `` `` $`1.0`$ `` (its unigram cost) `` $`= 2.1`$ ``, so `"foto"` at `` $`0.6`$ `` is returned
first. Each `` $`+`$ `` is one `` $`\otimes`$ `` step; the `min` that picks `"foto"` over `"fotos"` is
the `` $`\oplus`$ ``.

---

## 3. `LazyWfstWrapper` — driving a `StateSource`

A [`StateSource`](../architecture/02-wfst-trait-surface.md#3-statesource--the-computation-kernel) is the
**immutable computation kernel**: a pure `compute_state(&self, s) -> LazyState`. It is what composition
is built around — `compose` visits product states and calls `compute_state` through a shared reference,
never needing `&mut` ([architecture/04 §7](../architecture/04-lazy-evaluation-and-caching.md#7-the-immutable--mutable-split)).
Wrap a bare `StateSource` in a `LazyWfstWrapper` to get a `Wfst` / `LazyWfst` you can compose or expand;
the wrapper layers a cache policy over the same state ids the source computes.

```rust,ignore
use duallity::LevenshteinStateSource;
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use lling_llang::prelude::*;                            // LazyWfstWrapper, Wfst, LazyWfst, …

let dict     = DynamicDawgChar::<()>::from_terms(vec!["hello", "help"]);
let source   = LevenshteinStateSource::new(&dict, "helo", 2);   // max_distance: usize
let mut wfst = LazyWfstWrapper::new(source);            // now a Wfst<char, TropicalWeight>

let start = wfst.start();
let edges = wfst.transitions_lazy(start);               // drive expansion (mutable path)
assert!(!edges.is_empty());
```

### `LazyWfstWrapper` vs. composing a variant directly

Both give you a composable `Wfst`; the choice is about *what you already hold*.

| Approach | You write | Use when | Notes |
|---|---|---|---|
| **Direct** — a variant is already a `Wfst` | `compose(RewriteWfst, LevenshteinWfst)` | you hold a variant type (the common case) | `LevenshteinWfst`, `RewriteWfst`, `GeneralizedWfst`, `WallBreakerWfst`, … all implement `Wfst` + `LazyWfst`. |
| **Wrapped** — you hold a bare kernel | `compose(LazyWfstWrapper::new(source), other)` | you hold a `StateSource` (e.g. `LevenshteinStateSource`) and want a `Wfst` | The wrapper is exactly the adapter `compose` uses over the immutable `compute_state` kernel; set its cache policy with `LazyWfstWrapper::with_cache_policy`. |

---

## 4. `DictionaryBackend` — the vocabulary layer

When a downstream stage needs lling-llang's lattice vocabulary (terms ↔ `VocabId`), adapt a dictionary
with [`DictionaryBackend`](../design/levenshtein-wfst.md) (design/levenshtein-wfst §7). It implements
lling-llang's `LatticeBackend` trait, interning terms lazily to sequential `VocabId`s. The `intern` and
`lookup` methods come from the `LatticeBackend` trait, which `lling_llang::prelude::*` brings into
scope.

```rust,ignore
use duallity::DictionaryBackend;
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use lling_llang::prelude::*;                            // LatticeBackend (intern/lookup), VocabId, …

let dict        = DynamicDawgChar::<()>::from_terms(vec!["hello", "world"]);
let mut backend = DictionaryBackend::new(dict);         // vocabulary starts empty (lazy)

let id = backend.intern("hello");                       // -> VocabId (stable; same word ⇒ same id)
assert_eq!(backend.lookup(id), Some("hello"));          // VocabId -> Option<&str>
assert_eq!(backend.intern("hello"), id);                // idempotent
```

Prefer `try_intern(&str) -> Option<VocabId>` when you must distinguish a real id from vocabulary
exhaustion (the `u32` `VocabId` space is finite; the infallible `intern` returns the reserved
`DictionaryBackend::VOCAB_ID_EXHAUSTED` sentinel when full, and `lookup` returns `None` for it).

---

## 5. Shortest-path search

A composed transducer's accepting paths, ranked by tropical weight, **are** the best corrections: the
minimum-weight path is the best answer ([theory/01 §3](../theory/01-semirings-and-wfsts.md)).
`accepting_paths()` yields them **best-first** (lowest weight first), so the first item is the top
correction and you can stop early for top-`` $`N`$ ``:

```rust,ignore
let mut composed = compose(rewrite, lev);
for path in composed.accepting_paths().take(5) {        // top-5, best-first
    println!("{:?}  weight {}", path.outputs, path.weight.value());
}
```

Read `path.outputs` for the corrected string (the output tape), `path.inputs` for the query-side tape,
and `path.weight.value()` for the total tropical cost.

<!--
  NEW DIAGRAM (optional) — D19 · compose-search-sequence
    rendered SVG : ../diagrams/compose-search-sequence.svg
    source       : docs/diagrams/src/compose-search-sequence.puml   (PlantUML sequence diagram)
    a sequence diagram of accepting_paths() driving compute_state across the lazy product;
    add to the diagrams/README catalog if rendered. Optional — the prose above is self-contained.
-->

---

## 6. WallBreaker is eager and borrows its dictionary

> ⚠️ **WallBreaker is a *view over a finished answer*, not an incremental searcher.**
> `WallBreakerWfst<'a, D>` **borrows** its dictionary (the `` $`'a`$ `` lifetime bounds the transducer
> and any composition over it), and `new` / `with_algorithm` / `build` run the **entire** WallBreaker
> query *eagerly at construction*, pre-registering the finite result-chain forest
> ([design/wallbreaker-wfst](../design/wallbreaker-wfst.md);
> [architecture/04 §7](../architecture/04-lazy-evaluation-and-caching.md#7-the-immutable--mutable-split)).
> It implements `Wfst` + `LazyWfst` + `StateSource` like the other variants — its `compute_state` is a
> **fully functional** pure read of that materialized forest, *not* a deferred `Pending` — so it
> composes normally; but because the answer is already computed, "laziness" buys nothing there, and the
> up-front cost is paid whether or not you enumerate every path.

As with every lazy variant, the immutable accessors read the cache, so when you traverse a WallBreaker
WFST **by hand**, drive expansion through the mutable `LazyWfst` surface before eager reads:

```rust,ignore
use duallity::WallBreakerWfst;
use libdictenstein::scdawg::Scdawg;
use lling_llang::prelude::*;

let scdawg  = Scdawg::<()>::from_terms(vec!["cathedral", "category", "catering"]);
let mut wb  = WallBreakerWfst::new(&scdawg, "cathedrel", 2);   // runs the whole query eagerly
let s0      = wb.start();
let edges   = wb.transitions_lazy(s0);                         // drive via LazyWfst, then read
```

WallBreaker also requires an **SCDAWG** (`SubstringDictionary` + `BidirectionalDictionaryNode`), unlike
the other matchers — see the [backend × variant matrix](02-choosing-a-variant.md#dictionary-backend--variant-compatibility).

---

## See also

- [theory/04 · Composition](../theory/04-composition.md) — the math: the `` $`\varepsilon`$ ``-filtered
  lazy product, the matched-tape rule, and why a fuzzy matcher must *be* a WFST to participate.
- [design/phonetic-pipeline-builder](../design/phonetic-pipeline-builder.md) — a builder that *emits*
  pipeline stages (`build_rewrite_wfst` / `build_phonetic_nfa` / `build`).
- [guides/02 · Choosing a variant](02-choosing-a-variant.md) — which variant to use as each stage.
- [guides/05 · Performance and tuning](05-performance-and-tuning.md) — keeping lazy composition cheap
  (cache policy over the product state space).
