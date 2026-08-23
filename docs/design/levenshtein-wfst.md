# Levenshtein WFST

> **`LevenshteinWfst<D>`** — the core adapter, and the place to start. A query-parameterized
> Levenshtein automaton crossed with a dictionary `` $`D`$ ``, presented as a lazy
> `Wfst<char, TropicalWeight>`. Its kernel is `LevenshteinStateSource<D>`; its dictionary-vocabulary
> companion is `DictionaryBackend<D>` (§7). Always available — **no feature flag**.

All symbols below are defined in the [master notation table](../theory/README.md#master-notation); the
first use of each is linked there. Mathematics is GitHub-flavored MathJax: inline math is a backtick
span wrapped in dollar signs and display math is a fenced `math` block.

---

## 1. Intuition

`LevenshteinWfst::new(&dict, "helo", 2)` reads as *"the Levenshtein automaton for the query
`` $`q`$ `` `= "helo"` up to edit distance `` $`k`$ `` `= 2`, walking the dictionary `dict`, exposed as
a composable transducer."* Its accepting paths spell out exactly the dictionary terms in the
[neighborhood](../theory/README.md#master-notation) `` $`L(q, k)`$ `` — those within
[Levenshtein distance](../theory/README.md#master-notation) `` $`d_{\mathrm{lev}}(q, w) \le k`$ `` — and
each accepting path carries, as its [tropical](../theory/README.md#master-notation) `` $`\mathbb{T}`$ ``
weight, that term's edit distance. It is the [edit lattice](../theory/02-edit-distance-and-levenshtein-automata.md)
turned into labelled, weighted [transitions](../theory/03-levenshtein-as-transducer.md): the input tape
carries query characters, the output tape carries dictionary characters, and a path's weight is the
`` $`\otimes`$ ``-sum ( `` $`= +`$ `` in `` $`\mathbb{T}`$ `` ) of its arc costs plus the accepting
state's final weight.

<img src="../diagrams/transducer-two-tape.svg" alt="The four edit operations as labelled, weighted transitions on a query tape and a dictionary tape" width="820"/>

The wrapper is **lazy**: nothing but the start state exists until you walk it. Reading an edge computes
(and caches) the target state's edges on demand, so composing `LevenshteinWfst` with a downstream WFST
only materializes the product region a shortest-path search actually visits
([architecture/04](../architecture/04-lazy-evaluation-and-caching.md)).

---

## 2. Operational semantics

`LevenshteinWfst<D>` is the WFST view of the product of the dictionary trie with the Levenshtein
automaton for `` $`q`$ `` and `` $`k`$ ``. The kernel `LevenshteinStateSource<D>` realizes the metric
selected by an `Algorithm` value; below, `` $`n = \lvert q\rvert`$ `` is the query length in Unicode
scalars and `` $`k`$ `` = `max_distance`.

### 2.1 State set `` $`Q`$ ``

A product state pairs a **registered dictionary node** `` $`d`$ `` (a position in the dictionary trie)
with an **automaton state** — one of four tagged `` $`(i, e)`$ `` cells, where
`` $`i`$ `` is the query position already consumed and `` $`e`$ `` is the accumulated edit cost:

```math
Q \;=\; \underbrace{\bigl\{\,(d,\ \mathsf{N}(i,e))\,\bigr\}}_{\text{normal (always)}}
\;\cup\; \underbrace{\bigl\{\,(d,\ \mathsf{T}(i,e))\,\bigr\}}_{\texttt{Transposition}}
\;\cup\; \underbrace{\bigl\{\,(d,\ \mathsf{Mg}(i,e)),\ (d,\ \mathsf{Sp}(i,e))\,\bigr\}}_{\texttt{MergeAndSplit}},
\qquad 0 \le i \le n,\ \ 0 \le e \le k .
```

`` $`\mathsf{N}`$ `` is the ordinary Levenshtein cell; `` $`\mathsf{T}`$ ``, `` $`\mathsf{Mg}`$ ``, and
`` $`\mathsf{Sp}`$ `` are **continuation states** (§2.5) that exist only when the corresponding
`Algorithm` variant is selected — they emit the *second* arc of a two-arc multi-character edit. The
`` $`\mathsf{T}`$ `` tag is present under `Algorithm::Transposition`; the `` $`\mathsf{Mg}`$ `` and
`` $`\mathsf{Sp}`$ `` tags are present under `Algorithm::MergeAndSplit`.

Each product state is encoded as a single `u32` [`StateId`](../theory/README.md#master-notation) using
the **Levenshtein arithmetic scheme**
([architecture/03](../architecture/03-state-encoding-and-product-space.md)):

```math
\mathrm{StateId} \;=\; d \cdot M_{\mathrm{lev}} + a,
\qquad
a\bigl(\mathsf{N}(i,e)\bigr) = i\,(k{+}1) + e,
\qquad
a\bigl(\mathrm{slot}_s(i,e)\bigr) = s\,(n{+}1)(k{+}1) + i\,(k{+}1) + e ,
```

where `` $`a`$ `` is the [automaton-state id](../theory/README.md#master-notation), the continuation
slot index `` $`s \in \{1, 2, 3\}`$ `` numbers the enabled continuation classes in order
(transposition, then merge, then split), and the **radix** is

```math
M_{\mathrm{lev}} \;=\; (n{+}1)\,(k{+}1)\,(1 + c),
\qquad c = \begin{cases} 0 & \texttt{Standard},\\ 1 & \texttt{Transposition},\\ 2 & \texttt{MergeAndSplit}. \end{cases}
```

Here `` $`c`$ `` is the number of enabled continuation-state classes (`continuation_state_kinds`), so
`` $`M_{\mathrm{lev}}`$ `` equals `max_automaton_states`. The `` $`d`$ `` component is a **dense id
assigned by the dictionary-node registry** as the trie is discovered; only the automaton component
`` $`a`$ `` is arithmetic. Decoding is `` $`d = \lfloor \mathrm{StateId} / M_{\mathrm{lev}}\rfloor`$ ``,
`` $`a = \mathrm{StateId} \bmod M_{\mathrm{lev}}`$ `` — see
[theory/02 · the `` $`2k{+}1`$ `` band](../theory/02-edit-distance-and-levenshtein-automata.md#4-the-diagonal-band-and-the-compact-radix).

<img src="../diagrams/levenshtein-edit-lattice.svg" alt="The (position, edit-cost) band of automaton cells that the automaton component a indexes" width="820"/>

### 2.2 Initial state `` $`q_0`$ ``

```math
q_0 \;=\; \bigl(d = 0,\ \mathsf{N}(0, 0)\bigr) \;=\; 0 \cdot M_{\mathrm{lev}} + 0 \;=\; 0 .
```

The dictionary root is registered as node id `` $`0`$ `` at construction, and `` $`\mathsf{N}(0,0)`$ ``
encodes to `` $`0`$ ``, so `start()` is always the integer `` $`0`$ `` (test
`test_levenshtein_wfst_start_state`).

### 2.3 Final predicate and final weight

Only **normal** states can accept; continuation states are always non-final. A normal state
`` $`(d, \mathsf{N}(i, e))`$ `` is final iff its dictionary node terminates a term *and* the unconsumed
query tail can be deleted within the remaining budget:

```math
\text{final}\bigl(d, \mathsf{N}(i,e)\bigr) \;\iff\; d.\texttt{is\_final()} \ \wedge\ e + \mathrm{rem} \le k,
\qquad \mathrm{rem} = n - i .
```

When it accepts, its **final weight** is exactly that deletable tail:

```math
\rho\bigl(d, \mathsf{N}(i,e)\bigr) \;=\; \mathrm{rem} \;=\; n - i .
```

The `` $`e + \mathrm{rem} \le k`$ `` test (`within_max_distance`) is the load-bearing correctness
condition: it charges both the edits already made (`` $`e`$ ``) *and* the cost of deleting the
remaining `` $`\mathrm{rem}`$ `` query scalars, so a path is accepted only if the *total* edit distance
stays `` $`\le k`$ ``. A path's reported weight is `` $`w(\pi) \otimes \rho(\pi)`$ `` — the summed arc
costs plus `` $`\mathrm{rem}`$ `` — which equals `` $`d_{\mathrm{lev}}(q, w)`$ `` for the spelled term
`` $`w`$ ``.

### 2.4 The four standard operations

From a normal state `` $`(d, \mathsf{N}(i, e))`$ ``, for each dictionary out-edge labelled
`` $`c`$ `` leading to child `` $`d'`$ ``, the kernel emits (writing labels as
`` $`\text{in}:\text{out}\,/\,w`$ ``, with `` $`\varepsilon`$ `` the empty label):

| Operation | Guard | `` $`\text{in}:\text{out}/w`$ `` | Successor |
|-----------|-------|-------------------------------|-----------|
| **match** | `` $`i < n`$ ``, `` $`q[i] = c`$ `` | `` $`q[i] : c \,/\, 0`$ `` | `` $`(d',\ \mathsf{N}(i{+}1,\ e))`$ `` |
| **substitute** | `` $`i < n`$ ``, `` $`q[i] \ne c`$ ``, `` $`e < k`$ `` | `` $`q[i] : c \,/\, 1`$ `` | `` $`(d',\ \mathsf{N}(i{+}1,\ e{+}1))`$ `` |
| **insert** | `` $`e < k`$ `` | `` $`\varepsilon : c \,/\, 1`$ `` | `` $`(d',\ \mathsf{N}(i,\ e{+}1))`$ `` |
| **delete** | `` $`i < n`$ ``, `` $`e < k`$ `` | `` $`q[i] : \varepsilon \,/\, 1`$ `` | `` $`(d,\ \mathsf{N}(i{+}1,\ e{+}1))`$ `` |

**Match** and **substitute** consume one query scalar and one dictionary scalar; **insert** consumes a
dictionary scalar only (a character missing from the query); **delete** consumes a query scalar only
and *stays at the same dictionary node* `` $`d`$ `` (a character the dictionary term lacks). An edge is
pruned — never registered — when its successor would exceed the budget (`` $`e{+}1 > k`$ ``) or fail to
encode within `` $`M_{\mathrm{lev}}`$ `` (test
`test_state_source_does_not_register_pruned_normal_edges_at_max_distance`). See the literate pseudocode
in [theory/03 §3](../theory/03-levenshtein-as-transducer.md#3-the-transition-kernel-as-literate-pseudocode).

### 2.5 Adjacent transposition (Damerau–Levenshtein)

Because the WFST label type is a single `char`, one logical adjacent swap is emitted as **two arcs**: a
first arc that charges the unit cost and enters a `` $`\mathsf{T}`$ `` continuation state, and a second
arc that finishes the swap for free. Under `Algorithm::Transposition`, from `` $`(d, \mathsf{N}(i, e))`$ ``
with `` $`e < k`$ ``, `` $`i < n-1`$ ``, `` $`q[i] \ne q[i{+}1]`$ ``, an edge `` $`c = q[i{+}1]`$ `` to
`` $`d'`$ ``, and `` $`d'`$ `` itself having a `` $`q[i]`$ ``-edge to `` $`d''`$ ``:

| Arc | `` $`\text{in}:\text{out}/w`$ `` | Successor |
|-----|-------------------------------|-----------|
| transpose · 1 | `` $`q[i] : q[i{+}1] \,/\, 1`$ `` | `` $`(d',\ \mathsf{T}(i,\ e{+}1))`$ `` |
| transpose · 2 | `` $`q[i{+}1] : q[i] \,/\, 0`$ `` | `` $`(d'',\ \mathsf{N}(i{+}2,\ e{+}1))`$ `` |

The two arcs sum to unit cost `` $`1 + 0 = 1`$ `` and consume `` $`q[i]q[i{+}1]`$ `` against the swapped
dictionary path `` $`q[i{+}1]q[i]`$ `` — a genuine Damerau swap
[[5](#references)], not a free reordering. Test `test_levenshtein_wfst_transposition_reaches_final_state`
pins the two-arc shape.

### 2.6 Merge and split (OCR arities)

`Algorithm::MergeAndSplit` adds the OCR-style operations from liblevenshtein's operation set, each again
a two-arc chain (unit + free). Following duallity's `generalized_ops.rs` naming
(`OperationType::new(2, 1, …, "merge")`, first argument = **dictionary** arity): **merge** collapses two
dictionary scalars into one query scalar (the true word's `"rn"` read as a single `"m"`); **split**
expands one dictionary scalar into two query scalars (the true word's `"m"` read as `"rn"`). From
`` $`(d, \mathsf{N}(i, e))`$ `` with `` $`e < k`$ `` and an edge `` $`c`$ `` to `` $`d'`$ ``:

| Operation | Guard | Arc 1 | Arc 2 |
|-----------|-------|-------|-------|
| **merge** `` $`\langle 2\,\text{dict},\ 1\,\text{query}\rangle`$ `` | `` $`i < n`$ ``, `` $`d'`$ `` has an edge `` $`c'`$ `` to `` $`d''`$ `` | `` $`q[i] : c \,/\, 1 \ \to\ (d',\ \mathsf{Mg}(i,\ e{+}1))`$ `` | `` $`\varepsilon : c' \,/\, 0 \ \to\ (d'',\ \mathsf{N}(i{+}1,\ e{+}1))`$ `` (per edge `` $`c'`$ `` to `` $`d''`$ ``) |
| **split** `` $`\langle 1\,\text{dict},\ 2\,\text{query}\rangle`$ `` | `` $`i < n-1`$ `` | `` $`q[i] : c \,/\, 1 \ \to\ (d',\ \mathsf{Sp}(i,\ e{+}1))`$ `` | `` $`q[i{+}1] : \varepsilon \,/\, 0 \ \to\ (d',\ \mathsf{N}(i{+}2,\ e{+}1))`$ `` |

Merge's second arc emits each further dictionary scalar `` $`c'`$ `` with an `` $`\varepsilon`$ `` input;
split's second arc stays at `` $`d'`$ `` and consumes `` $`q[i{+}1]`$ `` with an `` $`\varepsilon`$ ``
output. Test `test_levenshtein_wfst_merge_and_split_reaches_final_state` pins the chain.

---

## 3. The 4.0.0-rc.1 API

### 3.1 Type and bounds

```rust,ignore
pub struct LevenshteinWfst<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
{ /* state_source: LevenshteinStateSource<D>, cache, max_distance: usize, algorithm: Algorithm */ }
```

The unit bound `Into<char> + TryFrom<char>` is what lets the *same* wrapper serve byte (`u8`) and
Unicode (`u32`) dictionaries: edit distance is always computed **per Unicode scalar**, never per byte
(test `test_levenshtein_wfst_utf8_support` exercises `café` / `naïve` / `北京`). `D: Clone` is required
because the state source holds its own clone of the dictionary handle — cheap for the shared-structure
dictionaries (`DynamicDawgChar`, `DoubleArrayTrieChar`, …).

### 3.2 Constructors and methods

```rust,ignore
impl<D> LevenshteinWfst<D> {
    // Construct with Algorithm::Standard.
    pub fn new(dictionary: &D, query: &str, max_distance: usize) -> Self;
    // Construct with an explicit algorithm (Standard / Transposition / MergeAndSplit).
    pub fn with_algorithm(dictionary: &D, query: &str, max_distance: usize, algorithm: Algorithm) -> Self;

    pub fn max_distance(&self) -> usize;         // the k it was built with
    pub fn algorithm(&self) -> Algorithm;        // the selected metric
    pub fn query(&self) -> &str;                 // borrows the original UTF-8 query (no reallocation)
    pub fn set_max_cache_size(&mut self, size: usize);  // honoured only under CachePolicy::Lru
}
```

`Algorithm` is re-exported from `liblevenshtein::transducer`. `query()` returns a borrow of the interned
query string, so repeated calls hand back the *same* pointer (test `test_levenshtein_wfst_creation`).

### 3.3 Trait impls and the laziness contract

`LevenshteinWfst<D>` implements both `Wfst<char, TropicalWeight>` and
`LazyWfst<char, TropicalWeight>` ([architecture/02](../architecture/02-wfst-trait-surface.md)). The lazy
methods — `expand`, `transitions_lazy`, `is_expanded`, `cache_policy` / `set_cache_policy`,
`computed_states`, `clear_cache` — drive on-demand computation; the eager `Wfst` methods read whatever
is already cached or registered:

- `start()` `` $`= 0`$ ``.
- `transitions(state)` returns an **empty slice** until you `expand(state)` (or call
  `transitions_lazy(state)`, which expands then reads).
- `is_final(state)` / `final_weight(state)` first consult the cache; on a miss they fall back to an
  on-the-fly registry probe (`final_weight_for_state`). That probe answers correctly only for states
  whose product components are **already registered** — i.e. the start state, or any state reached by
  expanding its predecessor. For an as-yet-unregistered state they return `false` and
  `TropicalWeight::zero()` `` $`= +\infty`$ `` (the `` $`\bar{0}`$ `` "no path" weight — see the
  [naming gotcha](../theory/README.md#semirings-and-weights)).

The practical rule is therefore **expand before you read**. The default cache policy is `CacheAll`;
switch to `CachePolicy::Lru { max_states }` (or call `set_max_cache_size`) for bounded memory
([architecture/04 §4](../architecture/04-lazy-evaluation-and-caching.md#4-cache-policy-and-deterministic-eviction)).

<img src="../diagrams/lazy-expand-sequence.svg" alt="Lazy expansion sequence: wrapper delegates to the state source, which computes and caches the state" width="800"/>

---

## 4. Complexity

Let `` $`n = \lvert q\rvert`$ ``, `` $`k`$ `` = `max_distance`, `` $`c`$ `` the continuation-class count
(§2.1), `` $`\delta`$ `` the out-degree of a dictionary node, and `` $`M_{\mathrm{lev}} = (n{+}1)(k{+}1)(1{+}c)`$ ``
the radix.

| Phase | Cost | Notes |
|-------|------|-------|
| **Construction** (`new` / `with_algorithm`) | `` $`O(n)`$ `` + one dictionary-handle clone | collects `` $`q`$ `` into scalars, sizes the codec, registers the root; **no** dictionary traversal |
| **Per-state expansion** (`expand`) | `` $`O\bigl(\delta \cdot (1 + c)\bigr)`$ `` | one pass over the node's out-edges, `` $`O(1)`$ `` arcs and one amortized-`` $`O(1)`$ `` registry insert per edge, plus one delete arc |
| **Whole-neighborhood walk** | `` $`O\bigl(\lvert R\rvert \cdot \delta \cdot (1{+}c)\bigr)`$ `` | `` $`\lvert R\rvert`$ `` = product states actually visited, bounded by `` $`\lvert D_{\text{reg}}\rvert \cdot M_{\mathrm{lev}}`$ `` |
| **Space** | `` $`O(\lvert R\rvert)`$ `` cached states | bounded by the LRU cap when `CachePolicy::Lru` is set |

The `u32` `StateId` bounds the addressable product space: `try_encode` returns `None` (silently pruning
that edge) once `` $`\lvert D_{\text{reg}}\rvert \cdot M_{\mathrm{lev}} \ge 2^{32}`$ `` (test
`test_state_source_does_not_register_unencodable_dictionary_edges`). Because the whole structure is
rebuilt per query, `` $`m`$ `` queries against the same `` $`(D, k)`$ `` pay construction `` $`m`$ ``
times — the motivation for the [universal variant](universal-wfst.md), whose `` $`U_k`$ `` is built once
and reused.

---

## 5. Worked end-to-end example

Take `` $`D = \{`$ ``\ `"hello"`, `"help"`, `"world"`\ `` $`\}`$ ``, query `` $`q = `$ `` `"helo"`
(`` $`n = 4`$ ``), and `` $`k = 2`$ ``, standard metric (`` $`c = 0`$ ``, so
`` $`M_{\mathrm{lev}} = 5 \cdot 3 \cdot 1 = 15`$ ``).

```rust,ignore
use duallity::LevenshteinWfst;
use liblevenshtein::transducer::Algorithm;
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use lling_llang::prelude::*;

let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help", "world"]);
let mut lev = LevenshteinWfst::new(&dict, "helo", 2);
assert_eq!(lev.max_distance(), 2);
assert_eq!(lev.query(), "helo");

let s0 = lev.start();          // == 0
lev.expand(s0);                // register the root's children, compute its arcs
assert!(lev.is_expanded(s0));
```

**Path to `"hello"` (edit distance 1 — one insertion).** Reading `q = "helo"` against the term
`"hello"`:

```text
(root, N(0,0))  --  h : h / 0  -->  (d_h,    N(1,0))     ▷ match
                --  e : e / 0  -->  (d_he,   N(2,0))     ▷ match
                --  l : l / 0  -->  (d_hel,  N(3,0))     ▷ match
                --  ε : l / 1  -->  (d_hell, N(3,1))     ▷ insert the extra 'l'
                --  o : o / 0  -->  (d_hello,N(4,1))     ▷ match; d_hello.is_final()
```

At `` $`(d_{\text{hello}}, \mathsf{N}(4,1))`$ ``: `` $`\mathrm{rem} = n - i = 0`$ ``, and
`` $`e + \mathrm{rem} = 1 + 0 = 1 \le 2`$ ``, so the state accepts with final weight
`` $`\rho = 0`$ ``. The path's total weight is `` $`(0{+}0{+}0{+}1{+}0) \otimes 0 = 1 = d_{\mathrm{lev}}(\texttt{"helo"}, \texttt{"hello"})`$ ``.

**Path to `"help"` (edit distance 1 — one substitution `o → p`).** `h:h/0`, `e:e/0`, `l:l/0`, then
`o:p/1` into `` $`(d_{\text{help}}, \mathsf{N}(4,1))`$ ``, which is final with `` $`\rho = 0`$ `` and
total weight `` $`1`$ ``.

**`"world"` is rejected.** `` $`d_{\mathrm{lev}}(\texttt{"helo"}, \texttt{"world"}) = 4 > 2`$ ``: no
prefix of `"world"` survives two edits against `"helo"`, so every candidate arc is pruned before an
accepting state is reached.

**Transposition: `"tset"` `` $`\to`$ `` `"test"` (Damerau distance 1).** With `Algorithm::Transposition`
over `` $`D = \{`$ ``\ `"test"`\ `` $`\}`$ ``, `` $`q = `$ `` `"tset"`, `` $`k = 2`$ ``:

```text
(root, N(0,0)) -- t : t / 0 --> (d_t,  N(1,0))      ▷ match
               -- s : e / 1 --> (d_te, T(1,1))      ▷ transpose · 1  (out = q[2]='e')
               -- e : s / 0 --> (d_tes, N(3,1))     ▷ transpose · 2  (in = q[2]='e', out = q[1]='s')
               -- t : t / 0 --> (d_test,N(4,1))     ▷ match; d_test.is_final()
```

The input tape reads `t,s,e,t` `` $`= q`$ `` and the output tape reads `t,e,s,t` `` $`= `$ `` `"test"`;
acceptance at `` $`\mathsf{N}(4,1)`$ `` gives `` $`\rho = 0`$ `` and total weight
`` $`1 = d_{\mathrm{DL}}(\texttt{"tset"}, \texttt{"test"})`$ ``.

```rust,ignore
let _dl = LevenshteinWfst::with_algorithm(&dict, "tset", 2, Algorithm::Transposition);
```

---

## 6. ⚠ Honest limitations

- **A fresh automaton is built per query.** `new` clones the dictionary handle and allocates fresh
  registries, so `` $`m`$ `` queries against the same `` $`(D, k)`$ `` rebuild the machinery `` $`m`$ ``
  times. For high query volume against a fixed dictionary and bound, use
  [`BoundUniversalWfst`](universal-wfst.md), which builds its query-agnostic automaton once and reuses
  it across queries.
- **Eager reads need a prior `expand`.** `transitions(state)` is empty and `is_final` / `final_weight`
  return `false` / `` $`+\infty`$ `` for any state whose product components have not yet been registered
  by expanding its predecessor. Drive with `expand` / `transitions_lazy`, or use a composition wrapper
  that does.
- **Multi-character edits are two arcs.** Transposition, merge, and split split one logical edit into a
  unit-cost arc followed by a free (`` $`0`$ ``) arc through a non-final continuation state. A consumer
  that reasons by counting arcs must treat each such pair as a single edit; the intermediate
  `` $`\mathsf{T}`$ `` / `` $`\mathsf{Mg}`$ `` / `` $`\mathsf{Sp}`$ `` state never accepts.
- **The product space must fit a `u32`.** Once
  `` $`\lvert D_{\text{reg}}\rvert \cdot M_{\mathrm{lev}} \ge 2^{32}`$ `` (extreme `` $`n \cdot k`$ ``
  over a very large dictionary) encoding fails and the offending edges are *silently pruned* rather than
  erroring. Keep `` $`(n{+}1)(k{+}1)(1{+}c)`$ `` times the reachable node count below `` $`2^{32}`$ ``.
- **`max_distance` is `usize` here** — but `u8` on the universal, generalized, and phonetic variants.
  Mixing variants in one pipeline needs an explicit cast.
- **Default caching is unbounded.** `CacheAll` retains every expanded state; set
  `CachePolicy::Lru { max_states }` / `set_max_cache_size` for a memory ceiling. `set_max_cache_size`
  has no effect under `CacheAll`.

---

## 7. `DictionaryBackend<D>` — the vocabulary adapter (not a transducer)

`DictionaryBackend<D>` is the supporting adapter that lets a libdictenstein dictionary act as
lling-llang's `LatticeBackend` — the *vocabulary* layer used by lattice infrastructure
([architecture/02 §6](../architecture/02-wfst-trait-surface.md#6-latticebackend--adapting-a-dictionary)).
It is **not** a WFST and performs no edit-distance search; it interns terms to `VocabId`s (sequential
`u32` indices) and answers membership. Its bounds are looser than the WFST wrapper's — it needs no
`Unit: Into<char>`:

```rust,ignore
pub struct DictionaryBackend<D>
where D: Dictionary + Clone + Send + Sync, D::Node: Send + Sync
{ /* dictionary: D, word_to_id: FxHashMap<Arc<str>, VocabId>, id_to_word: Vec<Arc<str>> */ }

impl<D> DictionaryBackend<D> {
    pub const VOCAB_ID_EXHAUSTED: VocabId = VocabId::MAX;   // sentinel; never assigned to a word

    pub fn new(dictionary: D) -> Self;                       // empty vocab, lazy interning
    pub fn with_vocabulary<I: IntoIterator<Item = String>>(dictionary: D, terms: I) -> Self;      // pre-intern; stops at exhaustion
    pub fn try_with_vocabulary<I: IntoIterator<Item = String>>(dictionary: D, terms: I) -> Option<Self>; // None on exhaustion
    pub fn try_intern(&mut self, word: &str) -> Option<VocabId>;  // None when the u32 space is exhausted
    pub fn has_vocab_capacity(&self) -> bool;

    pub fn dictionary(&self) -> &D;
    pub fn dictionary_mut(&mut self) -> &mut D;
    pub fn into_dictionary(self) -> D;
}

// via the LatticeBackend trait (bring `lling_llang::prelude::*` into scope):
impl<D> LatticeBackend for DictionaryBackend<D> {
    fn intern(&mut self, word: &str) -> VocabId;   // infallible: returns VOCAB_ID_EXHAUSTED on exhaustion
    fn lookup(&self, id: VocabId) -> Option<&str>;
    fn vocab_size(&self) -> usize;
    fn contains(&self, word: &str) -> bool;        // true if in the intern cache OR the underlying Dictionary
    fn get_id(&self, word: &str) -> Option<VocabId>;  // consults the cache only — no auto-intern
    // iter(), supports_sharing() -> false, …
}
```

Interning is lazy and backed by an `FxHashMap<Arc<str>, VocabId>` (forward) plus a `Vec<Arc<str>>`
(reverse), so the backend clones cheaply. The last representable `VocabId` is reserved: `try_intern`
returns `None` (and the infallible `intern` returns `VOCAB_ID_EXHAUSTED`) rather than colliding with the
sentinel, so `lookup(VOCAB_ID_EXHAUSTED)` is always `None`. `contains` widens the query to the
underlying dictionary; `get_id` does not (it never mutates), which is the distinction that lets you test
membership without allocating a `VocabId`.

```rust,ignore
use duallity::DictionaryBackend;
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use lling_llang::prelude::*;

let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "world"]);
let mut backend = DictionaryBackend::new(dict);

let id_hello = backend.intern("hello");
assert_eq!(backend.lookup(id_hello), Some("hello"));
assert_eq!(backend.intern("hello"), id_hello);   // stable across calls
assert_eq!(backend.get_id("world"), None);       // not interned yet — no auto-intern
assert!(backend.contains("world"));              // …but present in the underlying dictionary
```

---

## See also

- [theory/02 · Edit distance and Levenshtein automata](../theory/02-edit-distance-and-levenshtein-automata.md) — the metric and the `` $`2k{+}1`$ `` band.
- [theory/03 · The Levenshtein automaton as a transducer](../theory/03-levenshtein-as-transducer.md) — the transition kernel this page realizes.
- [architecture/03 · State encoding and the product space](../architecture/03-state-encoding-and-product-space.md) — the `` $`\mathrm{StateId} = d \cdot M + a`$ `` scheme and its two regimes.
- [architecture/04 · Lazy evaluation and caching](../architecture/04-lazy-evaluation-and-caching.md) — expansion, cache policies, eviction.
- [design/universal-wfst](universal-wfst.md) — the query-agnostic counterpart for many-query workloads.
- [guides/01 · Quickstart](../guides/01-quickstart.md) · [guides/02 · Choosing a variant](../guides/02-choosing-a-variant.md).

## References

1. **Levenshtein, V. I.** (1966). *Binary codes capable of correcting deletions, insertions, and reversals.* Soviet Physics Doklady 10(8), pp. 707–710 — the edit distance.
2. **Wagner, R. A., & Fischer, M. J.** (1974). *The string-to-string correction problem.* Journal of the ACM 21(1), pp. 168–173. [doi:10.1145/321796.321811](https://doi.org/10.1145/321796.321811) — the edit lattice `` $`\Delta`$ ``.
3. **Schulz, K. U., & Mihov, S.** (2002). *Fast string correction with Levenshtein automata.* IJDAR 5(1), pp. 67–85. [doi:10.1007/s10032-002-0082-8](https://doi.org/10.1007/s10032-002-0082-8) — the automaton and its `` $`2k{+}1`$ `` band.
4. **Mohri, M.** (1997). *Finite-state transducers in language and speech processing.* Computational Linguistics 23(2), pp. 269–311. ACL J97-2003 — weighted transducers over a semiring.
5. **Damerau, F. J.** (1964). *A technique for computer detection and correction of spelling errors.* Communications of the ACM 7(3), pp. 171–176. [doi:10.1145/363958.363994](https://doi.org/10.1145/363958.363994) — adjacent transposition as a unit edit.
</content>
</invoke>
