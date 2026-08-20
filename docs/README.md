# duallity documentation

Welcome to the full documentation for **duallity** — *Levenshtein automata as lling-llang WFSTs*.
This hub maps the corpus and suggests reading orders. The crate-root [`README`](../README.md) is the
one-page overview; everything below goes deeper, from first-principles theory through the Rust
architecture, per-variant design, task-oriented guides, and the engineering/security envelope.

## The map

```
docs/
├── theory/          ← what a Levenshtein WFST is, and why (semirings, automata, composition, limits)
├── architecture/    ← how it is built (traits, state encoding, laziness, registries)
├── design/          ← one page per WFST variant (API + exact semantics + honest limits)
├── guides/          ← task-oriented usage (quickstart, choosing, composing, phonetics, tuning)
├── engineering/     ← safety, concurrency, testing
├── security/        ← threat model, hashing & collisions
├── scientific-ledger/ ← reproducible performance and verification observations
├── references/      ← bibliography (with DOIs) + glossary
├── diagrams/        ← all diagram sources + rendered SVGs + the shared color legend
└── archive/         ← inherited research designs, retained verbatim, outside the shipped crate surface
```

| Section | Start here |
|---------|-----------|
| **Theory** | [theory/README](theory/README.md) — and the [master notation table](theory/README.md#master-notation) every other page uses |
| **Architecture** | [architecture/README](architecture/README.md) |
| **Design (variants)** | [design/README](design/README.md) — the variant selection matrix |
| **Guides** | [guides/README](guides/README.md) — install + feature flags |
| **Foreign-language bindings** | [bindings/README](bindings/README.md) — C, C++, JavaScript, TypeScript, and ClojureScript guides with executable evidence |
| **Engineering** | [engineering/README](engineering/README.md) |
| **Security** | [security/README](security/README.md) |
| **References** | [bibliography](references/bibliography.md) · [glossary](references/glossary.md) |
| **Diagrams** | [diagrams/README](diagrams/README.md) — catalog + color legend |

## Reading orders

**Newcomer (≈30 min)** — understand the idea and run it:
1. crate [README](../README.md)
2. [theory/01 · Semirings and WFSTs](theory/01-semirings-and-wfsts.md)
3. [theory/02 · Edit distance and Levenshtein automata](theory/02-edit-distance-and-levenshtein-automata.md)
4. [guides/01 · Quickstart](guides/01-quickstart.md)
5. [guides/02 · Choosing a variant](guides/02-choosing-a-variant.md)

**Implementer (≈2 h)** — integrate or extend duallity:
1. the Newcomer path, then
2. [theory/03 · The Levenshtein automaton as a transducer](theory/03-levenshtein-as-transducer.md) and [theory/04 · Composition](theory/04-composition.md)
3. all of [architecture/](architecture/README.md)
4. the [design](design/README.md) page(s) for the variant(s) you use
5. [guides/03 · Composing pipelines](guides/03-composing-pipelines.md) and [guides/05 · Performance and tuning](guides/05-performance-and-tuning.md)
6. [engineering/](engineering/README.md) and [security/](security/README.md)

**Researcher (≈3 h)** — the full theory and provenance:
1. all of [theory/](theory/README.md) (01 → 07), including the complete correctness proofs
2. [references/bibliography](references/bibliography.md)
3. the [design](design/README.md) pages for exact semantics
4. [theory/06 · WallBreaker](theory/06-wallbreaker-and-the-wall-effect.md) and [theory/07 · Regular-language limits](theory/07-regular-language-limits.md)

**Binding author (≈45 min)** — call duallity from C, C++, or JavaScript:
1. [architecture/06 · The resource ABI and language bindings](architecture/06-resource-abi-and-bindings.md) — the seven-function C ABI, the nine kinds, capture-once, and the double adapter
2. [guides/07 · Language bindings](guides/07-language-bindings.md) — the nine-section per-language guide
3. the [binding hub](bindings/README.md), then the per-language C, C++, or JavaScript-family guide
4. [security/threat-model](security/threat-model.md) — why a foreign dictionary is untrusted input

## Conventions

- **Mathematics is GitHub-flavored MathJax.** Inline math is a backtick span wrapped in dollar signs —
  e.g. `` $`\oplus`$ `` renders `` $`\oplus`$ `` — and display math is a fenced block whose info-string
  is `math`. (A literal dollar character is written as an inline code span, `` `$` ``.) Every symbol is
  defined once in the [master notation table](theory/README.md#master-notation); the
  [glossary](references/glossary.md) mirrors those renderings in prose. These two files plus the
  diagram [color legend](diagrams/README.md) are the three **single sources of truth** the rest of the
  corpus draws from.
- **Diagrams** use one [shared color legend](diagrams/README.md): liblevenshtein = red-pink,
  libdictenstein = green, duallity = blue, lling-llang = yellow, output = purple; query/input tape =
  orange, dictionary/output tape = teal; match/substitute/insert/delete = green/red/blue/orange;
  accepting = gold. Each is a committed source (PlantUML, D2, or Graphviz) plus a rendered SVG;
  mathematical labels are typeset with PlantUML `<latex>` where the illustration carries formulae.
- **⚠ Honest limitations** are flagged in the design pages where a variant departs from its ideal —
  e.g. the [Rewrite WFST uses unconditional rules](design/phonetic-rewrite-wfst.md) and the
  [Pipeline builder does not itself compose or search](design/phonetic-pipeline-builder.md). Nothing
  here over-promises.

## A note on `archive/`

The [`archive/`](archive/README.md) directory holds documentation **inherited from an earlier project**
(`liblevenshtein-rust`) describing a research **FST + CFG + Neural** text-normalization system. It sits
**outside duallity's shipped crate surface** and is retained verbatim, under its original banners, as
historical context — it is deliberately *not* rewritten to these conventions. The canonical, accurate
documentation of what `duallity` actually is and does is everything *outside* `archive/`. Where a
concept there maps onto real duallity code (the phonetic-regex→NFA→WFST pipeline; the regular-language
expressivity limits; tropical-semiring composition), it has been re-grounded in the canonical tree and
is cross-referenced from [`archive/README`](archive/README.md).
