# Architecture

This section explains *how* duallity is built. Where the [theory](../theory/) section answers **why**
a Levenshtein automaton is a weighted finite-state transducer (WFST) and what that buys us, and the
[design](../design/) section documents **each concrete variant** end to end, this section documents the
**shared machinery** that every variant is built from: the crate family duallity bridges, the trait
surface every WFST satisfies, the product-state encoding, the lazy-evaluation and caching pipeline, and
the registries that give dictionary nodes and automaton states stable identities.

## The three lenses

The documentation is deliberately split into three lenses so that each question is answered exactly
once, in the place best suited to it. Read them in order, or jump to the lens that matches your
question.

| Lens | Question it answers | For duallity, that means |
|------|---------------------|--------------------------|
| [**theory**](../theory/) | *Why* is this the right abstraction? | A Levenshtein neighborhood is a regular relation, so it is a WFST; edit cost lives in the tropical `` $`(\min, +)`$ `` semiring; composition `` $`T_1 \circ T_2`$ `` is the one operation that ties matchers together. |
| **architecture** (this section) | *How* is it realized in Rust? | The [`Wfst`](02-wfst-trait-surface.md) trait family, the `` $`\mathrm{StateId} = d \cdot M + a`$ `` [encoding](03-state-encoding-and-product-space.md), the [lazy cache](04-lazy-evaluation-and-caching.md), and the [interning registries](05-registries-and-interning.md). |
| [**design**](../design/) | *What* does each variant do, concretely? | One document per adapter — [Levenshtein](../design/levenshtein-wfst.md), [Universal](../design/universal-wfst.md), [WallBreaker](../design/wallbreaker-wfst.md), [Generalized](../design/generalized-wfst.md), and the [phonetic](../design/phonetic-wfst.md) family. |

The dividing line between **architecture** and **design** is the dividing line between the *infrastructure*
(one implementation, shared by all) and the *variants* (many implementations, one per matching strategy).
`compute_state` is the seam: architecture documents the trait that declares it and the cache that
memoizes it; design documents what each variant *computes*.

## Chapters

Every chapter states its **Prerequisites** and what it **Defines**, and holds one load-bearing
**invariant** — a property that is true of every duallity WFST and that the rest of the crate relies on.
Keeping the invariants explicit is what lets the variants in the design section stay small: each variant
only has to preserve the invariant, not re-establish it.

| # | Document | What you will learn | Invariant it upholds |
|---|----------|---------------------|----------------------|
| 01 | [Crate family and dependency graph](01-crate-family-and-dependency-graph.md) | The four crates, why duallity is a *separate* crate, what it re-exports, and how to migrate from liblevenshtein's old `wfst` module. | The dependency graph is a **DAG**: duallity depends on all three siblings and nothing depends back on it — the crate boundary is exactly the liblevenshtein `` $`\rightleftarrows`$ `` lling-llang cut. |
| 02 | [The WFST trait surface](02-wfst-trait-surface.md) | `Wfst`, `LazyWfst`, `StateSource`, `LatticeBackend`, and `CachePolicy`, with per-method pre/postconditions. | Every variant is a `Wfst<char, TropicalWeight>`, and the **eager view is a pure function of what has been expanded** — it never reports a transition that has not been computed. |
| 03 | [State encoding and the product space](03-state-encoding-and-product-space.md) | How `` $`(d, a)`$ `` pairs pack into one `` $`u32`$ `` `StateId`, and the two encoding regimes (arithmetic vs. registry). | Encoding is a **bijection** on the valid region: `` $`\mathrm{decode}(\mathrm{encode}(d, a)) = (d, a)`$ `` whenever `` $`0 \le a < M`$ ``. |
| 04 | [Lazy evaluation and caching](04-lazy-evaluation-and-caching.md) | `expand → compute_state → cache`, deterministic LRU, and the immutable/mutable split. | Each `StateId` is computed **at most once per cache epoch**; `compute_state` is referentially transparent — a pure function of the id. |
| 05 | [Registries and interning](05-registries-and-interning.md) | How nodes and abstract states get stable `` $`u32`$ `` ids, and the lock-based concurrency model. | Every distinct node / abstract state receives **one stable, dense `` $`u32`$ `` id** that never changes for the life of the WFST. |
| 06 | [The resource ABI and language bindings](06-resource-abi-and-bindings.md) | The vinary-tree resource ABI, the eight-function `duallity_*` C ABI, the nine automaton kinds, the capture-once rule, and the double-adapter bridge. | A dictionary revision is **captured exactly once** at construction; the resulting resource is immutable and may outlive its source, so every later expansion reads the same snapshot. |

## Source-module map

