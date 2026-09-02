# Glossary

Every term, symbol, and acronym used in duallity's documentation, defined alphabetically. Symbols are
rendered exactly as in the [theory notation table](../theory/README.md#master-notation), which is the
single source of truth; this glossary is the prose mirror.

| Term | Definition |
|------|------------|
| **ABI (Application Binary Interface)** | The compiled-artifact contract — struct layout, calling convention, and symbol set — that lets independently built libraries interoperate without shared source. duallity's stable C ABI is the eight `duallity_*` functions ([architecture/06](../architecture/06-resource-abi-and-bindings.md)). |
| **acceptance / accepting state** | A WFST state that ends a valid input/output relation and carries a *final weight*. For a Levenshtein WFST, a state $`(d, (i, e))`$ is accepting iff the dictionary node $`d`$ is final and $`e + (n - i) \le k`$. |
| **alphabet ($`\Sigma`$)** | The set of symbols. duallity works per Unicode scalar (`char`), so $`\Sigma \subseteq \texttt{char}`$. |
| **arctic semiring** | The max-plus semiring $`(\mathbb{R} \cup \{-\infty\},\ \max,\ +,\ -\infty,\ 0)`$; the fzf kind scores in it (higher is better) and its resource advertises `VtWeightDomain::ArcticF64`, in contrast to the tropical (min-plus) kinds. |
| **automaton state ($`a`$)** | The non-dictionary component of a product state — a query position, a universal-automaton state, or a product-automaton state. |
| **bidirectional node** | A dictionary node exposing `parent()`/`parent_label()` (toward the root) as well as `edges()` (toward leaves); required by WallBreaker for extension. |
| **cache policy** | `CacheAll` / `Lru { max_states }` / `NoCache` — the memoization strategy for computed states, applied to duallity's `LazyStateCache`. |
| **capture-once rule** | duallity reads a foreign dictionary exactly once, at construction, into an immutable snapshot revision that may outlive the source handle ([architecture/06 §5](../architecture/06-resource-abi-and-bindings.md#5-the-capture-once-rule); modeled in `proofs/tla/SnapshotCaptureOnce.tla`). |
| **characteristic vector ($`\chi(c, s)`$)** | A bit vector over a window $`s`$ whose $`j`$-th bit is $`1`$ iff $`s_j = c`$; how a dictionary character enters the universal automaton. |
| **Chomsky hierarchy** | The containment $`\text{regular} \subsetneq \text{context-free} \subsetneq \text{context-sensitive} \subsetneq \text{recursively enumerable}`$; duallity's WFSTs are strictly **regular** (Type 3). |
| **composition ($`T_1 \circ T_2`$)** | Chaining transducers by matching $`T_1`$'s output tape against $`T_2`$'s input tape: $`(T_1 \circ T_2)(x, z) = \min_y\,[\,T_1(x, y) + T_2(y, z)\,]`$ in the tropical semiring. |
| **continuation state** | An intermediate state (in `RewriteWfst` and the Generalized/Levenshtein edit variants) that encodes a multi-symbol operation as a zero-cost char/$`\varepsilon`$ chain after a first cost-bearing arc. |
| **DAWG** | Directed Acyclic Word Graph — a compressed dictionary automaton; `DynamicDawgChar` is duallity's general-purpose backend. |
| **Damerau–Levenshtein distance ($`d_{\mathrm{DL}}`$)** | Edit distance with adjacent **transposition** as a unit-cost operation, in addition to insert/delete/substitute. |
| **dictionary ($`D`$)** | A libdictenstein container of terms that a WFST walks. |
| **dictionary node ($`d`$)** | A position in the dictionary's trie/DAWG; the dictionary component of a product state. |
| **double adapter** | duallity's bridge implementing *both* libdictenstein's `Dictionary`/`DictionaryNode` (over a foreign `vt.dictionary.v1` resource) *and* lling-llang's `ScalarWfstProvider` (re-exposing the product as a `vt.scalar-wfst.1` resource) ([architecture/06 §6](../architecture/06-resource-abi-and-bindings.md#6-the-double-adapter-bridge)). |
| **edit distance / Levenshtein distance ($`d_{\mathrm{lev}}`$)** | The minimum number of insert/delete/substitute edits transforming one string into another. |
| **edit lattice ($`\Delta`$)** | The $`(\text{query position},\ \text{term position})`$ grid whose minimum-cost path equals the edit distance; $`\Delta[i, j] = d_{\mathrm{lev}}(q[0 \mathbin{..} i],\, w[0 \mathbin{..} j])`$. |
| **epsilon ($`\varepsilon`$)** | The empty label on a tape — consumes/produces nothing (insert has $`\varepsilon`$ input, delete has $`\varepsilon`$ output). |
| **final weight** | The tropical weight an accepting state contributes; for a Levenshtein WFST, the cost of deleting the unconsumed query tail, $`\mathrm{rem} = n - i`$. |
| **FST / WFST** | (Weighted) Finite-State Transducer — an automaton whose transitions carry $`\text{input} : \text{output}\ (/\ \text{weight})`$. |
| **idempotent semiring** | A semiring in which $`a \oplus a = a`$. The tropical $`\oplus = \min`$ is idempotent; this (with monotonicity) is what makes single-source shortest-path search over a lazy WFST correct. |
| **`LatticeBackend`** | The lling-llang trait `DictionaryBackend` implements: a vocabulary adapter mapping terms ↔ $`\texttt{VocabId}`$. |
| **`LazyStateCache`** | duallity's own per-WFST state cache (`src/lazy_cache.rs`): an `FxHashMap` of computed states, a $`\texttt{BinaryHeap}\langle \texttt{Reverse}(\text{clock}, \texttt{StateId}) \rangle`$ deterministic-LRU min-heap, and a `scratch` slot for `NoCache` / `Lru{0}`. |
| **`LazyWfst`** | The mutable lazy interface (`expand`, `transitions_lazy`); computes and caches states on touch. |
| **`max_automaton_states` ($`M`$)** | The radix of the state encoding: $`\mathrm{StateId} = d \cdot M + a`$. For the parameterized standard path, $`M_{\mathrm{lev}} = (n{+}1)(k{+}1)(1{+}c)`$ with $`c`$ enabled continuation classes. |
| **`max_distance` ($`k`$)** | The maximum edit distance / error bound. |
| **Myhill–Nerode theorem** | A language is regular iff its right-congruence $`x \equiv y \iff (\forall z)\ (xz \in L \leftrightarrow yz \in L)`$ has finitely many classes; used in chapter 07 to prove $`\{a^{n} b^{n}\}`$ is not regular. |
| **NFA** | Nondeterministic Finite Automaton; a compiled phonetic regex (`NFAChar`) that `PhoneticNfaWfst` runs by subset construction. |
| **pigeonhole split** | Cutting a query into $`k{+}1`$ (Standard) or $`2k{+}1`$ (Transposition / MergeAndSplit) pieces so at least one is uncorrupted under $`k`$ edits — the basis of WallBreaker. |
| **poison recovery** | Recovering a $`\texttt{RwLock}`$ guard after a panic poisoned it (via `into_inner` on the `PoisonError`), so a panicked thread cannot wedge the registries. |
| **`PositionVariant` ($`V`$)** | The universal-automaton metric: $`\textsf{Standard}`$, $`\textsf{Transposition}`$, or $`\textsf{MergeAndSplit}`$. |
| **product state** | A WFST state $`(d, a)`$ (dictionary node, automaton state); either arithmetically packed into a $`\texttt{StateId}`$ (Levenshtein path) or assigned a dense id by a registry (other variants). |
| **pumping lemma (regular)** | Every regular $`L`$ has a length $`p`$ such that any $`s \in L`$ with $`\lvert s \rvert \ge p`$ factors as $`s = xyz`$, $`\lvert xy \rvert \le p`$, $`\lvert y \rvert \ge 1`$, with $`xy^{i}z \in L`$ for all $`i \ge 0`$; used to show non-regularity. |
| **RAII (Resource Acquisition Is Initialization)** | The C++ idiom binding a resource's lifetime to a scope-bound object; the `duallity.hpp` `wfst` and `resource` types free/release in their destructors. |
| **rational (regular) relation** | A relation $`R \subseteq \Sigma^{\ast} \times \Sigma^{\ast}`$ realizable by a finite-state transducer; the exact expressive class of every duallity WFST. |
| **readers–writer lock** | A lock ($`\texttt{RwLock}`$) admitting many concurrent readers **xor** one writer; duallity wraps its registries in $`\texttt{Arc}\langle \texttt{RwLock}\langle \cdot \rangle \rangle`$. |
| **relevant subword ($`s_n(w, i)`$)** | The window $`w[\max(i-n, 1) \mathbin{..} \min(\lvert w \rvert, i+n+1)]`$, padded with the `$` sentinel, that the universal automaton inspects at position $`i`$. |
| **retain / release** | The reference-count operations on a `VtResource`: a non-null resource owns one retain; `retain` adds one, `release` drops one, and the provider frees the resource at zero. Copying the two words does **not** retain. |
| **SCDAWG** | Symmetric Compact Directed Acyclic Word Graph — supports substring search; required by WallBreaker. |
| **semiring ($`\mathbb{K}`$)** | The weight algebra $`(K, \oplus, \otimes, \bar{0}, \bar{1})`$. duallity uses the **tropical** semiring. |
| **`StateId`** | A `u32` identifying a WFST state (an encoded product state). |
| **`StateSource`** | The immutable computation kernel (`compute_state(&self, …)`); the pure path `compose` uses. |
| **subset / powerset construction** | Building a DFA-like machine from an NFA by tracking sets of NFA states; how `PhoneticNfaWfst` exposes an NFA as a WFST. |
| **tropical semiring ($`\mathbb{T}`$)** | $`(\mathbb{R} \cup \{+\infty\},\ \min,\ +,\ +\infty,\ 0)`$: $`\oplus = \min`$, $`\otimes = +`$, $`\bar{0} = +\infty`$ (`zero()`), $`\bar{1} = 0`$ (`one()`). |
| **`TropicalWeight`** | lling-llang's tropical weight type; `zero()` is $`+\infty`$, `one()` is $`0`$ (algebraic-role naming). |
| **universal automaton ($`U_k`$)** | A query-agnostic Levenshtein automaton, built once per $`k`$ and reused across queries via characteristic vectors. |
| **`VocabId`** | A `u32` vocabulary identifier in lling-llang's lattice backend. |
| **`VtResource`** | The two-word `(context, vtable)` reference-counted handle exchanged across the vinary-tree resource ABI ([`vinary-tree-interop`](https://github.com/vinary-tree/liblevenshtein-rust/tree/master/vinary-tree-interop)); the unit that crosses the C ABI. |
| **`vt.dictionary.v1`** | The versioned interop interface a *foreign* dictionary provider implements and duallity consumes as input. |
| **`vt.scalar-wfst.1`** | The versioned interop interface duallity's *output* resource implements, so lling-llang can compose it. |
| **wall effect** | The combinatorial barrier at large $`k`$: the first $`k`$ characters cannot prune, so all short prefixes stay live. WallBreaker overcomes it. |
| **`WeightedTransition`** | An arc $`\{\, \text{from},\ \text{input} : \texttt{Option}\langle\texttt{char}\rangle,\ \text{output} : \texttt{Option}\langle\texttt{char}\rangle,\ \text{target},\ \text{weight} \,\}`$. |

## See also

- [theory/README · Master notation](../theory/README.md#master-notation) — the symbol table this mirrors.
- [references/bibliography](bibliography.md) — the cited works.
