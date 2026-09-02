# Theory

This section builds, from first principles, the theory behind `duallity`: weighted finite-state
transducers (WFSTs), semirings, Levenshtein automata, composition, universal automata, the
WallBreaker algorithm, and the expressivity limits of regular transducers. It is the conceptual
foundation the [architecture](../architecture/) and [design](../design/) sections build on.

Read it in order if you are new to WFSTs; jump to a numbered chapter if you only need one idea. Every
chapter states its **Prerequisites** and what it **Defines**, proves its load-bearing claims in full,
and closes with a `## References` list whose entries are mirrored in the
[bibliography](../references/bibliography.md).

| # | Document | What you will learn |
|---|----------|---------------------|
| 01 | [Semirings and WFSTs](01-semirings-and-wfsts.md) | What a WFST is, the semiring abstraction, and why duallity uses the tropical $`(\min, +)`$ semiring. |
| 02 | [Edit distance and Levenshtein automata](02-edit-distance-and-levenshtein-automata.md) | Levenshtein distance, the edit lattice, and the automaton that accepts everything within distance $`k`$. |
| 03 | [The Levenshtein automaton as a transducer](03-levenshtein-as-transducer.md) | How the four edit operations become labelled, weighted transitions with query-side input and dictionary-side output. |
| 04 | [Composition](04-composition.md) | $`T_1 \circ T_2`$, lazy composition, and why a fuzzy matcher must *be* a WFST to participate. |
| 05 | [Universal automata](05-universal-automata.md) | The query-agnostic automaton, characteristic vectors, and reuse across queries. |
| 06 | [WallBreaker and the wall effect](06-wallbreaker-and-the-wall-effect.md) | The combinatorial "wall" at large $`k`$, the pigeonhole split, exact-substring seeding, and bidirectional extension. |
| 07 | [Regular-language limits](07-regular-language-limits.md) | What a Levenshtein/phonetic WFST can and cannot express, positioned in the Chomsky hierarchy. |

---

## Master notation

Every symbol used across the documentation is defined **here, once**, and referenced thereafter. This
table is the single source of truth for notation; the [glossary](../references/glossary.md) mirrors
these renderings in prose, and no page introduces a symbol absent from this table.

Mathematics is written in **GitHub-flavored MathJax**: inline math is a backtick span wrapped in
dollar signs — e.g. $`\oplus`$ — and display math is a fenced block whose info-string is
`math`. (A literal dollar character is written as an inline code span, `$`.)

### Strings and alphabets