duallity is **one crate** (see [chapter 01](01-crate-family-and-dependency-graph.md)); its `src/` tree
groups into the families below. This table is the index from *source file* to the *document that explains
it* — architecture chapters for the shared machinery, design documents for the individual variants.

| Module family | Source files | Key public types | Documented in |
|---------------|--------------|------------------|---------------|
| **Crate glue** | `lib.rs` | prelude re-exports, `state_encoding`, `InvalidWeightError` | [architecture/01](01-crate-family-and-dependency-graph.md) (re-exports), [architecture/03](03-state-encoding-and-product-space.md) (`state_encoding`) |
| **Backend adapter** | `backend.rs` | `DictionaryBackend` | [architecture/02 §6](02-wfst-trait-surface.md#6-latticebackend--adapting-a-dictionary), [design/levenshtein-wfst §7](../design/levenshtein-wfst.md) |
| **Lazy cache** | `lazy_cache.rs` | `LazyStateCache`, `CachedCharState` | [architecture/04](04-lazy-evaluation-and-caching.md) |
| **Interning registries** | `node_key.rs`, `node_registry.rs` | `NodeRegistry`, node keys | [architecture/05](05-registries-and-interning.md) |
| **Parameterized Levenshtein** | `wrapper.rs`, `state_source.rs`, `state_source_support.rs` | `LevenshteinWfst`, `LevenshteinStateSource` | [architecture/02](02-wfst-trait-surface.md), [design/levenshtein-wfst](../design/levenshtein-wfst.md) |
| **Universal Levenshtein** | `universal_wrapper.rs`, `universal_state_source.rs`, `universal_state_support.rs` | `UniversalLevenshteinWfst`, `BoundUniversalWfst`, `UniversalLevenshteinStateSource`, `UniversalStateRegistry` | [architecture/02](02-wfst-trait-surface.md), [architecture/05](05-registries-and-interning.md), [design/universal-wfst](../design/universal-wfst.md) |
| **Generalized** | `generalized_wfst.rs`, `generalized_builder.rs`, `generalized_ops.rs`, `generalized_state_support.rs` | `GeneralizedWfst`, `GeneralizedWfstBuilder` | [design/generalized-wfst](../design/generalized-wfst.md) |
| **WallBreaker** | `wallbreaker_wfst.rs`, `wallbreaker_builder.rs`, `wallbreaker_results.rs` | `WallBreakerWfst`, `WallBreakerWfstBuilder` | [design/wallbreaker-wfst](../design/wallbreaker-wfst.md) |
| **Phonetic rewrite** | `phonetic_rewrite_wfst.rs`, `phonetic_rewrite_support.rs` | `RewriteWfst`, `RewriteRule`, `CommonPhoneticRules` | [design/phonetic-rewrite-wfst](../design/phonetic-rewrite-wfst.md) |
| **Phonetic regex/NFA** | `phonetic_nfa_wfst.rs`, `phonetic_nfa_support.rs` | `PhoneticNfaWfst` | [design/phonetic-nfa-wfst](../design/phonetic-nfa-wfst.md) |
| **Phonetic Levenshtein** | `phonetic_wfst.rs`, `phonetic_state_source.rs`, `phonetic_state_support.rs`, `phonetic_anchors.rs` | `PhoneticWfst`, `PhoneticWfstBuilder`, `PhoneticStateSource` | [design/phonetic-wfst](../design/phonetic-wfst.md) |
| **Phonetic pipeline** | `composed_phonetic.rs` | `PhoneticPipelineBuilder`, `PhoneticPipelineConfig`, `PhoneticMatch` | [design/phonetic-pipeline-builder](../design/phonetic-pipeline-builder.md) |
| **Resource ABI / bindings** | `ffi.rs`, `bindings.rs` | `DuallityWfst`, `DuallityStatus`, `WfstKind`, `create_wfst`, `ResourceDictionary`, `ResourceNode`, `AdapterProvider` | [architecture/06](06-resource-abi-and-bindings.md) |

The `phonetic_*` modules marked `#[cfg(feature = "phonetic-rules")]` in `lib.rs` (`phonetic_anchors`,
`phonetic_nfa_support`, `phonetic_nfa_wfst`, `phonetic_state_source`, `phonetic_state_support`,
`phonetic_wfst`) compile only when the [`phonetic-rules` feature](../guides/README.md) is enabled;
`composed_phonetic` and `phonetic_rewrite_*` are always compiled.

---

All symbols are from the [theory notation table](../theory/README.md#master-notation), which is the
single source of truth for mathematical notation and rendering (GitHub-flavored MathJax: inline math is
a backtick span wrapped in dollar signs, display math is a fenced `math` block). Diagram colors follow
the [shared legend](../diagrams/README.md).
