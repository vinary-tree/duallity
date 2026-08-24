# Design — the WFST variants

duallity ships a family of WFST wrappers. Distance variants implement
`Wfst<char, TropicalWeight>`; the [fzf adapter](fzf-wfst.md) implements
`Wfst<char, ArcticWeight>`. Each lazy wrapper composes when the surrounding
pipeline accepts its weight algebra ([theory/04](../theory/04-composition.md)).
This section documents each variant in depth: its exact 4.0.0-rc.3 API, its operational semantics, its
complexity, worked examples, and its **honest limitations**. Pick a variant by *what you are matching*
and *how you will call it*.

Throughout, symbols follow the [master notation table](../theory/README.md#master-notation): `` $`n = \lvert q\rvert`$ ``
is the query length in Unicode scalars, `` $`k`$ `` = `max_distance`, `` $`M`$ `` is the state-encoding
radix (`max_automaton_states`), and `` $`c \in \{0, 1, 2\}`$ `` is the number of enabled
continuation-state classes on the Levenshtein path (Standard / Transposition / MergeAndSplit).

---

## Variant selection matrix — capability and cost

The eight entries below are the eight things this crate can hand you. The first seven are WFSTs; the
eighth is a front-end that *emits* the others.

| Variant | Public type(s) | State set / id regime | Radix `` $`M`$ `` | Construction & per-query cost | Feature | Honest caveat |
|---------|----------------|-----------------------|--------|-------------------------------|---------|---------------|
| [**Levenshtein**](levenshtein-wfst.md) | `LevenshteinWfst<D>` | `` $`(d,\ \mathsf{N}(i,e))`$ `` + continuation cells; **arithmetic** `` $`a`$ `` | `` $`(n{+}1)(k{+}1)(1{+}c)`$ `` | fresh automaton **per query**: build `` $`O(n)`$ ``, expand `` $`O(\delta(1{+}c))`$ ``/state | none | rebuilds per query; eager reads need a prior `expand`; `k`: `usize` |
| [**Universal**](universal-wfst.md) | `UniversalLevenshteinWfst<V,D>`, `BoundUniversalWfst<V,D>` | `` $`(d,\ \Pi)`$ ``; **registry** ids (both components) | `` $`(n{+}1)^2(2k{+}1)`$ `` | factory `` $`O(1)`$ ``; `with_query` `` $`O(n(n{+}k))`$ ``; `` $`U_k`$ `` needs no per-query build; `` $`\lvert\Sigma\rvert`$ ``-independent | none | `` $`k \le 255`$ `` (`u8`); weight-`` $`\bar{1}`$ `` edges ⇒ pruning only at acceptance |
| [**WallBreaker**](wallbreaker-wfst.md) | `WallBreakerWfst<'a,D>`, `WallBreakerWfstBuilder<'a,D>` | super-start + one identity chain per match; **registry** forest ids (not a `` $`d\!\cdot\!M{+}a`$ `` product) | — (finite forest) | **eager**: runs the whole query at construction, then a view over the answer | none | requires an SCDAWG (`SubstringDictionary` + `BidirectionalDictionaryNode`); does the work up front |
| [**Generalized**](generalized-wfst.md) | `GeneralizedWfst<D>`, `GeneralizedWfstBuilder<'a,D>` | `` $`(d,\ \text{query byte offset},\ \text{cost})`$ `` + continuations; **registry** ids | registry-bounded | lazy product graph; operation set chosen at build time | none | `D::Node: DictionaryNode<Unit = char>` (stricter); `k`: `u8`, default 2 |
| [**Rewrite**](phonetic-rewrite-wfst.md) | `RewriteWfst`, `RewriteRule`, `CommonPhoneticRules` | state `` $`0`$ `` home/accepting + continuations; **no dictionary** | `` $`1 + \text{continuations}`$ `` | from a rule list; `num_states() = 1 + `continuations | none | rules apply **unconditionally** (no context/lookahead); expand context into explicit rules |
| [**Phonetic NFA**](phonetic-nfa-wfst.md) | `PhoneticNfaWfst` | epsilon-closed NFA-state sets, subset-construction **registry** ids; **no dictionary** | registry-bounded | lazy powerset construction over a compiled `NFAChar` | `phonetic-rules` | exact only over a **finite** alphabet `` $`\Sigma`$ `` (default printable ASCII); wide labels are `` $`\Sigma`$ ``-relative |
| [**Phonetic**](phonetic-wfst.md) | `PhoneticWfst<D>`, `PhoneticWfstBuilder<D>` | triple product `` $`(d,\ \text{NFA}\times\text{Levenshtein state})`$ ``; **registry** ids | `` $`\max\bigl((k{+}1)\cdot 1000,\ 10000\bigr)`$ `` | compile regex → NFA (Thompson) → triple product, lazy | `phonetic-rules` | builder **owns** the dictionary by value; weights affect ranking, not the `` $`k`$ ``-pruned set |
| [**Pipeline builder**](phonetic-pipeline-builder.md) | `PhoneticPipelineBuilder`, `PhoneticPipelineConfig`, `PhoneticMatch` | — (not a WFST; a type-state front-end) | — | assembles a stage: `build_rewrite_wfst` / `build_phonetic_nfa` / `build` | mixed¹ | ⚠ does **not** compose or search — the caller does; `PhoneticMatch` is caller-constructed |

¹ `build_rewrite_wfst()` needs no feature; `build_phonetic_nfa()` and `build()` need `phonetic-rules`.

**Radix, honestly.** Only the Levenshtein, Universal, and Phonetic variants have a closed-form radix
`` $`M`$ `` (`` $`M_{\mathrm{lev}}`$ ``, `` $`M_{\mathrm{uni}}`$ ``, `` $`M_{\mathrm{phon}}`$ ``). The
Generalized and Phonetic-NFA state ids are dense registry ids without a simple arithmetic form;
WallBreaker and Rewrite are not dictionary products at all — their dense ids index a pre-materialized
result forest and a rule graph, respectively (see the corrected *shared shape* below).

---

## Variant selection matrix — how you will call it

The same eight, keyed by the questions you actually ask when choosing:

| Variant | Best at query volume | Best at `` $`k`$ `` | Needs a dictionary? | Needs a substring index (SCDAWG)? | Feature-gated? |
|---------|----------------------|----------|---------------------|-----------------------------------|----------------|
| [Levenshtein](levenshtein-wfst.md) | low–moderate (one query at a time) | small–moderate | **yes** | no | no |
| [Universal](universal-wfst.md) | **high** (reuse one `BoundUniversalWfst`) | small–moderate (`` $`\le 255`$ ``) | **yes** | no | no |
| [WallBreaker](wallbreaker-wfst.md) | low–moderate (eager per query) | **large** (defeats the wall effect) | **yes** (SCDAWG) | **yes** | no |
| [Generalized](generalized-wfst.md) | low–moderate | small–moderate | **yes** | no | no |
| [Rewrite](phonetic-rewrite-wfst.md) | any (compose upstream of a matcher) | n/a (not edit-bounded) | no | no | no |
| [Phonetic NFA](phonetic-nfa-wfst.md) | any (bare pattern stage) | n/a | no | no | **`phonetic-rules`** |
| [Phonetic](phonetic-wfst.md) | low–moderate | small–moderate | **yes** | no | **`phonetic-rules`** |
| [Pipeline builder](phonetic-pipeline-builder.md) | — (front-end) | — | `build()` yes; other exits no | no | mixed¹ |

A task-oriented decision guide — with concrete thresholds, trade-offs, and the dictionary-backend
choices — is in [guides/02 · Choosing a variant](../guides/02-choosing-a-variant.md).

---

## Shared shape

Every variant that is a transducer follows the same contract, established in
[theory](../theory/) and [architecture](../architecture/). Each page below states where its variant
*departs* from this shape — most importantly, the **honest limitations** flagged with ⚠.

- **labels** — input = query side, output = dictionary side ([theory/03](../theory/03-levenshtein-as-transducer.md)).
- **weights** — tropical `` $`\mathbb{T} = (\mathbb{R}\cup\{+\infty\},\ \min,\ +,\ +\infty,\ 0)`$ ``,
  lower is better ([theory/01](../theory/01-semirings-and-wfsts.md)). Mind the
  [naming gotcha](../theory/README.md#semirings-and-weights): `TropicalWeight::zero()` `` $`= +\infty`$ ``
  ( `` $`\bar{0}`$ ``, "no path" ) and `TropicalWeight::one()` `` $`= 0`$ `` ( `` $`\bar{1}`$ ``, a free
  step ).
- **state ids** — a product state `` $`(d, a)`$ `` — a dictionary-node id `` $`d`$ `` and an
  automaton-state id `` $`a`$ `` — packed into one `u32` as
  `` $`\mathrm{StateId} = d \cdot M + a`$ `` with radix `` $`M = `$ ``\ `max_automaton_states`
  ([architecture/03](../architecture/03-state-encoding-and-product-space.md)). **The arithmetic scheme
  — where `` $`a = i(k{+}1) + e`$ `` decodes to a `` $`(\text{query position } i,\ \text{edit cost } e)`$ ``
  cell — is the Levenshtein path's alone.** The **Universal, Generalized, Phonetic-NFA, WallBreaker, and
  Rewrite** variants instead assign `` $`a`$ `` (and, on the universal path, `` $`d`$ `` too) as **dense
  ids from a registry**, keyed by each variant's own state descriptor (a serialized universal
  position-set, an interned `` $`(\text{node},\ \text{byte offset},\ \text{cost})`$ `` tuple, an epsilon-closed
  NFA-state set, a `WallBreakerStateKey`, …); those ids do **not** decode arithmetically. WallBreaker and
  Rewrite are not dictionary products at all — their dense ids index a pre-materialized result forest and
  a rule graph. Both regimes are documented in
  [architecture/03](../architecture/03-state-encoding-and-product-space.md).
- **laziness** — states are computed on first touch and cached
  ([architecture/04](../architecture/04-lazy-evaluation-and-caching.md)). Two departures: **WallBreaker**
  runs its whole search *eagerly at construction* (the WFST is a view over the finished answer), and the
  **pipeline builder** is not a WFST — it assembles stages the caller then composes and searches.

---

## Where to go next

- New to the shape? Read [design/levenshtein-wfst](levenshtein-wfst.md) first — it grounds the labels,
  weights, encoding, and laziness that every other page assumes.
- Choosing between variants under real constraints? [guides/02 · Choosing a variant](../guides/02-choosing-a-variant.md).
- Composing several stages into one matcher? [theory/04 · Composition](../theory/04-composition.md) and
  [guides/03 · Composing pipelines](../guides/03-composing-pipelines.md).
</content>
