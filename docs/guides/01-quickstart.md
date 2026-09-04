# 01 · Quickstart

Build a Levenshtein WFST, compose it with a downstream transducer, and walk the best paths. This is
the shortest path from "I have a dictionary and a misspelled query" to "ranked corrections", and it is
the whole reason duallity exists: it makes liblevenshtein's fuzzy matcher a *first-class weighted
transducer* you can fold into an lling-llang pipeline ([theory/04](../theory/04-composition.md)).

<img src="../diagrams/composition-pipeline.svg" alt="A query and a dictionary become a Levenshtein WFST, composed with a downstream transducer, and searched by shortest path" width="820"/>

The diagram traces the whole story below: the query $`q`$ (orange, input tape) and the dictionary
$`D`$ (green) become a `LevenshteinWfst` (blue); `compose` (yellow) folds it against a downstream
transducer; and a shortest-path search reads out ranked corrections (purple).

---

## The one end-to-end story

Everything in this section is one continuous program. Read it top to bottom; the expected output is at
the end.

### 1. A dictionary, a query, and a matcher

Any [libdictenstein](../architecture/01-crate-family-and-dependency-graph.md) backend whose edge unit
converts to `char` works as the dictionary. `DynamicDawgChar` is a good general-purpose Unicode choice
(updatable at runtime; see [02 · Choosing a variant](02-choosing-a-variant.md#dictionary-backend--variant-compatibility) for
the alternatives).

```rust,ignore
use duallity::LevenshteinWfst;
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;

// A dictionary of correct terms.
let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help", "world"]);

// The Levenshtein automaton for the misspelling "helo", up to edit distance k = 2, as a WFST.
let lev = LevenshteinWfst::new(&dict, "helo", 2);
```

`lev` *is* a `Wfst<char, TropicalWeight>`. Its accepting paths are exactly the dictionary terms within
edit distance $`k = 2`$ of `"helo"`, each weighted by its edit distance
([theory/02](../theory/02-edit-distance-and-levenshtein-automata.md)). For this dictionary that set is:

| Dictionary term | Edit from `"helo"` | Distance |
|---|---|---|
| `hello` | insert one `l` ($`\texttt{helo} \to \texttt{hello}`$) | $`1`$ |
| `help` | substitute $`\texttt{o} \to \texttt{p}`$ ($`\texttt{helo} \to \texttt{help}`$) | $`1`$ |
| `world` | four edits — outside the $`k = 2`$ band | excluded |

### 2. Compose with a downstream transducer

Because `lev` is a WFST, you can `compose` it with any other `Wfst<char, TropicalWeight>` — a language
model, a phonetic rescorer, your own transducer. Composition matches `lev`'s **output** tape (the
dictionary side, teal) against the downstream stage's **input** tape, and sums the weights in the
tropical semiring ([theory/04](../theory/04-composition.md)).

Suppose `language_model` is a user-supplied `Wfst<char, TropicalWeight>` that relabels each term to
itself and adds a unigram cost — say `hello` costs $`0.5`$ and `help` costs $`2.0`$
(`help` is the rarer word here). Then:

```rust,ignore
use lling_llang::composition::compose;
use lling_llang::prelude::*;

// lev ∘ language_model : "helo" → fuzzy-match the dictionary → rescore each candidate term.
// `accepting_paths` needs &mut self, so bind the composition as `mut`.
let mut composed = compose(lev, language_model);

// Walk best paths (lowest tropical weight first). A ComposedPath exposes three PUBLIC FIELDS:
//   path.inputs : Vec<char>  — the query-side tape (what lev consumed)
//   path.outputs: Vec<char>  — the dictionary/output-side tape (the corrected term)
//   path.weight : TropicalWeight — the total tropical cost; read the f64 with .value()
for path in composed.accepting_paths() {
    println!(
        "{:?} -> {:?}  (weight {})",
        path.inputs,
        path.outputs,
        path.weight.value()
    );
}
```

### 3. Expected output

The composed weight of each path is `edit distance + language-model cost`, and `accepting_paths()`
yields paths **best-first** (lowest tropical weight first — the minimum-weight path is the best
correction; see [theory/01 §3](../theory/01-semirings-and-wfsts.md)):

```text
['h', 'e', 'l', 'o'] -> ['h', 'e', 'l', 'l', 'o']  (weight 1.5)
['h', 'e', 'l', 'o'] -> ['h', 'e', 'l', 'p']       (weight 3.0)
```

- `hello`: edit distance $`1`$ $`+`$ language-model cost $`0.5`$ $`= 1.5`$ — the
  top correction.
- `help`: edit distance $`1`$ $`+`$ language-model cost $`2.0`$ $`= 3.0`$.

Swap in a different downstream stage and only the second summand changes; the edit distances are fixed
by the Levenshtein automaton. That separation — a fuzzy geometry (edits) plus a domain score
(the downstream) combined by one associative $`+`$ — is the point of modelling the matcher as a
transducer.

---

## What the weights mean

