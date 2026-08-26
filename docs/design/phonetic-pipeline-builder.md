# Phonetic Pipeline Builder

> **`PhoneticPipelineBuilder`**, **`PhoneticPipelineConfig`**, **`PhoneticMatch`** — one fluent,
> type-state front-end that emits any of the phonetic WFST *stages* from a single configuration. The
> rewrite stage is always available; the NFA / dictionary stages need `features = ["phonetic-rules"]`.

## 1. Intuition

Rather than constructing [`RewriteWfst`](phonetic-rewrite-wfst.md),
[`PhoneticNfaWfst`](phonetic-nfa-wfst.md), or [`PhoneticWfst`](phonetic-wfst.md) by hand — each with
its own constructor signature and weight validation — you configure **one** builder and ask it for
whichever stage you need. It advances through a **type state**: it starts as
`PhoneticPipelineBuilder<()>` (no dictionary) and becomes `PhoneticPipelineBuilder<D>` once you attach
a dictionary, which is the moment the dictionary-backed `build()` becomes callable.

The builder is an **assembler, not an engine**. It hands you WFST stages; *you* compose them and run
the shortest-path search (§6). This split is deliberate — composition and search belong to
`lling_llang`, and keeping them in the caller's hands is what lets a phonetic stage sit inside a
larger pipeline (a language model, a domain filter) instead of a closed box. §5 states this plainly,
because it is the single most common misconception about this type.

## 2. Type-state semantics

> **Notation.** `` $`\langle T \rangle`$ `` denotes the builder's type parameter `` $`D = T`$ ``;
> `` $`()`$ `` is the unit type (no dictionary attached). `` $`\varnothing`$ `` is the empty set;
> `` $`\le_{\mathrm{lex}}`$ `` is lexicographic order. Weights and costs are `f64` in the tropical
> convention (lower is better).

**The two type-states and the transition.** The builder is a two-node state machine over its type
parameter. The dictionary-attaching method rewrites the type; every other fluent method preserves it:

```math
\texttt{PhoneticPipelineBuilder}\langle()\rangle
\ \xrightarrow{\ \texttt{.dictionary::}\langle D2\rangle\ }\
\texttt{PhoneticPipelineBuilder}\langle D2\rangle .
```

The `dictionary` method **borrows** `` `&d` `` and clones it into the builder (bounds
`` $`D2 : \texttt{Dictionary} + \texttt{Clone} + \texttt{Send} + \texttt{Sync} + \texttt{'static}`$ ``,
`char` units), so the `` $`\langle D2\rangle`$ `` state owns its dictionary and can surrender it to
`build()`.

**The three exits.** Each build method is a guarded projection from the configuration to a concrete
stage:

| Exit | Produces | Requires | Feature | Reads |
|------|----------|----------|---------|-------|
| `build_rewrite_wfst()` | `RewriteWfst` | — (rules and/or identity) | **none** | `rewrite_rules`, `allow_identity` |
| `build_phonetic_nfa()` | `PhoneticNfaWfst` | a `pattern` | `phonetic-rules` | `pattern`, `phonetic_weight` |
| `build()` | `PhoneticWfst<D>` | `dictionary` **and** `pattern` | `phonetic-rules` | all scoring fields + `dictionary` |

**The pattern / rule exclusivity guard.** `build_phonetic_nfa()` and `build()` first call
`ensure_pattern_mode_only`, which **errors** when rewrite rules are present:

```math
\texttt{ensure\_pattern\_mode\_only} \;=\;
\begin{cases}
\mathsf{Ok} & \texttt{rewrite\_rules} = \varnothing,\\[2pt]
\mathsf{Err}(\text{"…use build\_rewrite\_wfst() for rewrite-rule configuration"}) & \text{otherwise.}
\end{cases}
```

So a configuration carrying **both** a pattern and rewrite rules is rejected by the NFA/dictionary
exits — this prevents silently dropping rewrite rules from a dictionary-backed build. The guard is
one-directional: `build_rewrite_wfst()` never calls it and simply **ignores any pattern**, emitting a
rewriter from the rules (or an identity-only rewriter when `rewrite_rules = ` `` $`\varnothing`$ ``).

**`PhoneticMatch` ordering.** The result type is a total order (usable in a `BinaryHeap` /
`BTreeSet`), sorted ascending by total cost then term, with `f64::total_cmp` so it stays total even
over `NaN` and `` $`\pm\infty`$ ``:

```math
m_1 \le m_2 \iff \bigl(m_1.\texttt{total\_cost},\ m_1.\texttt{term}\bigr)
\ \le_{\mathrm{lex}}\ \bigl(m_2.\texttt{total\_cost},\ m_2.\texttt{term}\bigr),
\qquad
\texttt{total\_cost} = \texttt{phonetic\_cost} + \texttt{edit\_cost}.
```

## 3. API surface (duallity 4.0.0-rc.5)

