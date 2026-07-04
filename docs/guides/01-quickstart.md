# 01 · Quickstart

Build a Levenshtein WFST, compose it with a downstream transducer, and walk the best paths. This is
the shortest path from "I have a dictionary and a misspelled query" to "ranked corrections".

## 1. A dictionary and a query

Any [libdictenstein](../architecture/01-crate-family-and-dependency-graph.md) backend whose edge unit
converts to `char` works. `DynamicDawgChar` is a good general-purpose Unicode choice.

```rust,ignore
use duallity::LevenshteinWfst;
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;

// 1. A dictionary.
let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help", "world"]);

// 2. The Levenshtein automaton for "helo", up to edit distance 2, as a WFST.
let lev = LevenshteinWfst::new(&dict, "helo", 2);
```

`lev` *is* a `Wfst<char, TropicalWeight>`: its accepting paths are the dictionary terms within edit
distance 2 of `"helo"`, each weighted by its edit distance ([theory/02](../theory/02-edit-distance-and-levenshtein-automata.md)).

## 2. Compose with a downstream transducer

Because `lev` is a WFST, you can `compose` it with any other `Wfst<char, TropicalWeight>` — a phonetic
rewriter, an n-gram language model, your own transducer. Composition matches `lev`'s **output** tape
(dictionary side) against the downstream **input** tape ([theory/04](../theory/04-composition.md)).

```rust,ignore
use lling_llang::composition::compose;
use lling_llang::prelude::*;

// `language_model` is any Wfst<char, TropicalWeight> you supply.
let composed = compose(lev, language_model);

// 3. Walk best paths; tropical weight = edit distance + downstream cost.
for path in composed.accepting_paths() {
    println!("{:?}  (weight {:?})", path.labels(), path.weight());
}
```

## 3. Pick the algorithm variant

`with_algorithm` selects the metric. Adjacent transposition and fixed merge/split edits are available
directly through `LevenshteinWfst`; use the [universal](../design/universal-wfst.md) path for many
queries over the same automaton and the [generalized](../design/generalized-wfst.md) path when you
need a runtime-composed operation set.

```rust,ignore
use duallity::LevenshteinWfst;
use liblevenshtein::transducer::Algorithm;
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;

let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
let _lev = LevenshteinWfst::with_algorithm(&dict, "tset", 2, Algorithm::Transposition);
```

## 4. Phonetic rewriting (no feature needed)

`RewriteWfst` applies rules like `ph→f` and composes in front of a Levenshtein WFST:

```rust,ignore
use duallity::{LevenshteinWfst, RewriteWfst, CommonPhoneticRules};
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use lling_llang::composition::compose;

let dict    = DynamicDawgChar::<()>::from_terms(vec!["phone", "graph", "telephone"]);
let rewrite = RewriteWfst::with_rules(CommonPhoneticRules::english())
    .expect("valid preset rules");
let lev     = LevenshteinWfst::new(&dict, "fone", 2);
let _phonetic_fuzzy = compose(rewrite, lev);     // "fone" → (f↔ph) → fuzzy match
```

## 5. Phonetic regex (needs `phonetic-rules`)

```rust,ignore
// Cargo.toml:  duallity = { version = "0.3", features = ["phonetic-rules"] }
use duallity::PhoneticWfstBuilder;
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;

let dict = DynamicDawgChar::<()>::from_terms(vec!["phone", "fone", "bone"]);
let _wfst = PhoneticWfstBuilder::new(dict, 2)
    .phonetic_weight(0.1)
    .expect("valid phonetic weight")
    .build_from_pattern("(ph|f)one")
    .expect("valid phonetic pattern");
```

## Next steps

- Not sure which variant? → [02 · Choosing a variant](02-choosing-a-variant.md).
- Building a multi-stage pipeline? → [03 · Composing pipelines](03-composing-pipelines.md).
- Tuning memory/latency? → [05 · Performance and tuning](05-performance-and-tuning.md).
