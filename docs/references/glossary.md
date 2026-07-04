# Glossary

Every term, symbol, and acronym used in duallity's documentation, defined alphabetically. Symbols are
cross-referenced to the [theory notation table](../theory/README.md#master-notation).

| Term | Definition |
|------|------------|
| **acceptance / accepting state** | A WFST state that ends a valid input/output relation; carries a *final weight*. For a Levenshtein WFST: `dict_node.is_final() ∧ (n − pos) ≤ k`. |
| **alphabet (`Σ`)** | The set of symbols. duallity works per Unicode scalar (`char`). |
| **automaton state (`a`)** | The non-dictionary component of a product state — a query position, a universal-automaton state, or a product-automaton state. |
| **bidirectional node** | A dictionary node exposing `parent()`/`parent_label()` (toward the root) as well as `edges()` (toward leaves); required by WallBreaker for extension. |
| **cache policy** | `CacheAll` / `Lru { max_states }` / `NoCache` — the memoization strategy for computed states. |
| **characteristic vector (`χ(c, s)`)** | A bit vector over a window `s` whose `j`-th bit is 1 iff `s[j] = c`; how a dictionary character enters the universal automaton. |
| **composition (`T₁ ∘ T₂`)** | Chaining transducers by matching `T₁`'s output tape against `T₂`'s input tape: `(T₁∘T₂)(x,z) = min_y[T₁(x,y) + T₂(y,z)]`. |
| **continuation state** | An intermediate `RewriteWfst` state used to encode a multi-symbol rule as a char/ε chain. |
| **DAWG** | Directed Acyclic Word Graph — a compressed dictionary automaton; `DynamicDawgChar` is duallity's general-purpose backend. |
| **Damerau–Levenshtein distance (`dₜ`)** | Edit distance with adjacent **transposition** as a unit-cost operation, in addition to insert/delete/substitute. |
| **dictionary (`D`)** | A libdictenstein container of terms that a WFST walks. |
| **dictionary node (`d`)** | A position in the dictionary's trie/DAWG; the dictionary component of a product state. |
| **edit distance / Levenshtein distance (`dₗₑᵥ`)** | The minimum number of insert/delete/substitute edits transforming one string into another. |
| **edit lattice** | The `(query position, term position)` grid whose minimum-cost path equals the edit distance. |
| **epsilon (`ε`)** | The empty label on a tape — consumes/produces nothing (insert has `ε` input, delete has `ε` output). |
| **final weight** | The tropical weight an accepting state contributes; for a Levenshtein WFST, the cost of deleting the unconsumed query tail (`n − pos`). |
| **FST / WFST** | (Weighted) Finite-State Transducer — an automaton whose transitions carry `input : output (/ weight)`. |
| **`LatticeBackend`** | The lling-llang trait `DictionaryBackend` implements: a vocabulary adapter mapping terms ↔ `VocabId`. |
| **`LazyWfst`** | The mutable lazy interface (`expand`, `transitions_lazy`); computes and caches states on touch. |
| **`max_automaton_states` (`M`)** | The radix of the state encoding: `StateId = d·M + a`. For the parameterized standard path, `(n+1)·(k+1)`; algorithms with continuation states multiply that normal lattice by the enabled continuation classes. |
| **`max_distance` (`k`)** | The maximum edit distance / error bound. |
| **NFA** | Nondeterministic Finite Automaton; a compiled phonetic regex (`NFAChar`) that `PhoneticNfaWfst` runs by subset construction. |
| **pigeonhole split** | Cutting a query into `k+1` (or `2k+1`) pieces so at least one is uncorrupted under `k` edits — the basis of WallBreaker. |
| **`PositionVariant` (`V`)** | The universal-automaton metric: `Standard`, `Transposition`, or `MergeAndSplit`. |
| **product state** | A WFST state `(dict_node, automaton_state)`, packed into a single `StateId`. |
| **relevant subword (`s_n(w, i)`)** | The window `w[i−n .. min(|w|, i+n+1)]` (padded with `$`) the universal automaton inspects at position `i`. |
| **SCDAWG** | Symmetric Compact Directed Acyclic Word Graph — supports substring search; required by WallBreaker. |
| **semiring (`𝕂`)** | The weight algebra `(K, ⊕, ⊗, 0̄, 1̄)`. duallity uses the **tropical** semiring. |
| **`StateId`** | A `u32` identifying a WFST state (an encoded product state). |
| **`StateSource`** | The immutable computation kernel (`compute_state(&self, …)`); the path `compose` uses. |
| **subset / powerset construction** | Building a DFA-like machine from an NFA by tracking sets of NFA states; how `PhoneticNfaWfst` exposes an NFA as a WFST. |
| **tropical semiring** | `(ℝ ∪ {+∞}, min, +, +∞, 0)`: `⊕ = min`, `⊗ = +`, `0̄ = +∞` (`zero()`), `1̄ = 0` (`one()`). |
| **`TropicalWeight`** | lling-llang's tropical weight type; `zero()` is `+∞`, `one()` is `0` (algebraic-role naming). |
| **universal automaton** | A query-agnostic Levenshtein automaton, built once per `max_distance` and reused across queries. |
| **`VocabId`** | A `u32` vocabulary identifier in lling-llang's lattice backend. |
| **wall effect** | The combinatorial barrier at large `k`: the first `k` characters cannot prune, so all short prefixes stay live. WallBreaker overcomes it. |
| **`WeightedTransition`** | An arc `{ from, input: Option<char>, output: Option<char>, target, weight }`. |

## See also

- [theory/README · Master notation](../theory/README.md#master-notation) — the symbol table.
- [references/bibliography](bibliography.md) — the cited works.