`PhoneticPipelineBuilder`, `PhoneticPipelineConfig`, and `PhoneticMatch` are exported from the crate
root with **no feature gate** (`src/composed_phonetic.rs`); the two build exits that produce
feature-gated stages are themselves `#[cfg(feature = "phonetic-rules")]`.

```rust,ignore
pub struct PhoneticPipelineConfig {
    pub pattern: Option<String>,     // None by default
    pub max_distance: u8,            // default 2
    pub phonetic_weight: f64,        // default 0.0
    pub edit_weight: f64,            // default 1.0
    pub rewrite_rules: Vec<RewriteRule>,   // default empty
    pub allow_identity: bool,        // default true
}   // implements Default

pub struct PhoneticPipelineBuilder<D = ()> { /* config, dictionary: Option<D> */ }

pub struct PhoneticMatch {
    pub term: String,
    pub total_cost: f64,             // = phonetic_cost + edit_cost
    pub phonetic_cost: f64,
    pub edit_cost: f64,              // pass the ALREADY-weighted edit component
}
impl PhoneticMatch {
    pub fn new(term: String, phonetic_cost: f64, edit_cost: f64) -> Self;   // total = sum
}
// Eq + Ord: by total_cost (f64::total_cmp) then term.
```

```rust,ignore
impl PhoneticPipelineBuilder<()> { pub fn new() -> Self; }    // also Default

impl<D> PhoneticPipelineBuilder<D> {
    pub fn phonetic_pattern(self, pattern: &str) -> Self;
    pub fn max_edit_distance(self, distance: u8) -> Self;
    pub fn phonetic_weight(self, weight: f64) -> Result<Self, InvalidWeightError>;
    pub fn edit_weight(self, weight: f64) -> Result<Self, InvalidWeightError>;
    pub fn add_rewrite_rule(self, input: &str, output: &str, cost: f64)
        -> Result<Self, InvalidWeightError>;
    pub fn add_rewrite_rules(self, rules: Vec<RewriteRule>)
        -> Result<Self, InvalidWeightError>;
    pub fn allow_identity(self, allow: bool) -> Self;
    pub fn dictionary<D2>(self, dictionary: &D2) -> PhoneticPipelineBuilder<D2>;   // type-state move
    pub fn build_rewrite_wfst(&self) -> Result<RewriteWfst, InvalidWeightError>;   // no feature
}

#[cfg(feature = "phonetic-rules")]
impl<D> PhoneticPipelineBuilder<D> {
    pub fn build_phonetic_nfa(&self) -> Result<PhoneticNfaWfst, String>;           // needs a pattern
}
#[cfg(feature = "phonetic-rules")]
impl<D: Dictionary + Clone + Send + Sync + 'static /* char units */> PhoneticPipelineBuilder<D> {
    pub fn build(&self) -> Result<PhoneticWfst<D>, String>;                        // needs dict + pattern
}
```

**Validation.** `phonetic_weight`, `edit_weight`, `add_rewrite_rule`, and `add_rewrite_rules` return
`Result<_, InvalidWeightError>` — every cost must be finite and non-negative, checked at the builder
boundary. The two feature-gated exits return `Result<_, String>` (parse/compile/guard/weight failures
flattened to a message); `build_rewrite_wfst` returns `Result<_, InvalidWeightError>`. All the fluent
setters except the two weight/rule ones are infallible and consuming (`self` → `Self`).

## 4. Scoring

The scoring knobs are stored in the config and applied by the *emitted* stages, not by the builder:

- **`phonetic_weight`** is passed to `PhoneticNfaWfst` / `PhoneticWfst` and charged on each consuming
  phonetic transition (§2 of those pages).
- **`edit_weight`** is passed to `PhoneticWfst` and scales its accepting edit-distance final weight.
- **`max_edit_distance(k)`** sets the **unweighted** edit bound `` $`0 \le k \le 255`$ ``. It
  determines the set of product states explored within `` $`k`$ ``; the weight multipliers affect
  *ranking*, not that set.

`build_rewrite_wfst` and `build_phonetic_nfa` ignore `edit_weight` and `max_distance` (a rewriter and
a bare NFA have no edit-distance stage); only `build()` consumes all four scoring fields.

## 5. ⚠ Honest limitations

This is the load-bearing caveat for the whole page; read it before using the builder.

- **The builder does not compose or search.** It returns WFST *stages*. **The caller** composes them
  with `lling_llang::composition::compose` (or wraps a `StateSource` in a `LazyWfstWrapper`) and runs
  the shortest-path search. See [theory/04 · Composition](../theory/04-composition.md) and
  [guides/03 · Composing pipelines](../guides/03-composing-pipelines.md).
- **`PhoneticMatch` is not emitted internally.** It is the result / aggregation type — it separates
  `phonetic_cost` from `edit_cost` and exposes `total_cost` for ranking — but the builder never
  populates it. The caller constructs `PhoneticMatch` values *from* search results, passing the edit
  component **after** any `edit_weight` multiplier has been applied (`new` just sums the two costs).