| Symbol | Meaning |
|--------|---------|
| $`\Sigma`$ | the **alphabet** — the set of symbols. duallity works per Unicode scalar value (`char`), so $`\Sigma \subseteq \texttt{char}`$. |
| $`\Sigma^{\ast}`$ | the set of all finite strings over $`\Sigma`$ (the Kleene star of $`\Sigma`$). |
| $`\varepsilon`$ | the **empty string** / the **epsilon** label on a transition tape (consumes or produces nothing). |
| $`q`$ | the **query** string (the misspelled input being corrected). |
| $`n = \lvert q \rvert`$ | the length of the query, in Unicode scalars. |
| $`w`$ | a **dictionary term** (a candidate correction / word in the dictionary $`D`$). |
| $`m = \lvert w \rvert`$ | the length of a dictionary term. |
| $`q[i]`$, $`q[i \mathbin{..} j]`$ | the $`i`$-th scalar (0-indexed) and the half-open slice $`[i, j)`$. |
| $`D`$ | the **dictionary** — a [`libdictenstein`](https://github.com/vinary-tree/libdictenstein) container (DAWG, SCDAWG, …) of terms. |
| $`k`$ | the **maximum edit distance** (error bound); also written `max_distance`. |

### Edit distance

| Symbol | Meaning |
|--------|---------|
| $`d_{\mathrm{lev}}(q, w)`$ | the **Levenshtein (edit) distance** between $`q`$ and $`w`$: the minimum number of single-character insertions, deletions, and substitutions that turn one into the other. |
| $`d_{\mathrm{DL}}(q, w)`$ | the **Damerau–Levenshtein distance**: as above, plus adjacent **transposition** as a unit-cost operation. |
| $`L(q, k)`$ | the **Levenshtein neighborhood** $`\{\, w \in \Sigma^{\ast} : d_{\mathrm{lev}}(q, w) \le k \,\}`$ — the set the automaton of chapter 02 accepts. |
| $`\Delta`$ | the **edit-lattice cost matrix**; $`\Delta[i, j] = d_{\mathrm{lev}}(q[0 \mathbin{..} i],\, w[0 \mathbin{..} j])`$. |

### Semirings and weights

| Symbol | Meaning |
|--------|---------|
| $`\mathbb{K} = (K, \oplus, \otimes, \bar{0}, \bar{1})`$ | a **semiring** — the algebra of weights, carrier set $`K`$. |
| $`\oplus`$ | the semiring **plus** (combines *alternative* paths to the same place). |
| $`\otimes`$ | the semiring **times** (combines weights *along* one path). |
| $`\bar{0}`$ | the **additive identity** / annihilator: $`a \oplus \bar{0} = a`$, $`a \otimes \bar{0} = \bar{0}`$ — "no path / forbidden". |
| $`\bar{1}`$ | the **multiplicative identity**: $`a \otimes \bar{1} = a`$ — "a free step". |
| $`\bigoplus_{i} a_i`$ | iterated $`\oplus`$ over a finite index set. |
| **tropical** $`\mathbb{T}`$ | the semiring duallity uses: $`(\mathbb{R} \cup \{+\infty\},\ \min,\ +,\ +\infty,\ 0)`$. Here $`\oplus = \min`$, $`\otimes = +`$, $`\bar{0} = +\infty`$, $`\bar{1} = 0`$. |

The tropical semiring is defined in full as:

```math
\mathbb{T} \;=\; \bigl(\mathbb{R} \cup \{+\infty\},\; \min,\; +,\; +\infty,\; 0\bigr),
\qquad a \oplus b = \min(a, b), \qquad a \otimes b = a + b .
```

> ⚠️ **Naming gotcha.** In [`lling_llang`](https://github.com/vinary-tree/lling-llang),
> `TropicalWeight::zero()` is the value **$`+\infty`$** (the additive identity $`\bar{0}`$,
> meaning "no path"), and `TropicalWeight::one()` is the value **$`0`$** (the multiplicative
> identity $`\bar{1}`$, a free step). The method names follow the *algebraic* role, not the
> numeric value. This is the single most common point of confusion; it is called out wherever it
> matters.

### Transducers, paths, and composition

| Symbol | Meaning |
|--------|---------|
| $`T = (\Sigma_i, \Sigma_o, Q, I, F, E)`$ | a **weighted finite-state transducer** (WFST): input/output alphabets, state set, initial states $`I`$, final states $`F`$, weighted edges $`E`$. |
| $`E`$ | the weighted **transition relation** $`E \subseteq Q \times (\Sigma_i \cup \{\varepsilon\}) \times (\Sigma_o \cup \{\varepsilon\}) \times K \times Q`$, written $`\text{in} : \text{out} / w`$. |
| $`\pi`$, $`w(\pi) = \bigotimes_{e \in \pi} w(e)`$ | a **path** through $`T`$ and its $`\otimes`$-accumulated weight. |
| $`\rho(\pi)`$ | the **terminal (final) weight** of the last state of $`\pi`$ ($`\bar{0}`$ if that state is non-final). |
| $`T(x, y)`$ | the weight $`T`$ assigns the input/output pair $`(x, y)`$: $`T(x, y) = \bigoplus_{\pi : x \to y} w(\pi) \otimes \rho(\pi)`$, the $`\oplus`$-sum over all accepting paths reading $`x`$ and writing $`y`$. |
| $`\circ`$ | **composition**: $`(T_1 \circ T_2)`$ reads what $`T_1`$ reads, writes what $`T_2`$ writes, and matches $`T_1`$'s output tape against $`T_2`$'s input tape. |

### State encoding (the product automaton)

| Symbol | Meaning |
|--------|---------|
| $`\mathrm{StateId} \in [0, 2^{32})`$ | a single `u32` identifying a state of a duallity WFST. |
| $`d`$ | a **dictionary-node id** (one component of a product state). |
| $`a`$ | an **automaton-state id** (the other component — a query position, a universal-automaton state, or a product-automaton state). |
| $`M`$ | `max_automaton_states`, the **radix** of the encoding. |
| encode | $`\mathrm{StateId} = d \cdot M + a`$ (requires $`0 \le a < M`$). |
| decode | if $`M > 0`$, $`d = \lfloor \mathrm{StateId} / M \rfloor`$ and $`a = \mathrm{StateId} \bmod M`$; otherwise decoding is invalid. |
| $`M_{\mathrm{lev}}`$ | the standard-Levenshtein radix, $`(n{+}1)(k{+}1)(1{+}c)`$ where $`c`$ is the number of enabled continuation-state classes. |
| $`M_{\mathrm{uni}}`$ | the universal radix, $`(n{+}1)^2 (2k{+}1)`$ (registry-bounded). |
| $`M_{\mathrm{phon}}`$ | the phonetic radix, $`\max\bigl((k{+}1)\cdot 1000,\ 10000\bigr)`$. |

> The arithmetic encoding $`\mathrm{StateId} = d \cdot M + a`$ is the **Levenshtein path**'s
> scheme (see [architecture/03](../architecture/03-state-encoding-and-product-space.md)). The
> Universal, Generalized, Phonetic-NFA, WallBreaker, and Rewrite variants instead assign **dense ids
> from a registry**; both regimes are documented in architecture/03.

### Automaton state functions

| Symbol | Meaning |
|--------|---------|
| $`(d,\, (i, e))`$ | a parameterized-Levenshtein product state: dictionary node $`d`$, query position $`i`$ consumed, accumulated edit cost $`e`$. |
| $`\mathrm{rem} = n - i`$ | the number of unconsumed (remaining) query scalars at position $`i`$. |
| $`V`$ | a **position variant** — $`V \in \{\textsf{Standard},\ \textsf{Transposition},\ \textsf{MergeAndSplit}\}`$ — the type parameter selecting the metric. |
| $`s_n(w, i)`$ | the **relevant subword** of $`w`$ around position $`i`$ for radius $`n`$ (defined below). |
| $`\chi(c, s)`$ | the **characteristic vector** of a character $`c`$ over a window $`s`$: the bit vector with $`\chi(c, s)_j = 1`$ iff $`s_j = c`$. |
| $`\Pi`$ | a **universal-automaton state**: a subsumption-reduced set of positions $`\langle \mathrm{offset},\ \mathrm{errors},\ \mathrm{type} \rangle`$. |

The relevant subword is defined by (with $`(\cdot)^{+} = \max(\cdot, 0)`$ and $`1`$-indexed
positions):

```math
s_n(w, i) \;=\; \underbrace{\texttt{\$} \cdots \texttt{\$}}_{(\,n + 1 - i\,)^{+}}\;
w\bigl[\, \max(i - n,\, 1) \,\mathbin{..}\, \min(\lvert w \rvert,\ i + n + 1) \,\bigr] .
```

The `$` sentinel pads out-of-range left positions (Schulz & Mihov [3]). At dictionary depth
$`d`$ duallity evaluates $`s_k(q,\, d{+}1)`$ (`relevant_subword_at`,
`universal_state_support.rs`).

---

## Diagram conventions

All diagrams in this documentation use one shared color legend (the single source of truth is
[`../diagrams/README.md`](../diagrams/README.md)): `liblevenshtein` = red-pink, `libdictenstein` =
green, `duallity` = blue, `lling-llang` = yellow, output = purple; query/input tape = orange,
dictionary/output tape = teal; match = green, substitute = red, insert = blue, delete = orange;
accepting states = gold. Mathematical labels are typeset with PlantUML `<latex>…</latex>` (JLaTeXMath)
rather than Unicode literals.
