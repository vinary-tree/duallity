# Guides

Task-oriented documentation: install duallity, pick a variant, compose a pipeline, do phonetic
matching, and tune performance. These build on the concepts in [theory](../theory/) and the API in
[design](../design/), but you can follow them without reading those first.

| # | Guide | Goal |
|---|-------|------|
| 01 | [Quickstart](01-quickstart.md) | Build a Levenshtein WFST, compose it, and walk best paths. |
| 02 | [Choosing a variant](02-choosing-a-variant.md) | Decide between Levenshtein / Universal / WallBreaker / Generalized / Phonetic. |
| 03 | [Composing pipelines](03-composing-pipelines.md) | `compose`, `LazyWfstWrapper`, `DictionaryBackend`, and shortest-path search. |
| 04 | [Phonetic matching](04-phonetic-matching.md) | Rewrite rules vs. phonetic regex; the English/German/French rule sets. |
| 05 | [Performance and tuning](05-performance-and-tuning.md) | Cache policy, eviction, lazy costs, eager WallBreaker construction. |

## Install

duallity is built against liblevenshtein 0.9, lling-llang 0.2, and libdictenstein 0.2.

```toml
[dependencies]
duallity = "0.2"
liblevenshtein = "0.9"
lling-llang = "0.2"
libdictenstein = "0.2"
```

## Feature flags

| Feature | Default? | Enables | Pulls in |
|---------|----------|---------|----------|
| *(default)* | ✔ | `LevenshteinWfst`, `UniversalLevenshteinWfst` / `BoundUniversalWfst`, `WallBreakerWfst`, `GeneralizedWfst`, `RewriteWfst`, `DictionaryBackend`, `PhoneticPipelineBuilder` (+ `build_rewrite_wfst`) | — |
| `phonetic-rules` | ✗ | the NFA-backed phonetic variants: `PhoneticWfst` / `PhoneticWfstBuilder`, `PhoneticNfaWfst`, `PhoneticStateSource`, and `PhoneticPipelineBuilder::{build, build_phonetic_nfa}` | `liblevenshtein/phonetic-rules` |

```toml
duallity = { version = "0.2", features = ["phonetic-rules"] }
```

> The rule-based `RewriteWfst` (and `PhoneticPipelineBuilder::build_rewrite_wfst`) need **no** feature
> — only the regex→NFA path is gated.
