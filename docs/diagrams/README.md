# Diagram catalog & shared color legend

This directory holds every diagram used across the `duallity` documentation. Each diagram is
kept as a **text source** (in [`src/`](src/)) and a **rendered `.svg`** committed alongside it, so
the diagrams are diffable, reproducible, and viewable without a build step. This matches the
convention established by the original [`composition-pipeline.puml`](composition-pipeline.puml) /
[`composition-pipeline.svg`](composition-pipeline.svg) pair.

The diagram **tooling is drawn from the pgmcp diagramming catalog** (the toolbox `diagramming`
domain): **PlantUML**, **D2** (Terrastruct), **Graphviz**, and **ditaa** — each chosen for the
illustration type it renders best (UML/state → PlantUML; architecture/flow → D2; node-edge graphs
and grids → Graphviz; fixed-layout panels → ditaa).

## Shared color legend (single source of truth)

Every diagram colors concepts identically so the reader builds one consistent mental model. The
palette extends the per-crate legend from the original composition diagram.

| Concept | Meaning | Hex |
|---|---|---|
| **liblevenshtein** | fuzzy matching / automata (the engine duallity wraps) | `#FADBD8` (red-pink) |
| **libdictenstein** | dictionary containers (DAWG, SCDAWG, tries) | `#D5F5E3` (green) |
| **duallity** | the WFST adapters (this crate) | `#D6EAF8` (blue) |
| **lling-llang** | the WFST algebra (`Wfst`, semirings, `compose`) | `#FCF3CF` (yellow) |
| **output / results** | ranked corrections, matches | `#E8DAEF` (purple) |
| **query side / input tape** | the misspelled input `q` | `#FDEBD0` (warm orange) |
| **dictionary side / output tape** | the candidate term `w` | `#D1F2EB` (teal) |
| **weight / cost** | tropical-weight annotations | `#EAECEE` (gray) |
| **ε / free step** | epsilon transition (`1̄ = 0`) | `#F2F3F4` (light gray, dashed) |
| **final / accepting** | accepting state, final weight | `#F9E79F` (gold border) |

Operation edge colors (used in the edit-lattice and state diagrams):

| Edit operation | Color | Cost |
|---|---|---|
| **match** | `#2ECC71` (green) | `0` |
| **substitute** | `#E74C3C` (red) | `1` |
| **insert** (ε on input tape) | `#3498DB` (blue) | `1` |
| **delete** (ε on output tape) | `#E67E22` (orange) | `1` |

## Catalog

| ID | File | Tool | Embedded by |
|----|------|------|-------------|
| D1 | [`levenshtein-edit-lattice`](levenshtein-edit-lattice.svg) | Graphviz | theory/02, design/levenshtein-wfst |
| D2 | [`transducer-two-tape`](transducer-two-tape.svg) | PlantUML | theory/01, theory/03, design/levenshtein-wfst |
| D3 | [`state-encoding-bijection`](state-encoding-bijection.svg) | D2 | architecture/03 |
| D4 | [`lazy-expand-sequence`](lazy-expand-sequence.svg) | PlantUML | architecture/02, architecture/04 |
| D5 | [`universal-bound-factory`](universal-bound-factory.svg) | D2 | theory/05, design/universal-wfst |
| D6 | [`characteristic-vector-window`](characteristic-vector-window.svg) | Graphviz | theory/05, design/universal-wfst |
| D7 | [`wallbreaker-pipeline`](wallbreaker-pipeline.svg) | D2 | theory/06, design/wallbreaker-wfst |
| D8 | [`pigeonhole-principle`](pigeonhole-principle.svg) | D2 | theory/06 |
| D9 | [`wallbreaker-state-forest`](wallbreaker-state-forest.svg) | Graphviz | design/wallbreaker-wfst |
| D10 | [`rewrite-char-epsilon-chains`](rewrite-char-epsilon-chains.svg) | PlantUML | theory/03, design/phonetic-rewrite-wfst |
| D11 | [`phonetic-regex-nfa-product`](phonetic-regex-nfa-product.svg) | D2 | design/phonetic-nfa-wfst, design/phonetic-wfst |
| D12 | [`composed-pipeline-typestate`](composed-pipeline-typestate.svg) | PlantUML | theory/04, design/phonetic-pipeline-builder |
| D13 | [`operationtype-taxonomy`](operationtype-taxonomy.svg) | Graphviz | theory/07, design/generalized-wfst |
| D14 | [`generalized-builder-flow`](generalized-builder-flow.svg) | D2 | design/generalized-wfst |
| D15 | [`noderegistry-interning`](noderegistry-interning.svg) | D2 | architecture/05, security/hashing-and-collisions |
| D16 | [`crate-dependency-graph`](crate-dependency-graph.svg) | Graphviz | architecture/01 |
| D17 | [`tropical-semiring-algebra`](tropical-semiring-algebra.svg) | PlantUML | theory/01 |
| — | [`composition-pipeline`](composition-pipeline.svg) | PlantUML | README, guides/03 |

## Rendering

All sources render to SVG with the pgmcp diagramming toolbox engines, from this directory:

```sh
# PlantUML  (.puml → .svg) — run headless so it does not require an X display
DISPLAY= JAVA_TOOL_OPTIONS='-Djava.awt.headless=true' \
  plantuml -tsvg -o .. src/*.puml        # writes ../<name>.svg

# D2        (.d2 → .svg, ELK layout for clean architecture diagrams)
for f in src/*.d2;   do d2 --layout elk "$f" "$(basename "${f%.d2}").svg"; done

# Graphviz  (.dot → .svg)
for f in src/*.dot;  do dot -Tsvg "$f" -o "$(basename "${f%.dot}").svg"; done

# ditaa     (.ditaa → .svg)
for f in src/*.ditaa; do ditaa --svg "$f" "$(basename "${f%.ditaa}").svg"; done
```

If a local engine is unavailable, the [Kroki](https://kroki.io) gateway (`localhost:8000` on this
machine) renders the same sources over HTTP. Use `rsvg-convert` to rasterize an SVG to PNG/PDF for
crates.io or slide decks.
