# Generalized WFST

> **`GeneralizedWfst<D>`** and **`GeneralizedWfstBuilder<'a, D>`** — a runtime-configurable Levenshtein
> WFST whose **operation set** (standard, transposition, merge/split, phonetic digraphs) is chosen at
> build time and realized as a genuine lazy **product graph** with continuation states, so a
> single-`char` transducer can still emit multi-character rewrites such as $`\texttt{ph} \to \texttt{f}`$. Always available
> (no feature flag).

All shared symbols ($`q`$, $`n`$, $`k`$, $`D`$, the tropical semiring
$`\mathbb{T}`$ with $`\bar{1} = 0`$ / $`\bar{0} = +\infty`$, the transducer relation
$`T(x, y)`$, $`\varepsilon`$) are defined once in the
[master notation table](../theory/README.md#master-notation). Page-local symbols introduced here — the
product tuple $`\pi = (\nu, b, c)`$, an operation $`o = \langle x, y, \omega, \varrho\rangle`$,
and the common exact-cost scale $`s`$ — are defined at first use.

## 1. Intuition

Where [`LevenshteinWfst`](levenshtein-wfst.md) hardcodes the four standard edits, `GeneralizedWfst`
lets you **assemble the operation set at runtime** — add adjacent transpositions, OCR-style
merge/split, or phonetic digraph rewrites (`ph↔f`, `ch↔k`, `qu↔kw`) — and uses
the native operation grammar and exact cost scale while walking a lazily-interned product of the
dictionary against the query.

<img src="../diagrams/generalized-builder-flow.svg" alt="The fluent builder selects an OperationSet and builds a GeneralizedWfst backed by a lazy product graph" width="860"/>

The one structural subtlety is that the WFST's label type is a single `char`, yet a phonetic digraph
consumes/produces *two* characters on one tape. The wrapper resolves this with **continuation
states**: a multi-symbol operation emits its first aligned character pair as an ordinary weighted arc,
then threads the remaining pairs through zero-cost continuation arcs before landing on the successor
product state. The `Wfst<char, TropicalWeight>` surface is preserved exactly; the digraph simply
becomes a short chain of `char : char` arcs whose total weight is the operation's cost.

## 2. Operational semantics

### 2.1 Exact costs and state identity

Let $`s`$ be the common denominator derived by
`liblevenshtein::cost::CostScale::for_operations` from the **original** catalog.
Each configured weight $`\omega_o`$ is interpreted as its shortest round-tripping
decimal, reduced to a rational. Its scaled integer is $`a_o = s\omega_o`$;
the configured `u8` budget $`k`$ becomes $`K = sk`$. Unrepresentable denominators,
weights, or budgets are construction errors, never rounded substitutes.

This is exact decimal semantics, not exact binary-float semantics: separate
operations costing `0.1` and `0.2` sum internally to the same value as one
operation costing `0.3`. Passing the already-rounded expression `0.1 + 0.2`
as a *single configured weight* describes a different decimal and can fail
scale validation.

Two kinds of state have dense, stable IDs:

| Kind | Identity | Meaning |
|---|---|---|
| Product | $`(\nu,b,c)`$ | Dictionary path ID $`\nu`$, query UTF-8 byte offset $`b`$, and exact accumulated integer cost $`c`$. |
| Emit | Canonical label chain and position | Remaining query/input and dictionary/output scalar labels, plus the ultimate product target. |

Product equality uses integers, not floating-point bit patterns. A multi-label
chain stores its two label arrays and target once, with `Arc` sharing between
continuations. The complete chain is interned once; hashing its labels separately
for every continuation would introduce quadratic work.

### 2.2 Start state

State `0` is $`(\nu_{\mathrm{root}},0,0)`$. Both registries initially contain
only their root/start entry. A state's ID remains valid across transition-cache
eviction and across clones that share the registries.

### 2.3 Operation applicability and tape orientation

An operation consumes $`x`$ dictionary scalars and $`y`$ query scalars, as specified
by `OperationType::new(consume_x, consume_y, weight, name)`. The input tape spells
the query; the output tape spells the dictionary term. Widths count Unicode
scalar values, while query positions and listed-pair comparisons use UTF-8 bytes.

For selected dictionary and query scalar sequences $`\mathbf d`$ and $`\mathbf u`$,
the explicit `OperationApplicability` tag is authoritative:

```math
\mathrm{app}_o(\mathbf d,\mathbf u)=
\begin{cases}
\mathrm{true}, & \texttt{Any},\\
\mathbf d=\mathbf u, & \texttt{Equal},\\
d_0=u_1 \land d_1=u_0, & \texttt{AdjacentTranspose},\\
(\operatorname{bytes}(\mathbf d),\operatorname{bytes}(\mathbf u))
  \in R_o, & \texttt{Listed}(R_o).
\end{cases}
```

Here $`R_o`$ is the operation's directed substitution-pair set. Native validation
requires transpose widths to be two on each side. Names do not select behavior;
`Any` includes equal strings, and adjacent transpose accepts repeated equal
scalars. An empty listed set matches nothing.

### 2.4 Transitions and complete label chains

From $`(\nu,b,c)`$, first test $`a_o \le K-c`$ using checked integer arithmetic.
Select the next $`y`$ query scalars, ending at byte $`b'`$, and each dictionary
path of $`x`$ scalars ending at node $`\nu'`$. If the predicate holds, stage:

```math
(\nu',b',c+a_o).
```

For $`L=\max(x,y)`$, emit $`L`$ aligned input/output pairs, padding the shorter
side with epsilon. The first arc carries the original presentation weight
$`\omega_o`$; the other arcs carry zero. For example, dictionary `"ph"` and query
`"f"` produce `f:p/0.15` followed by `epsilon:h/0`.

<img src="../diagrams/product-emit-continuation.svg" alt="The ph-to-f rule emits its full cost on the first arc and finishes its labels through a zero-cost continuation" width="820"/>

All $`L-1`$ continuation identities are reserved and published with the first
arc. A state-limit failure therefore cannot leave a visible half-chain.
Query-only operations also emit their input labels explicitly.

### 2.5 Acceptance and presentation weights

A product is final exactly when its dictionary node is terminal, its query is
fully consumed, and its scaled cost is within budget:

```math
F(\nu,b,c)\iff
\operatorname{terminal}(\nu)\land b=|q|_{\mathrm{bytes}}\land c\le K,
\qquad
\rho(\nu,b,c)=0\quad\text{when final}.
```

Every continuation is non-final. An unmatched query suffix cannot be accepted
through a final-weight shortcut: that would admit paths whose input tape does
not spell the complete query. The accumulated cost is already on the arcs,
so adding it again as a final weight would double-count it.

Accepted paths spell the complete pair of strings. Their summed arc weights
approximate the configured decimal path cost in `TropicalWeight`'s `f64`
representation; acceptance and product identity do **not** use those rounded
sums. The operation set can be directional or non-metric, so arbitrary
configurations are not promised to satisfy symmetry or the triangle inequality.

### 2.6 Bounded expansion and publication

The expander first computes transaction-local paths and arcs, then publishes
dictionary IDs, product IDs, and full continuation chains together.
The [resource and transaction contract](../security/generalized-expansion-bounds.md)
defines each limit, error, fault scope, and charged work unit.

```text
Expand a registered product
  Input: fixed query, validated operations, immutable dictionary revision, limits
  Output: one complete expansion, cancellation, or explicit failure
  Invariant: no staged ID is observable before successful publication

  1. Open a computation-owned source-fault scope; check cancellation.
  2. Resolve the product and determine exact finality.
  3. Allocate metered compact width-cache slots.
  4. For each affordable operation:
       lazily fill its query slot; skip a missing query segment;
       lazily fill its dictionary-path slot with iterative bounded DFS;
       test applicability and stage matching operation arcs.
  5. Reconcile nodes; retire redundant owners outside locks and retry if needed.
  6. Keep the stable node guard and acquire the state write lock.
  7. Check retained limits and reserve complete product/continuation storage.
  8. Recheck cancellation and this computation's captured provider fault.
  9. Publish both registries using only prepared internal data.
 10. Release locks, then release staging reference counts.
 11. Return the complete expansion; cache it only on success.
```

Constructor-assigned width slots avoid an expansion-time hash map or repeated
linear searches. Equal widths reuse a slot; missing query segments and empty
dictionary path sets are cached too. User-defined node cloning, destruction,
and dictionary callbacks run outside registry locks.

## 3. Type, bounds, and the 4.0.0-rc.6 API

```rust,ignore
pub struct GeneralizedWfst<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: DictionaryNode<Unit = char>,     // NOTE: the unit must BE char (stricter than other variants)
{ /* owned dictionary, query, exact cost scale, OperationSet, prepared ops, limits, registries, cache */ }

pub struct GeneralizedWfstBuilder<'a, D>
where D: Dictionary + Clone + Send + Sync, D::Node: DictionaryNode<Unit = char>
{ /* &'a dictionary, query, max_distance: u8 (default 2), operations: OperationSet (default standard) */ }
```

`GeneralizedWfst` **owns a clone** of the dictionary (`dictionary.clone()`); the DAWG containers are
`Arc`-backed so the clone is cheap, but it is a real clone, unlike [`WallBreakerWfst`](wallbreaker-wfst.md)
which stores none.

```rust,ignore
impl<D> GeneralizedWfst<D> {
    pub fn new(dictionary: &D, query: &str, max_distance: u8, operations: OperationSet) -> Self;
    pub fn query(&self) -> &str;
    pub fn max_distance(&self) -> u8;                    // u8 (cf. usize for Levenshtein/WallBreaker)
    pub fn set_max_cache_size(&mut self, size: usize);
}

impl<'a, D> GeneralizedWfstBuilder<'a, D> {
    pub fn new(dictionary: &'a D) -> Self;               // defaults: max_distance 2, OperationSet::standard()
    pub fn query(self, query: &str) -> Self;
    pub fn max_distance(self, distance: u8) -> Self;
    pub fn with_standard_ops(self) -> Self;              // match / substitute / insert / delete
    pub fn with_transposition(self) -> Self;             // standard + adjacent swap
    pub fn with_merge_split(self) -> Self;               // standard + merge + split
    pub fn with_phonetic_digraphs(self) -> Self;         // standard + ph↔f, ch↔k, qu↔kw, … (restricted)
    pub fn with_operations(self, operations: OperationSet) -> Self;
    pub fn build(self) -> Result<GeneralizedWfst<D>, String>;   // Err("Query not set") if no query
}
```

The wrapper implements `Wfst`, `LazyWfst`, and `StateSource<char, TropicalWeight>` (and is `Clone`),
with the same dual composition surface as the other variants: `StateSource::compute_state` is the pure
`&self` path; `LazyWfst::expand` / `transitions_lazy` add the mutable transition cache.

> **Validation and bounding at construction.** `try_new` validates the original `OperationSet`
> before `bounded_operation_set` can remove anything. It then **drops any operation whose weight
> exceeds $`k`$** (it can never fire within budget) and **deduplicates operations with identical
> WFST semantics**: the same $`x`$, $`y`$, exact $`\omega`$, and complete applicability tag and
> payload. The diagnostic name is deliberately absent from semantic identity. At $`k = 0`$, for
> example, a weight-$`1`$ substitute is removed, while distinct zero-cost `Equal`, `Any`, and
> `Listed` predicates remain distinct. `new` is the convenience form that panics on an invalid
> grammar; the builder returns an error for either a missing query or an invalid operation set.

### Typed errors and explicit limits

Use `try_new_with_limits` or `GeneralizedWfstBuilder::limits(...).try_build()`
for typed `GeneralizedWfstError` construction failures. Accessors `dictionary()`,
`cost_scale()`, and `limits()` expose the fixed configuration. The older builder
`build()` retains its `Result<_, String>` return type.

For expansion, `try_transitions` and `LazyWfst::expand` return `ExpansionError`.
Exceeding a resource ceiling is a non-retryable `ResourceExhausted` failure,
not a successful empty language. `transitions_lazy` is the infallible convenience
surface and panics on failure; use the fallible methods at trust boundaries.

All defaults, a complete Rust example, and the distinction between scratch,
retained identities, and transition-cache memory are specified in
[generalized expansion bounds](../security/generalized-expansion-bounds.md).

### The operation set

`OperationSet` is a bag of `OperationType`s. Each preset (verified against
`liblevenshtein::transducer::operation_set` and `::phonetic`) contributes:

| Preset | Operation | $`\langle x_{\text{dict}},\, y_{\text{query}},\, \omega\rangle`$ | Applicability | Tape arc(s) — $`\text{query} : \text{dict} / \omega`$ |
|--------|-----------|:---:|-----------|-----------|
| `standard()` | match | $`\langle 1,1,0\rangle`$ | $`\mathbf{d}=\mathbf{u}`$ | $`u_0 : d_0 / 0`$ ($`u_0 = d_0`$) |
| `standard()` | substitute | $`\langle 1,1,1\rangle`$ | `Any`, including equality | $`u_0 : d_0 / 1`$ |
| `standard()` | insert | $`\langle 0,1,1\rangle`$ | any | $`u_0 : \varepsilon / 1`$ |
| `standard()` | delete | $`\langle 1,0,1\rangle`$ | any | $`\varepsilon : d_0 / 1`$ |
| `with_transposition()` | transpose | $`\langle 2,2,1\rangle`$ | $`d_0{=}u_1 \wedge d_1{=}u_0`$ | $`u_0 : d_0 / 1 \,\cdot\, u_1 : d_1 / 0`$ |
| `with_merge_split()` | merge | $`\langle 2,1,1\rangle`$ | any | $`u_0 : d_0 / 1 \,\cdot\, \varepsilon : d_1 / 0`$ |
| `with_merge_split()` | split | $`\langle 1,2,1\rangle`$ | any | $`u_0 : d_0 / 1 \,\cdot\, u_1 : \varepsilon / 0`$ |
| digraphs ($`2 \to 1`$) | $`\texttt{ch} \to \texttt{k}`$, $`\texttt{sh} \to \texttt{s}`$, $`\texttt{ph} \to \texttt{f}`$, $`\texttt{th} \to \texttt{t}`$ | $`\langle 2,1,0.15\rangle`$ | `Listed` | $`u_0 : d_0 / 0.15 \,\cdot\, \varepsilon : d_1 / 0`$ |
| digraphs ($`1 \to 2`$) | $`\texttt{k} \to \texttt{ch}`$, $`\texttt{s} \to \texttt{sh}`$, $`\texttt{f} \to \texttt{ph}`$, $`\texttt{t} \to \texttt{th}`$ | $`\langle 1,2,0.15\rangle`$ | `Listed` | $`u_0 : d_0 / 0.15 \,\cdot\, u_1 : \varepsilon / 0`$ |
| digraphs ($`2 \to 2`$) | $`\texttt{qu} \leftrightarrow \texttt{kw}`$ | $`\langle 2,2,0.15\rangle`$ | `Listed` | $`u_0 : d_0 / 0.15 \,\cdot\, u_1 : d_1 / 0`$ |

> **Merge vs. split arity.** Following duallity's `generalized_ops.rs`
> (`OperationType::new(2, 1, …, "merge")`, whose first argument is the **dictionary** arity), `merge`
> is $`\langle x{=}2, y{=}1\rangle`$ (two **dictionary** scalars collapse into one **query**
> scalar — the OCR reading of `"rn"` as `"m"`) and `split` is $`\langle x{=}1, y{=}2\rangle`$
> (one dictionary scalar spreads over two query scalars — `"m"` read as `"rn"`). This matches the
> `LevenshteinWfst` merge/split rows, reading $`x`$ as the dictionary side.

The taxonomy — standard vs. transposition vs. merge/split vs. restricted digraph — is diagram **D13**:

<img src="../diagrams/operationtype-taxonomy.svg" alt="The OperationType taxonomy: standard, transposition, merge/split, and phonetic digraphs, keyed by consume_x/consume_y arity" width="880"/>

## 4. Complexity and state IDs

Let $`r`$ count prepared operations, $`d_x`$ and $`d_y`$ count distinct source
and query widths, and $`V`$ count charged traversal, predicate, and label work.
The compact caches require $`O(d_x+d_y)`$ slots; rule lookup costs $`O(r)`$.
Expansion's explicit work is $`O(r+d_x+d_y+V)`$, subject to the work ledger.
Callbacks, allocator internals, and hash-table behavior have their own costs;
the work limit is not a wall-clock deadline.

Dictionary path enumeration can be exponential in operation width and branching.
Widths are not limited to two: the native aggregate-consumption ceiling is 4096.
Traversal is iterative, uses a shared prefix buffer, and checks work/path ceilings
before retaining additional results. Query slicing from a byte offset avoids
rescanning the prefix, but selecting $`y`$ scalars still costs $`O(y)`$.

`num_states_hint()` returns `None`: the old operation-count estimate was not
a valid bound for fractional costs or continuation states. `num_states()` reports
the actual shared registry length. Explicit retained-state and retained-node
limits bound successful publication; transition-cache limits do not replace them.

State IDs are dense registry offsets, not arithmetic encodings.
Products intern the exact tuple $`(\nu,b,c)`$, and canonical whole chains
supply their continuation IDs. Cache eviction changes materialized arcs, never
these identities. See the [transaction contract](../security/generalized-expansion-bounds.md)
for concurrent reconciliation and rollback.

## 5. Worked example

**Standard edits.** Construct directly with an explicit operation set and drive the start state:

```rust,ignore
use duallity::{GeneralizedWfst, GeneralizedWfstBuilder};
use liblevenshtein::transducer::OperationSet;
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use lling_llang::prelude::*;

let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help"]);

let mut g = GeneralizedWfst::new(&dict, "helo", 2, OperationSet::standard());
assert_eq!(g.query(), "helo");
g.expand(g.start()).expect("bounded start-state expansion");
```

Expanding $`q_0 = (\nu_{\mathrm{root}}, 0, 0)`$ yields, among others, the `match` arc
$`h : h / 0`$ to $`(\nu_{h}, 1, 0)`$. The minimum-weight accepting path spelling `"hello"`
threads three matches, one `delete` (the dictionary's second `l`, which the query lacks — `delete` is
$`\langle 1,0,1\rangle`$, so $`\varepsilon : l / 1`$), and a final match:

```text
 q₀ ─h:h/0─▶ (ν_h,1,0) ─e:e/0─▶ (ν_he,2,0) ─l:l/0─▶ (ν_hel,3,0) ─ε:l/1─▶ (ν_hell,3,1) ─o:o/0─▶ (ν_hello,4,1) ✔
                                                                                          ν_hello final, b=4=|helo| ⇒ final weight = 0
```

The path accumulates $`0 \otimes 0 \otimes 0 \otimes 1 \otimes 0 = 1`$ and closes with
$`\rho = 0`$, so
$`T(\texttt{helo}, \texttt{hello}) = 1 = d_{\mathrm{lev}}(\texttt{helo}, \texttt{hello})`$ — one
insertion, exactly the standard edit distance.

**Phonetic digraphs.** The builder wires `ph↔f` (and siblings) on top of the standard set:

```rust,ignore
let dict = DynamicDawgChar::<()>::from_terms(vec!["phone", "graph", "church"]);

let g2 = GeneralizedWfstBuilder::new(&dict)
    .query("fone").max_distance(2).with_phonetic_digraphs()
    .build()
    .expect("query was set");
assert_eq!(g2.max_distance(), 2);
```

For $`q = \texttt{"fone"}`$ against `"phone"`, the digraph $`\langle 2,1,0.15\rangle`$
(dictionary `"ph"`, query `"f"`) fires from $`q_0`$ as the continuation chain
$`f : p / 0.15 \,\cdot\, \varepsilon : h / 0`$, landing at $`(\nu_{ph}, 1, 3)`$ in the denominator-20 scale; three
matches (`o`, `n`, `e`) then reach the terminal `"phone"` node with the query exhausted, accepting at
$`\rho = 0`$, with total arc cost $`0.15`$. So `"fone"` is corrected to `"phone"` at cost $`0.15`$ rather than the
$`2`$ a naive two-substitution alignment would charge.

## 6. Limitations

> ⚠️ **`Unit = char` only.** The bound `D::Node: DictionaryNode<Unit = char>` is stricter than every
> other variant: byte (`u8`) dictionaries are **not** accepted, because operation arities count Unicode
> scalars and the expander slices UTF-8. Use a `char` container (`DynamicDawgChar`, `DoubleArrayTrieChar`,
> `SuffixAutomatonChar`, …). [`LevenshteinWfst`](levenshtein-wfst.md) and
> [`WallBreakerWfst`](wallbreaker-wfst.md), by contrast, accept `Unit: Into<char>` / `Into<u32>`.

> ⚠️ **Digraph weights are fixed at $`0.15`$.** `consonant_digraphs()` bakes in the cost
> $`0.15`$ for every `ch/sh/ph/th/qu` rewrite, and `with_phonetic_digraphs()` exposes no knob to
> retune it. To use different phonetic costs, build a custom `OperationSet` (via `OperationType::with_restriction`)
> and pass it through `with_operations(..)`; for fully rule-based phonetics with tunable weights, prefer
> [`RewriteWfst` / `PhoneticWfst`](phonetic-rewrite-wfst.md).

> ⚠️ **Over-budget operations vanish.** `bounded_operation_set` drops any operation with
> $`\omega > k`$ and collapses WFST-equivalent duplicates. This is correct (such operations can
> never fire within budget) but silent — an operation set that "does nothing" at a given $`k`$ is
> not an error. Raise $`k`$ (or lower the operation's weight) to make it reachable.

> ⚠️ **Continuation depth is bounded by arity.** Because the label type is a single `char`, every
> multi-symbol operation is realized as $`\max(x, y)`$ arcs through interned Emit states. With the
> shipped presets $`\max(x, y) \le 2`$, so a digraph is always exactly two arcs; a
> custom operation with larger arity produces a proportionally longer zero-cost tail, reserved
> atomically with its first arc. The maximum width is covered by a 64 KiB-stack regression test.

## 7. Diagrams

| ID | Diagram | Shows |
|----|---------|-------|
| **D13** | [`operationtype-taxonomy`](../diagrams/operationtype-taxonomy.svg) | the `OperationType` taxonomy keyed by $`\langle\texttt{consume\_x}, \texttt{consume\_y}\rangle`$ arity: standard, transposition, merge/split, restricted digraphs. |
| **D14** | [`generalized-builder-flow`](../diagrams/generalized-builder-flow.svg) | the fluent builder selecting an `OperationSet` and producing a `GeneralizedWfst` over a lazy product graph. |
| **NEW** | `product-emit-continuation` | how a multi-symbol operation ($`\texttt{ph} \to \texttt{f}`$) becomes an arc $`f{:}p/0.15`$ to an Emit continuation, then $`\varepsilon{:}h/0`$ to the target product state — the mechanism that keeps a single-`char` WFST able to emit digraphs. |

All follow the shared [color legend](../diagrams/README.md#shared-color-legend-single-source-of-truth)
(`liblevenshtein` red-pink, `libdictenstein` green, `duallity` blue; match green, substitute red,
insert blue, delete orange; $`\varepsilon`$ steps dashed light-gray; accepting states gold).

## See also

- [theory/07 · Regular-language limits](../theory/07-regular-language-limits.md) — the operation taxonomy and what it can/cannot express, in the Chomsky hierarchy.
- [design/levenshtein-wfst](levenshtein-wfst.md) — the fixed four-edit counterpart and the shared two-arc idiom for multi-symbol edits.
- [design/universal-wfst](universal-wfst.md) — compile-time metric variants (`Standard`/`Transposition`/`MergeAndSplit`) for many-query reuse.
- [design/phonetic-rewrite-wfst](phonetic-rewrite-wfst.md) — rule-based phonetic rewriting with tunable weights.
- [architecture/03 · State encoding and the product space](../architecture/03-state-encoding-and-product-space.md) — the dense-registry vs. arithmetic-radix id regimes.

## References

1. **Schulz, K. U., & Mihov, S.** (2002). *Fast String Correction with Levenshtein Automata.* IJDAR 5(1), 67–85. [doi:10.1007/s10032-002-0082-8](https://doi.org/10.1007/s10032-002-0082-8) — the automaton framework the generalized operations extend.
2. **Mihov, S., & Schulz, K. U.** (2004). *Fast Approximate Search in Large Dictionaries.* Computational Linguistics 30(4), 451–477. [doi:10.1162/0891201042544938](https://doi.org/10.1162/0891201042544938) — universal/generalized position semantics and merge/split arities.
3. **Mohri, M., Pereira, F., & Riley, M.** (2002). *Weighted finite-state transducers in speech recognition.* Computer Speech & Language 16(1), 69–88. [doi:10.1006/csla.2001.0184](https://doi.org/10.1006/csla.2001.0184) — the WFST / tropical-semiring surface the product graph composes into.
