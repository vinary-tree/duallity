# 03 · Composing pipelines

Composition is what makes duallity more than a fuzzy matcher: chain a phonetic rewriter, the
Levenshtein matcher, and a language model into one weighted transducer, then take the shortest path
([theory/04](../theory/04-composition.md)).

<img src="../diagrams/composition-pipeline.svg" alt="Query and dictionary become a Levenshtein WFST, composed with a downstream transducer, searched by shortest path" width="820"/>

## 1. `compose`

`lling_llang::composition::compose(t1, t2)` returns a **lazy** composition that matches `t1`'s output
tape against `t2`'s input tape. In the tropical semiring its weight is `min over y [ t1(x,y) + t2(y,z) ]`.

```rust,ignore
use duallity::{LevenshteinWfst, RewriteWfst, CommonPhoneticRules};
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use lling_llang::composition::compose;
use lling_llang::prelude::*;

let dict    = DynamicDawgChar::<()>::from_terms(vec!["phone", "telephone", "graph"]);
let rewrite = RewriteWfst::with_rules(CommonPhoneticRules::english())   // f ↔ ph, …
    .expect("valid preset rules");
let lev     = LevenshteinWfst::new(&dict, "fone", 2);

// rewrite ∘ levenshtein : "fone" → phonetic normalize → fuzzy match the dictionary.
let composed = compose(rewrite, lev);
for path in composed.accepting_paths() {
    println!("{:?}  weight {:?}", path.labels(), path.weight());
}
```

Composition is **associative**, so a three-stage pipeline is just nested `compose`:

```rust,ignore
// rewrite ∘ levenshtein ∘ language_model
let pipeline = compose(compose(rewrite, lev), language_model);
```

The weight of a complete accepting path is the sum of the per-stage tropical costs: phonetic-rewrite
cost + edit distance + language-model score.

## 2. `LazyWfstWrapper` — driving a `StateSource`

A `StateSource` ([architecture/02](../architecture/02-wfst-trait-surface.md#3-statesource--the-computation-kernel))
is the immutable computation kernel. Wrap it in a `LazyWfstWrapper` to get a `Wfst`/`LazyWfst` you can
compose or expand:

```rust,ignore
use duallity::LevenshteinStateSource;
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use lling_llang::prelude::*;

let dict   = DynamicDawgChar::<()>::from_terms(vec!["hello", "help"]);
let source = LevenshteinStateSource::new(&dict, "helo", 2);
let mut wfst = LazyWfstWrapper::new(source);     // now a Wfst<char, TropicalWeight>
wfst.transitions_lazy(wfst.start());
```

> **Note:** the `StateSource` (immutable) path is the one `compose` uses internally.
> `GeneralizedWfst` supports it through interior state registries. `WallBreakerWfst` is the remaining
> mutable-only variant: its `compute_state` returns `Pending`, so expand it through `LazyWfst` first
> ([architecture/04 §4](../architecture/04-lazy-evaluation-and-caching.md#4-the-immutable--mutable-split)).

## 3. `DictionaryBackend` — the vocabulary layer

When a downstream stage needs lling-llang's lattice vocabulary (terms ↔ `VocabId`), adapt a
dictionary with [`DictionaryBackend`](../design/levenshtein-wfst.md#7-dictionarybackend):

```rust,ignore
use duallity::DictionaryBackend;
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use lling_llang::prelude::*;

let dict        = DynamicDawgChar::<()>::from_terms(vec!["hello", "world"]);
let mut backend = DictionaryBackend::new(dict);
let id = backend.intern("hello");                // stable VocabId
assert_eq!(backend.lookup(id), Some("hello"));
```

## 4. Shortest-path search

A composed transducer's accepting paths, ranked by tropical weight, *are* the best corrections: the
minimum-weight path is the best answer ([theory/01 §3](../theory/01-semirings-and-wfsts.md#3-the-tropical-min--semiring)).
Iterate `accepting_paths()` (lowest weight first) and read `path.labels()` for the corrected string and
`path.weight()` for the total cost.

## See also

- [theory/04 · Composition](../theory/04-composition.md) (the math)
- [design/phonetic-pipeline-builder](../design/phonetic-pipeline-builder.md) (a builder that emits stages)
- [guides/05 · Performance and tuning](05-performance-and-tuning.md) (keeping lazy composition cheap)
