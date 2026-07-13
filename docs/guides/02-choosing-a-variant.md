# 02 · Choosing a variant

duallity ships **eight** things you can hand a caller: seven WFST variants and one front-end that
*emits* them. This guide picks one from your situation — *what you are matching* and *how you will call
it*. The full, exact API for each is in [design/](../design/README.md), whose two
[selection matrices](../design/README.md#variant-selection-matrix--capability-and-cost) this guide
distills into concrete thresholds.

Throughout, `` $`n = \lvert q \rvert`$ `` is the query length in Unicode scalars, `` $`k`$ `` is
`max_distance`, `` $`c \in \{0, 1, 2\}`$ `` is the number of enabled continuation-state classes
(Standard / Transposition / MergeAndSplit), and `` $`M`$ `` is the state-encoding radix
([master notation](../theory/README.md#master-notation)).

---

## Decision tree

<!--
  NEW DIAGRAM (requested) — D18 · variant-decision-tree
    rendered SVG : ../diagrams/variant-decision-tree.svg
    source       : docs/diagrams/src/variant-decision-tree.d2   (D2 / ELK layout — decision flow)
    add to the diagrams/README catalog once rendered; embedded by guides/02.
  Until it is rendered, the ASCII tree below is the authoritative fallback.
-->

The rendered decision tree will live at `../diagrams/variant-decision-tree.svg` (catalog entry D18,
built with D2). Until then, this ASCII fallback is authoritative:

```text
START ─ what are you matching?
│
├─ Sound-alike (phonetic)?
│   ├─ with literal rules (ph→f, sch→sh)        → RewriteWfst         (compose in front of a matcher; NO feature)
│   ├─ with a regex, over a dictionary          → PhoneticWfst        (feature: phonetic-rules)
│   └─ with a regex, as a bare pattern stage    → PhoneticNfaWfst      (feature: phonetic-rules; no dictionary)
│
└─ Edit-distance (fuzzy) matching?
    │
    ├─ Is k large (≥ 4–5) over a big dictionary?
    │   └─ yes                                   → WallBreakerWfst     (needs an SCDAWG substring index)
    │
    ├─ Many queries vs. the SAME dictionary & k, with k ≤ 255?
    │   └─ yes                                   → BoundUniversalWfst  (build the query-agnostic automaton once, reuse)
    │
    ├─ Need a runtime-configurable operation set (transposition / merge-split / custom)?
    │   └─ yes                                   → GeneralizedWfst     (runtime OperationSet; Unit = char)
    │
    └─ otherwise — the common case, small–moderate k
                                                 → LevenshteinWfst     (start here)

Assembling a phonetic STAGE rather than choosing a matcher?
                                                 → PhoneticPipelineBuilder  (a front-end that emits the above)
```

---

## Decision guide — all eight variants, with thresholds and Big-O

Each row links to its design page. Costs follow [design/README](../design/README.md#variant-selection-matrix--capability-and-cost);
`` $`\delta`$ `` is the out-degree of a dictionary node (its child count).

| Variant | `max_distance` | Best `` $`k`$ `` range | Build cost | Per-query / per-state cost | Radix `` $`M`$ `` | Feature |
|---|---|---|---|---|---|---|
| [`LevenshteinWfst`](../design/levenshtein-wfst.md) | `usize` | small–moderate (`` $`0`$ `` – `` $`3`$ ``) | `` $`O(n)`$ `` (per query) | `` $`O\!\bigl(\delta(1{+}c)\bigr)`$ `` / state | `` $`(n{+}1)(k{+}1)(1{+}c)`$ `` | — |
| [`BoundUniversalWfst`](../design/universal-wfst.md) / [`UniversalLevenshteinWfst`](../design/universal-wfst.md) | `u8` | small–moderate, `` $`k \le 255`$ `` | factory `` $`O(1)`$ ``; `with_query` `` $`O\!\bigl(n(n{+}k)\bigr)`$ `` | `` $`\lvert\Sigma\rvert`$ ``-independent walk | `` $`(n{+}1)^2(2k{+}1)`$ `` | — |
| [`WallBreakerWfst`](../design/wallbreaker-wfst.md) | `usize` | **large** (`` $`\ge 4`$ – `` $`5`$ ``) | **eager**: whole query at construction | view over the finished forest | — (finite forest) | — |
| [`GeneralizedWfst`](../design/generalized-wfst.md) | `u8` (default `` $`2`$ ``) | small–moderate | lazy product graph | `` $`O(\delta)`$ `` / state | registry-bounded | — |
| [`RewriteWfst`](../design/phonetic-rewrite-wfst.md) | — | n/a (not edit-bounded) | from a rule list | `num_states() = 1 + `continuations | `` $`1 + \text{continuations}`$ `` | — |
| [`PhoneticNfaWfst`](../design/phonetic-nfa-wfst.md) | — | n/a | lazy powerset over a compiled `NFAChar` | subset-construction / state | registry-bounded | `phonetic-rules` |
| [`PhoneticWfst`](../design/phonetic-wfst.md) | `u8` | small–moderate | compile regex → NFA → triple product (lazy) | `` $`O(\delta)`$ `` / state | `` $`\max\!\bigl((k{+}1)\cdot 1000,\ 10000\bigr)`$ `` | `phonetic-rules` |
| [`PhoneticPipelineBuilder`](../design/phonetic-pipeline-builder.md) | `u8` (config) | — (front-end) | assembles one stage | — (not a WFST) | — | mixed¹ |

¹ `build_rewrite_wfst()` needs no feature; `build_phonetic_nfa()` and `build()` need `phonetic-rules`.

### Reading the thresholds

- **Why WallBreaker only at large `` $`k`$ ``?** Its strength is jumping the combinatorial *wall* that
  grows exponentially with `` $`k`$ `` ([theory/06](../theory/06-wallbreaker-and-the-wall-effect.md),
  Theorem 6.1). At small `` $`k`$ `` the plain automaton's band is already narrow, so
  `LevenshteinWfst` / universal are simpler and avoid the SCDAWG requirement; WallBreaker also does its
  work *eagerly at construction* (see the caveat in [03 · Composing pipelines](03-composing-pipelines.md#6-wallbreaker-is-eager-and-borrows-its-dictionary)).
- **Why universal for many queries?** It builds the **query-agnostic** automaton `` $`U_k`$ `` once and
  reuses it across queries ([theory/05](../theory/05-universal-automata.md)); per-query cost is the
  dictionary walk plus final-weight extraction from the active universal positions, and it is
  independent of the alphabet size `` $`\lvert\Sigma\rvert`$ ``. Reuse it through the
  `BoundUniversalWfst<V, D>` factory: build once, call `with_query` per query. The `` $`k \le 255`$ ``
  ceiling is the `u8` type of `max_distance`.
- **Damerau–Levenshtein (adjacent transpositions)** is available in `LevenshteinWfst` (via
  `Algorithm::Transposition`), `BoundUniversalWfst::<Transposition, _>`, and
  `GeneralizedWfstBuilder::with_transposition()`. Choose `LevenshteinWfst` for one query over one
  dictionary and the universal variant when many queries reuse one automaton.
- **Merge / split edits** (OCR arities like `rn ↔ m`) work in
  `LevenshteinWfst::with_algorithm(..., Algorithm::MergeAndSplit)` for the fixed arities. Choose
  `GeneralizedWfst` when those arities or weights must be *composed at runtime* with custom operations.
- **Phonetic — rules vs. regex.** `RewriteWfst` (literal rules, no feature) applies rules
  **unconditionally** (no context/lookahead); `PhoneticWfst` (regex, `phonetic-rules`) matches a
  compiled pattern over a **finite** alphabet. [04 · Phonetic matching](04-phonetic-matching.md)
  compares them in depth.

---

## Dictionary backend × variant compatibility

Every dictionary-backed variant takes a [libdictenstein](../architecture/01-crate-family-and-dependency-graph.md)
dictionary, but the **trait bounds differ**, so not every backend fits every variant. The matrix below
is keyed by the actual bounds in `src/` (`✓` = compiles and is intended; `✗` = the bound is not
satisfied).

| Variant | `DynamicDawgChar` | `DoubleArrayTrieChar` | `SuffixAutomatonChar` | `Scdawg` (SCDAWG) | Bound (from `src/`) |
|---|:---:|:---:|:---:|:---:|---|
| [`LevenshteinWfst`](../design/levenshtein-wfst.md) | ✓ | ✓ | ✓ | ✓ | `Unit: Into<char> + TryFrom<char> + Copy` |
| [`UniversalLevenshteinWfst`](../design/universal-wfst.md) / [`BoundUniversalWfst`](../design/universal-wfst.md) | ✓ | ✓ | ✓ | ✓ | `Unit: Into<char> + TryFrom<char> + Copy` |
| [`GeneralizedWfst`](../design/generalized-wfst.md) | ✓ | ✓ | ✓ | †  | `DictionaryNode<Unit = char>` (strict) |
| [`WallBreakerWfst`](../design/wallbreaker-wfst.md) | ✗ | ✗ | †  | ✓ | `SubstringDictionary`, `Node: BidirectionalDictionaryNode`, `Unit: Into<u32>` |
| [`PhoneticWfst`](../design/phonetic-wfst.md) | ✓ | ✓ | ✓ | ✓ | `Unit: Into<char> + TryFrom<char>` (dictionary owned by value) |

† **Qualified.** A cell marked † compiles **iff** the backend satisfies the stated bound in your build:
`GeneralizedWfst` needs the node `Unit` to be exactly `char`, and `WallBreakerWfst` needs any
dictionary implementing `SubstringDictionary` **and** whose nodes implement
`BidirectionalDictionaryNode`. `Scdawg` is the canonical backend that qualifies for `WallBreaker`; the
plain forward tries (`DynamicDawgChar`, `DoubleArrayTrieChar`) do **not** (they are not substring
indexes).

**Dictionary-free variants** take no backend at all, so they are not in the matrix:
[`RewriteWfst`](../design/phonetic-rewrite-wfst.md) and [`PhoneticNfaWfst`](../design/phonetic-nfa-wfst.md)
carry no dictionary (they are stages you compose *against* a matcher), and
[`PhoneticPipelineBuilder`](../design/phonetic-pipeline-builder.md) only needs a dictionary on its
`build()` exit (which emits a `PhoneticWfst`); its `build_rewrite_wfst()` and `build_phonetic_nfa()`
exits need none.

### The backends themselves

| Backend | Unit | Good for |
|---|---|---|
| `DynamicDawgChar` | `char` (`u32`) | general-purpose Unicode; **updatable at runtime** — the default choice |
| `DoubleArrayTrieChar` | `char` | **static, read-heavy** dictionaries (fast reads, build once) |
| `SuffixAutomatonChar` | `char` | substring-oriented matching |
| `Scdawg` (SCDAWG) | substring / bidirectional | **required** by `WallBreakerWfst` (bidirectional substring search) |

See [05 · Performance and tuning](05-performance-and-tuning.md) for the read/update trade-offs between
these backends.

---

## Where to go next

- Ready to build one? → [01 · Quickstart](01-quickstart.md) has the runnable end-to-end story.
- Chaining several variants into one scorer? → [03 · Composing pipelines](03-composing-pipelines.md).
- Want the exact semantics, complexity proofs, and honest limitations per variant? →
  [design/](../design/README.md), and the theory of [universal automata](../theory/05-universal-automata.md)
  and the [WallBreaker wall effect](../theory/06-wallbreaker-and-the-wall-effect.md).
