# Diagram catalog & shared color legend

This directory holds every diagram used across the `duallity` documentation. Each diagram is
kept as a **text source** (in [`src/`](src/)) and a **rendered `.svg`** committed alongside it, so
the diagrams are diffable, reproducible, and viewable without a build step.

The diagram **tooling is drawn from the pgmcp diagramming catalog**: **PlantUML** (the default —
byte-reproducible and able to typeset LaTeX via JLaTeXMath), **D2** (Terrastruct), and **Graphviz**,
each chosen for the illustration type it renders best (UML / state / class / sequence / activity →
PlantUML; architecture / flow → D2; node-edge graphs and coordinate grids → Graphviz). **Any diagram
whose labels carry mathematics is PlantUML**, so the formulae are typeset with `<latex>…</latex>`
rather than Unicode literals.

## Shared color legend (single source of truth)

Every diagram colors concepts identically so the reader builds one consistent mental model.

| Concept | Meaning | Hex |
|---|---|---|
| **liblevenshtein** | fuzzy matching / automata (the engine duallity wraps) | `#FADBD8` (red-pink) |
| **libdictenstein** | dictionary containers (DAWG, SCDAWG, tries) | `#D5F5E3` (green) |
| **duallity** | the WFST adapters (this crate) | `#D6EAF8` (blue) |
| **lling-llang** | the WFST algebra (`Wfst`, semirings, `compose`) | `#FCF3CF` (yellow) |
| **output / results** | ranked corrections, matches | `#E8DAEF` (purple) |
| **query side / input tape** | the misspelled input $`q`$ | `#FDEBD0` (warm orange) |
| **dictionary side / output tape** | the candidate term $`w`$ | `#D1F2EB` (teal) |
| **weight / cost** | tropical-weight annotations | `#EAECEE` (gray) |
| **epsilon / free step** | epsilon transition ($`\bar{1} = 0`$) | `#F2F3F4` (light gray, dashed) |
| **final / accepting** | accepting state, final weight | `#F9E79F` (gold border) |

Resource-ABI concepts (used in the binding and trust-boundary diagrams). These three hexes are
**family-shared**: the same colors name the same concepts in every sibling catalog
(liblevenshtein, libdictenstein, lling-llang), so cross-repo data-flow diagrams read identically.
None collides with a hex already claimed above:

| Concept | Meaning | Hex |
|---|---|---|
| **VtResource handle** | the retained two-word `(context, vtable)` interop resource that crosses the C ABI | `#DCEDC8` (pale lime, `#33691E` stroke) |
| **foreign / host trust zone** | code on the far side of the ABI boundary — every callback result is untrusted input | `#FFCDD2` (light rose, `#B71C1C` dashed border) |
| **leased / borrowed memory** | provider-owned buffers (edge pages, arc batches) borrowed only for the duration of one call | `#FFECB3` (pale amber, `#FF6F00` stroke) |

Operation edge colors (used in the edit-lattice and state diagrams):

| Edit operation | Color | Cost |
|---|---|---|
| **match** | `#2ECC71` (green) | $`0`$ |
| **substitute** | `#E74C3C` (red) | $`1`$ |
| **insert** (epsilon on input tape) | `#3498DB` (blue) | $`1`$ |
| **delete** (epsilon on output tape) | `#E67E22` (orange) | $`1`$ |

## Catalog

Tools: **P** = PlantUML, **D2** = Terrastruct D2, **G** = Graphviz. Diagrams marked ★ carry
`<latex>`-typeset mathematics.

