# Phonetic Rewrite WFST

> **`RewriteWfst`**, **`RewriteRule`**, **`CommonPhoneticRules`** — a rule-based phonetic rewriter
> (`ph→f`, `ck→k`, …) as a composable transducer. **Always available — no feature flag.**

## 1. Intuition

A `RewriteWfst` applies orthography-normalizing rewrite rules so that, after composition with a
[Levenshtein WFST](levenshtein-wfst.md), `"fone"` can match `"phone"` cheaply. Each rule maps an input
substring to an output substring at a cost; unmatched alphanumerics pass through for free.

## 2. Types

```rust,ignore
pub struct RewriteRule {
    pub input: String,    // characters to match (query side)
    pub output: String,   // replacement characters (dictionary side)
    pub cost: f64,        // tropical cost of applying the rule
    pub priority: i32,    // higher = tried first when several rules match
}

pub struct RewriteWfst { /* rules, continuation_states, cache, allow_identity, … */ }
pub struct CommonPhoneticRules;   // a namespace of preset rule sets
```

```rust,ignore
impl RewriteRule {
    pub fn new(input: &str, output: &str) -> Self;                 // cost 0.0, priority 0
    pub fn with_cost(input: &str, output: &str, cost: f64) -> Result<Self, InvalidWeightError>;
    pub fn with_priority(self, priority: i32) -> Self;
}
impl RewriteWfst {
    pub fn new() -> Self;                                          // empty, allow_identity = true
    pub fn with_rules(rules: Vec<RewriteRule>) -> Result<Self, InvalidWeightError>;
    pub fn add_rule(&mut self, input: &str, output: &str, cost: f64) -> Result<(), InvalidWeightError>;
    pub fn add_rewrite_rule(&mut self, rule: RewriteRule) -> Result<(), InvalidWeightError>;
    pub fn set_allow_identity(&mut self, allow: bool);
    pub fn num_rules(&self) -> usize;
}
```

`CommonPhoneticRules::{english, german, french}()` each return a `Vec<RewriteRule>`. The English set,
for instance, is `ph→f (0.1)`, `gh→f (0.2)`, `ck→k (0.1)`, `qu→kw (0.1)`, `x→ks (0.1)`, `c→k (0.2)`,
`c→s (0.2)`.

Rule costs must be finite, non-negative `f64` values. `RewriteRule::with_cost`,
`RewriteWfst::with_rules`, `RewriteWfst::add_rule`, and `RewriteWfst::add_rewrite_rule` return
`InvalidWeightError` before a rule can emit invalid `TropicalWeight` transitions.

## 3. Semantics — char/ε chains (the `314f285` fix)

State `0` is the **home / accepting** state. A rule with `steps = max(|input|, |output|)` symbols is
encoded as a chain through `steps − 1` intermediate **continuation states**, emitting one
`input : output` pair per step, with the **whole rule cost deposited on the first step** and
continuations free (`weight = 0`):

<img src="../diagrams/rewrite-char-epsilon-chains.svg" alt="ph→f and f→ph encoded as char/epsilon chains on both tapes; cost on the first step" width="820"/>

| Rule | Step 0 | Step 1 |
|------|--------|--------|
| `ph → f` (many-to-one) | `p : f / 0.1` → s₁ | `h : ε / 0` → 0 |
| `f → ph` (one-to-many) | `f : p / 0.1` → s₁ | `ε : h / 0` → 0 |
| `c → s` (one-to-one) | `c : s / 0.2` → 0 | — |

The many-to-one chain shortens the input tape (a trailing `h : ε`); the one-to-many chain lengthens
the output tape (a trailing `ε : h`). These two shapes are pinned by
`test_rewrite_wfst_many_to_one_input_chain` and `test_rewrite_wfst_one_to_many_output_chain`. When
`allow_identity` is set (the default), state 0 also carries identity self-loops `c : c / 0` for every
ASCII alphanumeric, so un-rewritten characters pass through at zero cost.

`RewriteWfst` implements `Wfst`, `LazyWfst`, and a fully functional `StateSource` (it can be driven
through either path). `num_states() = 1 + continuation_states`.

## 4. Context model

`RewriteRule` stores only `input`/`output`/`cost`/`priority`, so rules apply **unconditionally**.
Priority controls ordering when several rules are available from state `0`; equal-priority rules
retain insertion order. The English `c→k` and `c→s` presets are therefore coarse alternatives, not
lookahead-conditioned rules.

Model left/right context or word-boundary conditioning by expanding those constraints into explicit
unconditional rules before constructing `RewriteWfst`. For example, a right-context rule such as
`c→s` before `e` can be represented as a consumed-context rule `ce→se` when the context character
should pass through unchanged.

## 5. Example

```rust,ignore
use duallity::{LevenshteinWfst, RewriteWfst, CommonPhoneticRules};
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use lling_llang::composition::compose;

let dict = DynamicDawgChar::<()>::from_terms(vec!["phone", "graph", "telephone"]);

// "fone" → rewrite (f↔ph at cost 0.1) → Levenshtein(2) over the dictionary.
let rewrite = RewriteWfst::with_rules(CommonPhoneticRules::english())
    .expect("valid preset rules");
let lev     = LevenshteinWfst::new(&dict, "fone", 2);
let _phonetic_fuzzy = compose(rewrite, lev);   // RewriteWfst ∘ LevenshteinWfst

// Or build a custom rule set:
let mut r = RewriteWfst::new();
r.add_rule("ph", "f", 0.1).expect("valid rewrite rule");
assert_eq!(r.num_rules(), 1);
```

## See also

- [theory/03 · ε on one tape](../theory/03-levenshtein-as-transducer.md) (the same convention)
- [design/phonetic-wfst](phonetic-wfst.md) (regex → NFA phonetic matching)
- [design/phonetic-pipeline-builder](phonetic-pipeline-builder.md) (`build_rewrite_wfst`)
- [guides/04 · Phonetic matching](../guides/04-phonetic-matching.md)