- **Pattern and rewrite-rule modes are mutually exclusive at the NFA / dictionary exits.**
  `build_phonetic_nfa()` and `build()` reject a config that also carries rewrite rules
  (`ensure_pattern_mode_only`); use `build_rewrite_wfst()` for rewrite-rule configurations. Conversely
  `build_rewrite_wfst()` ignores any configured pattern.
- **Do not expect ranked matches back from the builder.** It assembles stages; the ranking is the
  caller's search plus `PhoneticMatch` sorting.

## 6. Worked example

One pattern-only configuration drives all three exits, then the caller composes and ranks:

```rust,ignore
// Cargo.toml:  duallity = { version = "0.3", features = ["phonetic-rules"] }
use duallity::{PhoneticPipelineBuilder, PhoneticMatch};
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;

let dict = DynamicDawgChar::<()>::from_terms(vec!["phone", "fone", "bone"]);

// One config, three possible artifacts:
let builder = PhoneticPipelineBuilder::new()
    .phonetic_pattern("(ph|f)one")
    .max_edit_distance(2)
    .phonetic_weight(0.1).expect("valid phonetic weight")
    .edit_weight(1.5).expect("valid edit weight");

// (a) rewrite stage — always available; with no rewrite rules this is an identity-only
//     passthrough (the pattern is ignored by this exit).
let _rewrite = builder.build_rewrite_wfst().expect("valid rewrite rules");

// (b) bare phonetic NFA from the pattern (needs `phonetic-rules`).
let _nfa = builder.build_phonetic_nfa().expect("has a pattern");

// (c) attach a dictionary (type-state () -> D) and build the full phonetic WFST.
let full = builder.dictionary(&dict).build().expect("dictionary + pattern");
assert_eq!(full.max_distance(), 2);
assert_eq!(full.edit_weight(), 1.5);

// PhoneticMatch is the caller-constructed result / ranking type:
let m = PhoneticMatch::new("phone".to_string(), 0.1, 1.5);   // pass the WEIGHTED edit cost
assert_eq!(m.total_cost, 1.6);                               // 0.1 + 1.5
```

**The caller runs the pipeline.** The builder stopped at "here is a stage"; composition and search
are yours:

```rust,ignore
use duallity::{LevenshteinWfst, PhoneticMatch};
use lling_llang::composition::compose;
use lling_llang::prelude::*;

// Compose the rewrite stage in front of a Levenshtein matcher over the dictionary.
let rewrite  = builder.build_rewrite_wfst().expect("valid rewrite rules");
let lev      = LevenshteinWfst::new(&dict, "fone", 2);
let mut composed = compose(rewrite, lev);             // caller composes

// Walk shortest paths; the caller decomposes each path weight into its phonetic and
// edit components and builds a PhoneticMatch, then ranks by total_cost then term.
let mut matches: Vec<PhoneticMatch> = Vec::new();
for path in composed.accepting_paths() {              // caller searches
    let term          = path.outputs.iter().collect::<String>();
    let phonetic_cost = /* rewrite component of path.weight.value() */ 0.1;
    let edit_cost     = /* edit_weight * edit distance of the path */ 0.0;
    matches.push(PhoneticMatch::new(term, phonetic_cost, edit_cost));
}
matches.sort();                                       // ascending: total_cost, then term
```

## 7. Diagram

The builder advances from `` $`\langle()\rangle`$ `` to `` $`\langle D\rangle`$ `` and offers three
exits; the caller — not the builder — composes and searches:

<img src="../diagrams/composed-pipeline-typestate.svg" alt="PhoneticPipelineBuilder advances from the ⟨()⟩ type-state to ⟨D⟩ when a dictionary is attached; it exposes three exits (build_rewrite_wfst, build_phonetic_nfa, build) that emit WFST stages, after which the caller composes the stages and runs a shortest-path search, constructing PhoneticMatch results" width="820"/>

## See also

- [design/phonetic-rewrite-wfst](phonetic-rewrite-wfst.md), [design/phonetic-nfa-wfst](phonetic-nfa-wfst.md), [design/phonetic-wfst](phonetic-wfst.md) — the three stages this builder emits.
- [theory/04 · Composition](../theory/04-composition.md) — the `compose` fold the caller runs.
- [guides/03 · Composing pipelines](../guides/03-composing-pipelines.md) — `compose`, `LazyWfstWrapper`, shortest-path search.
- [guides/04 · Phonetic matching](../guides/04-phonetic-matching.md) — rules vs. regex, end to end.
- [architecture/02 · WFST trait surface](../architecture/02-wfst-trait-surface.md) — `Wfst` / `LazyWfst` / `StateSource`.
- Source: `src/composed_phonetic.rs`.

## References

1. **Mohri, M., Pereira, F., & Riley, M.** (2002). *Weighted Finite-State Transducers in Speech
   Recognition.* Computer Speech & Language 16(1), 69–88.
   [doi:10.1006/csla.2001.0184](https://doi.org/10.1006/csla.2001.0184) — the composition the caller
   performs on the emitted stages, and the additive cost decomposition `PhoneticMatch` records.