| ID | File | Tool | Embedded by |
|----|------|------|-------------|
| D1 | [`levenshtein-edit-lattice`](levenshtein-edit-lattice.svg) | G | theory/02, design/levenshtein-wfst |
| D2 ★ | [`transducer-two-tape`](transducer-two-tape.svg) | P | theory/01, theory/03, design/levenshtein-wfst |
| D3 ★ | [`state-encoding-bijection`](state-encoding-bijection.svg) | P | architecture/03 |
| D4 | [`lazy-expand-sequence`](lazy-expand-sequence.svg) | P | architecture/02, architecture/04, design/levenshtein-wfst |
| D5 | [`universal-bound-factory`](universal-bound-factory.svg) | D2 | theory/05, design/universal-wfst |
| D6 ★ | [`characteristic-vector-window`](characteristic-vector-window.svg) | P | theory/05, design/universal-wfst |
| D7 | [`wallbreaker-pipeline`](wallbreaker-pipeline.svg) | D2 | theory/06, design/wallbreaker-wfst |
| D8 ★ | [`pigeonhole-principle`](pigeonhole-principle.svg) | P | theory/06, design/wallbreaker-wfst |
| D9 | [`wallbreaker-state-forest`](wallbreaker-state-forest.svg) | G | theory/06, design/wallbreaker-wfst |
| D10 ★ | [`rewrite-char-epsilon-chains`](rewrite-char-epsilon-chains.svg) | P | theory/03, design/phonetic-rewrite-wfst, guides/04 |
| D11 | [`phonetic-regex-nfa-product`](phonetic-regex-nfa-product.svg) | D2 | design/phonetic-nfa-wfst, design/phonetic-wfst |
| D12 ★ | [`composed-pipeline-typestate`](composed-pipeline-typestate.svg) | P | theory/04, design/phonetic-pipeline-builder |
| D13 | [`operationtype-taxonomy`](operationtype-taxonomy.svg) | G | theory/07, design/generalized-wfst |
| D14 | [`generalized-builder-flow`](generalized-builder-flow.svg) | D2 | design/generalized-wfst |
| Generalized transaction | [`generalized-expansion-transaction`](generalized-expansion-transaction.svg) | P | security/generalized-expansion-bounds |
| D15 | [`noderegistry-interning`](noderegistry-interning.svg) | D2 | architecture/05, security/hashing-and-collisions |
| D16 | [`crate-dependency-graph`](crate-dependency-graph.svg) | G | architecture/01, README |
| D17 ★ | [`tropical-semiring-algebra`](tropical-semiring-algebra.svg) | P | theory/01, README |
| D18 ★ | [`semiring-axioms-panel`](semiring-axioms-panel.svg) | P | theory/01 |
| D19 ★ | [`levenshtein-band-states`](levenshtein-band-states.svg) | P | theory/02 |
| D20 ★ | [`transpose-two-arc-chain`](transpose-two-arc-chain.svg) | P | theory/03 |
| D21 ★ | [`lazy-product-frontier`](lazy-product-frontier.svg) | P | theory/04 |
| D22 ★ | [`universal-position-set-transition`](universal-position-set-transition.svg) | P | theory/05 |
| D23 ★ | [`wall-growth-vs-seed`](wall-growth-vs-seed.svg) | P | theory/06 |
| D24 ★ | [`chomsky-placement`](chomsky-placement.svg) | P | theory/07 |
| D25 ★ | [`product-emit-continuation`](product-emit-continuation.svg) | P | design/generalized-wfst |
| D26 ★ | [`triple-product-frontier`](triple-product-frontier.svg) | P | design/phonetic-wfst |
| D27 | [`wfst-trait-surface-class`](wfst-trait-surface-class.svg) | P | architecture/02 |
| D28 ★ | [`cache-policy-lru-eviction`](cache-policy-lru-eviction.svg) | P | architecture/04, guides/05 |
| D29 | [`variant-decision-tree`](variant-decision-tree.svg) | P | guides/02 |
| D30 ★ | [`compose-search-sequence`](compose-search-sequence.svg) | P | guides/03 |
| D31 | [`phonetic-route-decision`](phonetic-route-decision.svg) | P | guides/04 |
| D32 | [`panic-safety-boundary`](panic-safety-boundary.svg) | P | engineering/safety-and-panics |
| D33 | [`rwlock-lock-lifecycle`](rwlock-lock-lifecycle.svg) | P | engineering/concurrency-and-locking |
| D34 | [`test-suite-map`](test-suite-map.svg) | P | engineering/testing |
| D35 ★ | [`threat-surface-resource-bounds`](threat-surface-resource-bounds.svg) | P | security/threat-model |
| D36 ★ | [`fzf-prefix-shared-dp`](fzf-prefix-shared-dp.svg) | P | design/fzf-wfst |
| D37 ★ | [`duallity-resource-abi-dataflow`](duallity-resource-abi-dataflow.svg) | P | architecture/06 |
| D38 | [`wfst-new-capture-compose-sequence`](wfst-new-capture-compose-sequence.svg) | P | architecture/06 |
| D39 | [`foreign-provider-trust-boundary`](foreign-provider-trust-boundary.svg) | P | security/threat-model |
| — ★ | [`composition-pipeline`](composition-pipeline.svg) | P | README, guides/01, guides/03, theory/04 |

## Rendering

All sources render to SVG with the pgmcp diagramming toolbox engines, from this directory:

```sh
# PlantUML  (.puml → .svg) — the default; run headless so it does not require an X display.
# <latex>…</latex> labels are typeset by the bundled JLaTeXMath.
DISPLAY= JAVA_TOOL_OPTIONS='-Djava.awt.headless=true' \
  plantuml -tsvg -o .. src/*.puml            # writes ../<name>.svg
DISPLAY= JAVA_TOOL_OPTIONS='-Djava.awt.headless=true' \
  plantuml -tsvg composition-pipeline.puml   # the top-level flagship

# D2        (.d2 → .svg, ELK layout for clean architecture diagrams)
for f in src/*.d2;   do d2 --layout elk "$f" "$(basename "${f%.d2}").svg"; done

# Graphviz  (.dot → .svg)
for f in src/*.dot;  do dot -Tsvg "$f" -o "$(basename "${f%.dot}").svg"; done
```

**JLaTeXMath caveats (this PlantUML build).** The bundled JLaTeXMath renders the common math commands
(`\oplus`, `\min`, `\bar{0}`, `\mathbb{K}`, `\chi`, `\lfloor\rfloor`, `\langle\rangle`, `\bigoplus`,
`\Theta`, subscripts/superscripts, …) but **not** `\otimes` (it renders blank) or `\lvert`/`\rvert`
(parse error). Diagram sources therefore write the tropical *times* operator as the literal U+2297
(CIRCLED TIMES) character and use plain `|…|` for bars. GitHub-flavored Markdown MathJax has no such gap, so the prose uses `\otimes`
and `\lvert\rvert` freely.

If a local engine is unavailable, the [Kroki](https://kroki.io) gateway renders the same sources over
HTTP. Use `rsvg-convert` to rasterize an SVG to PNG/PDF for crates.io or slide decks.
