> **Research roadmap — design directions and external references.**
>
> Every document in this `docs/roadmap/` directory was **inherited from an earlier project**
> (`liblevenshtein-rust`) and describes a forward-looking **FST + CFG + Neural** text-normalization
> architecture outside the shipped `duallity` crate surface. The code paths, module names,
> benchmarks, and cross-references in these files belong to inherited research notes rather than
> this crate. They are retained as historical context and research background.
>
> 👉 For accurate, current documentation of what `duallity` actually is and does, start at the
> [documentation hub](../README.md) or the crate [README](../../README.md).

# Research Roadmap and External Design Archive

This collection predates `duallity`. It was written as a research-synthesis design study for a
hypothetical three-tier (FST → CFG → Neural) **text-normalization** system built on top of
`liblevenshtein-rust`. `duallity` instead solves a narrower, concrete problem: it exposes
Levenshtein automata as composable **lling-llang WFSTs**. None of the CFG/Earley parsing, neural
language models, lattice rescoring, MORK/PathMap integration, or adaptive-MSM machinery described
here belongs to this crate's source.

These files are kept (rather than deleted) because some of the underlying theory is sound and can
inform adjacent research directions. Where a concept here *does* map onto real `duallity` code, it
has been rewritten accurately in the canonical documentation tree and is cross-referenced below.

## What lives here

| File | Topic | Status vs. `duallity` |
|---|---|---|
| [`architecture.md`](architecture.md) | Three-tier FST+CFG+Neural text-normalization architecture, six-layer pipeline, MORK/LLM integration | **Research-only** — outside `duallity`'s crate surface. Only the generic tropical-semiring / $`\mathrm{FST} \circ \mathrm{FST}`$ composition theory is conceptually shared (see canonical [`theory/04-composition.md`](../theory/04-composition.md)). |
| [`cfg_grammar_correction.md`](cfg_grammar_correction.md) | CFG-based grammatical error correction (CYK, Earley, PCFG) | **Research-only** — parser, grammar, and CFG code are outside this crate. |
| [`lattice_parsing.md`](lattice_parsing.md) | Lattice parsing, parse forests, modified Earley | **Research-only** — lattice parsing is outside this crate. |
| [`lattice_data_structures.md`](lattice_data_structures.md) | `Lattice`/`Node`/`Edge`/`EarleyChart` data structures | **Research-only** — describes external types and dependencies absent from `Cargo.toml`. |
| [`implementation-comparison.md`](implementation-comparison.md) | Standalone vs. PathMap/MORK/MeTTaIL efficiency comparison | **Research-only** — these integrations are outside this crate. |
| [`adaptive-msm.md`](adaptive-msm.md) | Adaptive Move-Split-Merge time-series metric learning (FPTL) | **Research-only** — MSM metric learning is outside this crate. |
| [`nfa_phonetic_regex.md`](nfa_phonetic_regex.md) | NFA phonetic regular expressions | **Partially real** — the phonetic-regex → NFA → WFST concept *is* implemented. The accurate versions are canonical [`design/phonetic-wfst.md`](../design/phonetic-wfst.md) and [`design/phonetic-nfa-wfst.md`](../design/phonetic-nfa-wfst.md). The API names in this file (`PhoneticRegex::compile`, `.intersect`, Coq proofs) differ from duallity's API. |
| [`limitations.md`](limitations.md) | FST vs. CFG vs. Neural expressivity, Chomsky-hierarchy positioning | **Partially real** — the regular-language capability boundaries are valid and were re-grounded in canonical [`theory/07-regular-language-limits.md`](../theory/07-regular-language-limits.md). |
| [`references/papers.md`](references/papers.md) | 35+ paper bibliography for the research system | **Mostly out of scope** — the few relevant citations were promoted to canonical [`references/bibliography.md`](../references/bibliography.md). |

## Salvaged into the canonical documentation

| Concept here | Where it lives accurately now |
|---|---|
| Phonetic regex → NFA → WFST | [`design/phonetic-wfst.md`](../design/phonetic-wfst.md), [`design/phonetic-nfa-wfst.md`](../design/phonetic-nfa-wfst.md) |
| Tropical-semiring composition ($`T_1 \circ T_2`$) | [`theory/01-semirings-and-wfsts.md`](../theory/01-semirings-and-wfsts.md), [`theory/04-composition.md`](../theory/04-composition.md) |
| Regular-language / FST expressivity limits | [`theory/07-regular-language-limits.md`](../theory/07-regular-language-limits.md) |
| Schulz & Mihov 2002; Mohri 1997; Mohri, Pereira & Riley 2002 | [`references/bibliography.md`](../references/bibliography.md) |

---

*This index replaced the original inherited index (which carried inflated statistics, broken
cross-references to absent sibling directories, and links to the wrong GitHub repository). The
surrounding design files are preserved verbatim below their banners, and the original index remains
in the project's git history.*
