# duallity

**Levenshtein automata as [lling-llang](https://github.com/vinary-tree/lling-llang) WFSTs** — the bridge that lets a fuzzy-match automaton be *composed* with phonetic rewrites, language models, and any other weighted transducer.

![License](https://img.shields.io/badge/license-Apache--2.0-blue)
[![crates.io](https://img.shields.io/crates/v/duallity.svg)](https://crates.io/crates/duallity)
[![docs.rs](https://img.shields.io/docsrs/duallity)](https://docs.rs/duallity)
![MSRV](https://img.shields.io/badge/rustc-1.70%2B-orange)

---

## What is duallity?

`duallity` exposes [liblevenshtein](https://github.com/vinary-tree/liblevenshtein-rust)'s **Levenshtein automata** as [lling-llang](https://github.com/vinary-tree/lling-llang) **Weighted Finite-State Transducers (WFSTs)**. Once a fuzzy matcher *is* a WFST, it stops being a closed black box that only emits "all terms within edit distance `` $`k`$ ``" — it becomes an algebraic object you can **compose** with other transducers:

```text
   query + dictionary  ──►  Levenshtein WFST  ──∘──  phonetic / language-model WFST  ──►  best paths
        (liblevenshtein)        (duallity)         (lling-llang / your own)            (shortest-path)
```

It is the **dual** of two existing crates, and the connective tissue between them:

- **[liblevenshtein](https://github.com/vinary-tree/liblevenshtein-rust)** supplies the *query* half — a Levenshtein-automaton transducer that walks a dictionary to find all terms within an edit distance.
- **[libdictenstein](https://github.com/vinary-tree/libdictenstein)** supplies the *container* half — the tries, DAWGs, and SCDAWGs that automaton traverses.
- **[lling-llang](https://github.com/vinary-tree/lling-llang)** supplies the *algebra* — the `Wfst` trait, the semirings, and `compose`.

`duallity` wraps the first to satisfy the third's trait, so the product `(dictionary × automaton)` slots directly into a composition pipeline. It is **one crate** that depends on **all three** siblings — the only place where they meet.

> **Why a separate crate?** These adapters used to live behind liblevenshtein's `wfst` feature. But lling-llang already depended on liblevenshtein, so a `wfst` feature pulling lling-llang back in created a **dependency cycle**. Extracting the adapters into `duallity` — which sits *above* both — breaks the cycle and keeps the graph acyclic. See [Relationship & migration](#relationship--migration).

---

## The idea in one diagram

<img src="docs/diagrams/composition-pipeline.svg" alt="A query and dictionary become a lazy Levenshtein WFST, composed with a downstream transducer; a shortest-path search over the composed lattice returns the best corrections" width="820"/>

A query and a dictionary become a lazy Levenshtein WFST. `compose` lazily intersects it with a downstream transducer (phonetic rewrites, an n-gram LM, …). A shortest-path search over the composed lattice returns the best corrections, ranked by total weight.

---

## Documentation

This README is the overview. The full documentation lives in **[`docs/`](docs/README.md)** — a
guideline-driven corpus with reading orders for newcomers, implementers, and researchers:

- **[Theory](docs/theory/README.md)** — semirings, edit distance, Levenshtein automata, composition, universal automata, WallBreaker, regular-language limits (with complete correctness proofs).
- **[Architecture](docs/architecture/README.md)** — the crate family, the WFST trait surface, state encoding, lazy evaluation, registries.
- **[Design](docs/design/README.md)** — one page per WFST variant, with exact semantics and honest limitations.
- **[Guides](docs/guides/README.md)** — quickstart, choosing a variant, composing pipelines, phonetic matching, performance.
- **[Engineering](docs/engineering/README.md)** · **[Security](docs/security/README.md)** · **[References](docs/references/bibliography.md)** · **[Glossary](docs/references/glossary.md)** · **[Diagrams](docs/diagrams/README.md)**.

---

## What is a WFST, and what is composition?

A **Weighted Finite-State Transducer** is a finite automaton whose transitions carry an *input* label, an *output* label, and a *weight* drawn from a **semiring** `` $`(\mathbb{K}, \oplus, \otimes, \bar{0}, \bar{1})`$ ``:

- `` $`\oplus`$ `` (*plus*) combines **alternative** paths to the same place — it is how you sum over ways to do something.
- `` $`\otimes`$ `` (*times*) combines the weights **along** a single path — it is how you accumulate a path's cost.
- `` $`\bar{0}`$ `` is the identity for `` $`\oplus`$ `` (an unreachable / forbidden path); `` $`\bar{1}`$ `` is the identity for `` $`\otimes`$ `` (a free step).

A path's weight is the `` $`\otimes`$ ``-product of its transition weights; the weight a transducer assigns to an input/output string pair is the `` $`\oplus`$ ``-sum over all paths carrying that pair.

`duallity` works in the **tropical semiring** — lling-llang's `TropicalWeight`:

```math
\mathbb{T} \;=\; \bigl(\mathbb{R} \cup \{+\infty\},\; \min,\; +,\; +\infty,\; 0\bigr),
\qquad a \oplus b = \min(a, b), \qquad a \otimes b = a + b .
```

<img src="docs/diagrams/tropical-semiring-algebra.svg" alt="The tropical (min, +) semiring: plus is min (best alternative wins), times is plus (costs add along a path), zero is +infinity, one is 0" width="760"/>

Under `` $`(\min, +)`$ ``, "`` $`\oplus`$ ``-sum over all paths" becomes "**minimum-cost path**", and "`` $`\otimes`$ ``-product along a path" becomes "**sum of edge costs**". So a shortest path *is* the best answer.

> ⚠️ **Naming gotcha.** `TropicalWeight::zero()` is the value `` $`+\infty`$ `` (the additive identity `` $`\bar{0}`$ ``, "no path") and `TropicalWeight::one()` is the value `` $`0`$ `` (the multiplicative identity `` $`\bar{1}`$ ``, "a free step"). The method names follow the *algebraic* role, not the numeric value.

### The Levenshtein automaton as a WFST

For a query `` $`q`$ `` and bound `` $`k`$ ``, the Levenshtein automaton accepts exactly the strings within edit distance `` $`k`$ `` of `` $`q`$ ``. As a tropical WFST it is more than a yes/no acceptor — **its minimum path weight from start to an accepting state for a dictionary term `` $`w`$ `` is precisely `` $`d_{\mathrm{lev}}(q, w)`$ ``** (capped at `` $`k`$ ``). Each transition is one edit operation (input = query side, output = dictionary side):

| Operation | Condition | Label / weight |
|---|---|---|
| **match** | `` $`q[i] = c`$ `` | `` $`q[i] : c \,/\, 0`$ `` (a free step, `` $`\bar{1}`$ ``) |
| **substitute** | `` $`q[i] \neq c`$ `` | `` $`q[i] : c \,/\, 1`$ `` |
| **insert** | (extra dictionary char) | `` $`\varepsilon : c \,/\, 1`$ `` |
| **delete** | (missing dictionary char) | `` $`q[i] : \varepsilon \,/\, 1`$ `` |

`duallity` builds this **lazily**: the product state space (dictionary node × automaton state) is never materialized in full. A composite `StateId` packs the pair —

```math
\mathrm{StateId} \;=\; d \cdot M + a
```

— where `` $`d`$ `` is the dictionary-node id, `` $`a`$ `` the automaton-state id, and `` $`M`$ `` the per-engine radix; states are expanded and cached on first touch. The dictionary `` $`D`$ `` is generic over any [libdictenstein](https://github.com/vinary-tree/libdictenstein) backend whose edge unit converts to `char`, so byte and Unicode dictionaries both work; edit distance is computed **per Unicode scalar**, not per byte.

### Composition `` $`T_1 \circ T_2`$ ``

Composition chains two transducers: `` $`(T_1 \circ T_2)`$ `` reads what `` $`T_1`$ `` reads, writes what `` $`T_2`$ `` writes, and matches `` $`T_1`$ ``'s **output** tape against `` $`T_2`$ ``'s **input** tape. In the tropical semiring its weight is

```math
(T_1 \circ T_2)(x, z) \;=\; \min_{y}\,\bigl[\, T_1(x, y) + T_2(y, z) \,\bigr] .
```

`lling_llang::composition::compose(t1, t2)` returns a **lazy** composition — product states are computed only as a shortest-path search visits them, so the pipeline never pays for the full Cartesian state space. This is the whole point of `duallity`: make the Levenshtein matcher a `` $`T_1`$ `` you can feed into that `` $`\min`$ ``-over-`` $`y`$ `` fold. A plain result *set* discards the `` $`T_1(x, y)`$ `` weights and cannot enter that fold; only the WFST keeps the weighted relation.

---

## The WFST variants

Every wrapper below implements lling-llang's `Wfst<char, TropicalWeight>` (and `LazyWfst`), so every one is composable. Pick by what you are matching (a task-oriented decision guide is in [guides/02 · Choosing a variant](docs/guides/02-choosing-a-variant.md)).

| Variant | Type(s) | What it's for |
|---|---|---|
| **Levenshtein** | `LevenshteinWfst<D>` | The core adapter: a query-parameterized Levenshtein automaton × dictionary, as a lazy tropical WFST. Start here. |
| **Universal** | `UniversalLevenshteinWfst<V, D>`, `BoundUniversalWfst<V, D>` | liblevenshtein's *universal* (query-agnostic) automaton — built once per `max_distance` and reused across queries. `V` is the position variant (`Standard` / `Transposition` / `MergeAndSplit`). |
| **WallBreaker** | `WallBreakerWfst<'a, D>`, `WallBreakerWfstBuilder` | Defeats the "wall effect" at large `` $`k`$ ``: splits the query (pigeonhole), finds exact substring seeds via an SCDAWG, and extends bidirectionally. Needs a `SubstringDictionary`. |
| **Generalized** | `GeneralizedWfst<D>`, `GeneralizedWfstBuilder` | A runtime-configurable automaton: mix standard, transposition, merge/split, and phonetic-digraph (`ph↔f`, `ck↔k`) operations via an `OperationSet`. |
| **Phonetic** | `PhoneticWfst<D>` / `…Builder`, `PhoneticNfaWfst`, `RewriteWfst`, `PhoneticPipelineBuilder` | Sound-alike matching. `RewriteWfst` applies rule-based rewrites (`ph→f`); `PhoneticNfaWfst` / `PhoneticWfst` compile a phonetic regex (`(ph|f)one`) into an NFA-backed transducer (feature `phonetic-rules`). |

Supporting types are public too: `DictionaryBackend` (adapts a dictionary to lling-llang's `LatticeBackend`), `LevenshteinStateSource` / `UniversalLevenshteinStateSource` (the lazy `StateSource` engines), `InvalidWeightError`, and the `state_encoding` module (`try_encode` / `decode` / `estimate_automaton_states`).

---

## Quick start

```toml
[dependencies]
duallity = "0.3"
liblevenshtein = "0.9"
lling-llang = "0.2"
libdictenstein = "0.2"
```

**Wrap a Levenshtein automaton, then compose it.** Examples are marked `ignore` because they pull in the sibling crates; the constructor signatures are exact.

```rust,ignore
use duallity::LevenshteinWfst;
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use lling_llang::composition::compose;

// 1. A dictionary (any libdictenstein backend with `char` units works).
let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help", "world"]);

// 2. The Levenshtein automaton for query "helo", up to edit distance 2, as a WFST.
//    LevenshteinWfst::new(&dictionary, query, max_distance)
let lev = LevenshteinWfst::new(&dict, "helo", 2);

// 3. Compose with any downstream WFST `language_model` — a lazy product.
//    compose(t1, t2) matches t1's output tape against t2's input tape.
let mut composed = compose(lev, language_model);

// 4. Walk best paths; a ComposedPath exposes `inputs`, `outputs`, and `weight`.
//    The tropical weight is edit distance + downstream cost.
for path in composed.accepting_paths() {
    println!("{:?} -> {:?}  (weight {})", path.inputs, path.outputs, path.weight.value());
}
```

**Pick the algorithm variant** (transpositions = Damerau–Levenshtein):

```rust,ignore
use duallity::LevenshteinWfst;
use liblevenshtein::transducer::Algorithm;
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;

let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
// Adjacent-swap ("tset" → "test") costs 1, not 2.
let lev = LevenshteinWfst::with_algorithm(&dict, "tset", 2, Algorithm::Transposition);
```

**Compose with a rule-based phonetic rewriter** (no extra feature needed — `RewriteWfst` is always available):

```rust,ignore
use duallity::{LevenshteinWfst, RewriteWfst, CommonPhoneticRules};
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use lling_llang::composition::compose;

let dict = DynamicDawgChar::<()>::from_terms(vec!["phone", "graph", "telephone"]);

// "fone" → rewrite (f↔ph, cost 0.1) → Levenshtein(2) over the dictionary.
let rewrite = RewriteWfst::with_rules(CommonPhoneticRules::english())
    .expect("valid preset rules");
let lev     = LevenshteinWfst::new(&dict, "fone", 2);
let phonetic_fuzzy = compose(rewrite, lev);
```

**Compile a phonetic regex into a transducer** — requires `features = ["phonetic-rules"]`:

```rust,ignore
// Cargo.toml:  duallity = { version = "0.3", features = ["phonetic-rules"] }
use duallity::PhoneticWfstBuilder;
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;

let dict = DynamicDawgChar::<()>::from_terms(vec!["phone", "fone", "bone"]);

// "(ph|f)one" compiles to an NFA-backed phonetic WFST over the dictionary.
let wfst = PhoneticWfstBuilder::new(dict, 2)
    .phonetic_weight(0.1)
    .expect("valid phonetic weight")
    .build_from_pattern("(ph|f)one")
    .expect("valid phonetic pattern");
```

---

## Feature flags

| Feature | Enables | Pulls in |
|---|---|---|
| *(default)* | `LevenshteinWfst`, `UniversalLevenshteinWfst`, `WallBreakerWfst`, `GeneralizedWfst`, `RewriteWfst`, `DictionaryBackend`, the pipeline builder | — |
| `phonetic-rules` | NFA-backed phonetic variants: `PhoneticWfst` / `PhoneticWfstBuilder`, `PhoneticNfaWfst`, `PhoneticStateSource`, and `PhoneticPipelineBuilder::build` / `build_phonetic_nfa` | `liblevenshtein/phonetic-rules` |

```toml
duallity = { version = "0.3", features = ["phonetic-rules"] }
```

---

## Performance & safety at a glance

- **Lazy + cached.** Product states are computed on first touch and memoized in a per-WFST `LazyStateCache`; a `CachePolicy` (`CacheAll` / `Lru { max_states }` / `NoCache`) bounds memory. See [architecture/04](docs/architecture/04-lazy-evaluation-and-caching.md) and [guides/05](docs/guides/05-performance-and-tuning.md).
- **Zero `unsafe`, panic-free public surface.** Fallible operations return `Result`/`Option` (`try_encode`, `decode`, `try_intern`, weight setters → `InvalidWeightError`); `.expect`/`.unwrap` live only under `#[cfg(test)]`. See [engineering/safety-and-panics](docs/engineering/safety-and-panics.md).
- **`Clone + Send + Sync`.** Registries are `Arc<RwLock<…>>` with poison recovery; clones are cheap. See [engineering/concurrency-and-locking](docs/engineering/concurrency-and-locking.md).
- **Pure compute.** No I/O, no network, no deserialization; the one real risk is resource exhaustion, bounded by `max_distance` and the cache policy. See [security/threat-model](docs/security/threat-model.md).

---

## Relationship & migration

<img src="docs/diagrams/crate-dependency-graph.svg" alt="duallity sits at the top and depends on liblevenshtein, lling-llang, and libdictenstein; liblevenshtein uses libdictenstein dictionaries; lling-llang provides the WFST algebra" width="620"/>

`duallity` sits at the top and depends on **liblevenshtein 0.9**, **lling-llang 0.2**, and **libdictenstein 0.2** — the only place where all three meet, which is exactly why it can break the former liblevenshtein ⇄ lling-llang cycle.

**Migrating from liblevenshtein's old `wfst` module?** The types are unchanged; only the path moved:

```rust,ignore
// Old (liblevenshtein ≤ 0.8, behind the removed `wfst` feature)
use liblevenshtein::wfst::LevenshteinWfst;
use liblevenshtein::wfst::DictionaryBackend;

// New
use duallity::LevenshteinWfst;
use duallity::DictionaryBackend;
```

The dictionary types themselves also moved out of liblevenshtein into **libdictenstein** — see that crate's migration note (`use liblevenshtein::dictionary::X` → `use libdictenstein::X`).

---

## References

1. Mohri, M. (1997). *Finite-State Transducers in Language and Speech Processing.* Computational Linguistics 23(2), 269–311. [ACL Anthology J97-2003](https://aclanthology.org/J97-2003/)
2. Schulz, K. U., & Mihov, S. (2002). *Fast String Correction with Levenshtein Automata.* International Journal on Document Analysis and Recognition (IJDAR) 5(1), 67–85. [doi:10.1007/s10032-002-0082-8](https://doi.org/10.1007/s10032-002-0082-8) — the automaton `duallity` wraps.
3. Mohri, M., Pereira, F., & Riley, M. (2002). *Weighted Finite-State Transducers in Speech Recognition.* Computer Speech & Language 16(1), 69–88. [doi:10.1006/csla.2001.0184](https://doi.org/10.1006/csla.2001.0184) — composition and the tropical semiring.

The full, DOI-linked bibliography is in [`docs/references/bibliography.md`](docs/references/bibliography.md).

---

## License

Licensed under the [Apache License, Version 2.0](LICENSE). Minimum supported Rust version: **1.70**.

Part of the [vinary-tree](https://github.com/vinary-tree) family:
[`libdictenstein`](https://github.com/vinary-tree/libdictenstein) ·
[`liblevenshtein`](https://github.com/vinary-tree/liblevenshtein-rust) ·
[`lling-llang`](https://github.com/vinary-tree/lling-llang) ·
[`llattice`](https://github.com/vinary-tree/llattice)
