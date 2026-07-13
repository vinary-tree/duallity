# Guides

Task-oriented documentation for **duallity 0.3**: install the crate, pick a WFST variant, compose a
multi-stage pipeline, do phonetic matching, and tune performance. These guides are the *how-to* layer.
They build on the concepts in [theory](../theory/) and the exact per-variant API in [design](../design/),
but you can follow them front to back without reading those first — every guide cross-links back to the
theory and design pages when you want the underlying *why*.

Each guide is self-contained and grounds every code example in the crate's real 0.3.0 surface
(`src/lib.rs` and the per-variant modules). Code blocks are marked `rust,ignore`: they are
*illustrative and byte-accurate to the public API*, not doctests, so you can read them for signatures
and intent without a compile step. Mathematics is written in GitHub-flavored MathJax — inline as a
backtick-wrapped dollar span (e.g. `` $`k \le 255`$ ``) and display as a fenced ` ```math ` block.

---

## Which guide for which goal

Start from what you are trying to do; the strip below routes you to the one guide that answers it.

| If your goal is… | …read | It answers |
|---|---|---|
| "I have a dictionary and a misspelled query — get me ranked corrections, now." | [01 · Quickstart](01-quickstart.md) | the shortest end-to-end path: dictionary → `LevenshteinWfst::new` → `compose` → `accepting_paths`, with expected output. |
| "Which of the eight variants do I actually want?" | [02 · Choosing a variant](02-choosing-a-variant.md) | a decision tree, `` $`k`$ ``-thresholds, Big-O per variant, and a *backend × variant* compatibility matrix. |
| "I need to chain a rewriter, a matcher, and a language model into one scorer." | [03 · Composing pipelines](03-composing-pipelines.md) | `compose`, `LazyWfstWrapper`, `DictionaryBackend`, and shortest-path search, with per-stage weight math. |
| "I want sound-alike (phonetic) matching." | [04 · Phonetic matching](04-phonetic-matching.md) | rule-based `RewriteWfst` vs. regex-based `PhoneticWfst`; the English / German / French rule sets. |
| "It works, but I need it faster or smaller." | [05 · Performance and tuning](05-performance-and-tuning.md) | cache policy, LRU eviction, lazy costs, and eager WallBreaker construction. |

### The full guide index

| # | Guide | Goal |
|---|-------|------|
| 01 | [Quickstart](01-quickstart.md) | Build a Levenshtein WFST, compose it, and walk best paths. |
| 02 | [Choosing a variant](02-choosing-a-variant.md) | Decide across all eight variants: Levenshtein / Universal / WallBreaker / Generalized / Rewrite / Phonetic-NFA / Phonetic / Pipeline builder. |
| 03 | [Composing pipelines](03-composing-pipelines.md) | `compose`, `LazyWfstWrapper`, `DictionaryBackend`, and shortest-path search. |
| 04 | [Phonetic matching](04-phonetic-matching.md) | Rewrite rules vs. phonetic regex; the English / German / French rule sets. |
| 05 | [Performance and tuning](05-performance-and-tuning.md) | Cache policy, eviction, lazy costs, eager WallBreaker construction. |

---

## Install

duallity 0.3 is built against **liblevenshtein 0.9**, **lling-llang 0.2**, and **libdictenstein 0.2**.
The three companions map cleanly onto the three responsibilities in the pipeline: `libdictenstein`
holds the dictionary, `liblevenshtein` supplies the fuzzy-matching automata, and `lling-llang` provides
the weighted-transducer algebra (`Wfst`, the tropical semiring, `compose`). duallity is the thin adapter
that presents the first two *as* the third.

```toml
[dependencies]
duallity = "0.3"
liblevenshtein = "0.9"
lling-llang = "0.2"
libdictenstein = "0.2"
```

You only need to name the companions explicitly in `[dependencies]` when your own code references their
types directly (e.g. `libdictenstein::dynamic_dawg::char::DynamicDawgChar`, or
`lling_llang::composition::compose`) — which every non-trivial pipeline does. duallity also re-exports
the most-used `lling-llang` types (`Wfst`, `LazyWfst`, `LazyWfstWrapper`, `StateSource`,
`TropicalWeight`, `StateId`, `VocabId`, `WeightedTransition`, `Semiring`) from its own crate root for
convenience.

---

## Feature flags

duallity declares **no default features** (`default = []` in `Cargo.toml`). The entire non-phonetic
surface — every variant you need for fuzzy matching, composition, and rule-based phonetics — is
*always available* with no flag. A single opt-in feature, `phonetic-rules`, adds the automaton-backed
(regex → NFA) phonetic variants, because those pull a compiler stage from liblevenshtein.

| Feature | Enables | Pulls in |
|---------|---------|----------|
| *(always on — no feature)* | `LevenshteinWfst`, `UniversalLevenshteinWfst` / `BoundUniversalWfst`, `WallBreakerWfst` (+ `WallBreakerWfstBuilder`), `GeneralizedWfst` (+ `GeneralizedWfstBuilder`), `RewriteWfst` (+ `RewriteRule`, `CommonPhoneticRules`), `DictionaryBackend`, `PhoneticPipelineBuilder` (+ `PhoneticPipelineConfig`, `PhoneticMatch`, and `build_rewrite_wfst`) | — |
| `phonetic-rules` | the NFA-backed phonetic variants: `PhoneticWfst` / `PhoneticWfstBuilder`, `PhoneticNfaWfst`, `PhoneticStateSource`, and `PhoneticPipelineBuilder::{build, build_phonetic_nfa}` | `liblevenshtein/phonetic-rules` |

```toml
duallity = { version = "0.3", features = ["phonetic-rules"] }
```

> **The rule-based path needs no feature.** `RewriteWfst` and
> `PhoneticPipelineBuilder::build_rewrite_wfst` apply literal rewrite rules (like `ph → f`) and are
> available in the default build. Only the *regex → NFA* path (`PhoneticWfst`, `PhoneticNfaWfst`, and
> the pipeline builder's `build` / `build_phonetic_nfa` exits) is gated behind `phonetic-rules`. See
> [04 · Phonetic matching](04-phonetic-matching.md) for when you want each.

---

## How the guides fit the rest of the docs

- **Concepts and proofs** — [theory/](../theory/) develops semirings, Levenshtein automata,
  composition, universal automata, and the WallBreaker wall effect from first principles, with the
  single-source-of-truth [master notation table](../theory/README.md#master-notation).
- **Exact per-variant API** — [design/](../design/) documents each variant's 0.3.0 signatures,
  operational semantics, complexity, worked examples, and honest limitations.
- **Internals** — [architecture/](../architecture/) covers the WFST trait surface, state encoding,
  lazy evaluation and caching, and the registries.
- **Diagrams** — every illustration shares one [color legend](../diagrams/README.md): liblevenshtein =
  red-pink, libdictenstein = green, duallity = blue, lling-llang = yellow, output = purple; query/input
  tape = orange, dictionary/output tape = teal; accepting states = gold.