> **The weight is a tropical $`(\min, +)`$ cost — lower is better.** duallity works in the
> **tropical semiring** $`\mathbb{T} = (\mathbb{R} \cup \{+\infty\},\ \min,\ +,\ +\infty,\ 0)`$
> ([theory/01](../theory/01-semirings-and-wfsts.md)). Weights *add* along a path ($`\otimes = +`$)
> and *minimize* across alternative paths ($`\oplus = \min`$), so the weight of a correction is
> the sum of its per-stage costs and the best correction is the one of least total weight.
>
> **Mind the naming gotcha.** In lling-llang, `TropicalWeight::zero()` is the value **$`+\infty`$**
> (the additive identity $`\bar{0}`$, meaning "no path / forbidden") and `TropicalWeight::one()`
> is the value **$`0`$** (the multiplicative identity $`\bar{1}`$, "a free step"). The method
> names follow the *algebraic* role, not the numeric value
> ([master notation](../theory/README.md#semirings-and-weights)). Read the numeric cost with
> `weight.value()`.

---

## Common mistakes

> ⚠️ **Do not read a lazy state before it is expanded.** Every duallity variant is a **lazy** WFST:
> a state's transitions and final status are computed on *first mutable touch* and then cached
> ([architecture/04](../architecture/04-lazy-evaluation-and-caching.md)). The **immutable** `Wfst`
> accessors — `transitions(&self, s)`, `is_final(&self, s)`, `final_weight(&self, s)` — only *read the
> cache*: for a state that has never been expanded they return an **empty slice**, **`false`**, and the
> $`\bar{0} = +\infty`$ weight, respectively (`empty_char_transitions()` in the wrappers; the
> lling-llang source even comments that immutable access "requires mutable access in practice"). Drive
> expansion first through the **mutable** `LazyWfst` surface:

```rust,ignore
use duallity::LevenshteinWfst;
use lling_llang::prelude::*;                       // Wfst, LazyWfst, StateId, …
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;

let dict     = DynamicDawgChar::<()>::from_terms(vec!["hello", "help"]);
let mut lev  = LevenshteinWfst::new(&dict, "helo", 2);
let start    = lev.start();

// ✗ WRONG: eager read before expansion — the state is not computed yet, so this is empty.
let empty = lev.transitions(start);                // &[]  (nothing has been computed)
assert!(empty.is_empty());

// ✓ RIGHT: the lazy path computes-on-touch, caches, and returns the real transitions.
let edges = lev.transitions_lazy(start);           // &mut self; computes and caches `start`
assert!(!edges.is_empty());
// After transitions_lazy/expand, the immutable accessors see the cached state:
assert!(lev.is_expanded(start));
```

`compose` and `accepting_paths` handle this driving for you — the mistake bites when you traverse a WFST
by hand (as a breadth-first walk does; see `benches/wfst_expansion.rs`, which expands every reachable
state with `transitions_lazy`).

---

## The variant signatures at a glance

Four snippets that pin the constructor signatures you will reach for most. Note the `max_distance`
type per variant — it is **`usize`** for `LevenshteinWfst` and **`u8`** for the universal / generalized
/ phonetic variants (the [design pages](../design/README.md) tabulate all eight).

### Levenshtein — the default matcher

```rust,ignore
use duallity::LevenshteinWfst;
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;

let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help", "world"]);
let lev  = LevenshteinWfst::new(&dict, "helo", 2);   // max_distance: usize

assert_eq!(lev.query(), "helo");                     // query() -> &str (borrowed, stable)
assert_eq!(lev.max_distance(), 2usize);              // max_distance() -> usize
```

### Damerau–Levenshtein and OCR arities — `with_algorithm`

`with_algorithm` selects the metric. Adjacent transposition (`Algorithm::Transposition`, for swaps like
$`\texttt{tset} \to \texttt{test}`$) and fixed OCR merge/split edits (`Algorithm::MergeAndSplit`) are available directly.

```rust,ignore
use duallity::LevenshteinWfst;
use liblevenshtein::transducer::Algorithm;
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;

let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
let _lev = LevenshteinWfst::with_algorithm(&dict, "tset", 2, Algorithm::Transposition);
```

### Rule-based phonetics — `RewriteWfst` (no feature needed)

`RewriteWfst` applies literal rules like $`\texttt{ph} \to \texttt{f}`$ and composes in front of a Levenshtein matcher. Its
constructors return `Result<_, InvalidWeightError>` because rule costs are validated to be finite and
non-negative.

```rust,ignore
use duallity::{RewriteWfst, CommonPhoneticRules};

let rewrite = RewriteWfst::with_rules(CommonPhoneticRules::english())
    .expect("preset rules carry valid costs");        // -> Result<RewriteWfst, InvalidWeightError>
assert_eq!(rewrite.num_rules(), 7);                   // the English preset has 7 rules
```

### Regex phonetics — `PhoneticWfstBuilder` (needs `phonetic-rules`)

The regex → NFA path is feature-gated. The builder **takes the dictionary by value** and its
weight setters return `Result<_, InvalidWeightError>`; `max_distance` here is a `u8`.

```rust,ignore
// Cargo.toml:  duallity = { version = "=4.0.0-rc.6", features = ["phonetic-rules"] }
use duallity::PhoneticWfstBuilder;
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;

let dict  = DynamicDawgChar::<()>::from_terms(vec!["phone", "fone", "bone"]);
let _wfst = PhoneticWfstBuilder::new(dict, 2)         // new(dictionary: D, max_distance: u8)
    .phonetic_weight(0.1)                             // -> Result<Self, InvalidWeightError>
    .expect("finite, non-negative weight")
    .build_from_pattern("(ph|f)one")                  // -> Result<PhoneticWfst<D>, String>
    .expect("valid phonetic pattern");
```

---

## Next steps

- Not sure which variant? → [02 · Choosing a variant](02-choosing-a-variant.md).
- Building a multi-stage pipeline (per-stage weight math, `LazyWfstWrapper`, `DictionaryBackend`)? →
  [03 · Composing pipelines](03-composing-pipelines.md).
- Sound-alike matching? → [04 · Phonetic matching](04-phonetic-matching.md).
- Tuning memory/latency? → [05 · Performance and tuning](05-performance-and-tuning.md).
