# 04 · Phonetic matching

duallity offers two routes to sound-alike matching. This guide helps you pick and use them.

| Route | Type | Feature | When |
|-------|------|---------|------|
| **Rules** | [`RewriteWfst`](../design/phonetic-rewrite-wfst.md) | none | you have a fixed list of rewrites (`ph→f`, `ck→k`) |
| **Regex** | [`PhoneticWfst`](../design/phonetic-wfst.md) | `phonetic-rules` | you want pattern matching (`(ph|f)one`, classes) over a dictionary |

## 1. Rule-based rewriting

`RewriteWfst` applies orthography rules as char/ε transition chains
([design/phonetic-rewrite-wfst](../design/phonetic-rewrite-wfst.md)) and composes in front of a
Levenshtein matcher. The cost of a rule is paid once (on its first step); unmatched ASCII
alphanumerics pass through free. Rule costs must be finite and non-negative.

```rust,ignore
use duallity::{LevenshteinWfst, RewriteWfst, RewriteRule, CommonPhoneticRules};
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use lling_llang::composition::compose;

let dict = DynamicDawgChar::<()>::from_terms(vec!["phone", "telephone", "graph"]);

// Preset rule sets:
let english = RewriteWfst::with_rules(CommonPhoneticRules::english())
    .expect("valid English rules");
let _german = RewriteWfst::with_rules(CommonPhoneticRules::german())
    .expect("valid German rules");
let _french = RewriteWfst::with_rules(CommonPhoneticRules::french())
    .expect("valid French rules");

// Or a custom set with explicit costs / priorities:
let mut custom = RewriteWfst::new();
custom.add_rule("ph", "f", 0.1).expect("valid rewrite rule");
custom
    .add_rewrite_rule(
        RewriteRule::with_cost("ck", "k", 0.1)
            .expect("valid rewrite rule")
            .with_priority(5),
    )
    .expect("valid rewrite rule");

let lev = LevenshteinWfst::new(&dict, "fone", 2);
let _matcher = compose(english, lev);
```

### The preset rule sets

| Set | Rules (input → output, cost) |
|-----|------------------------------|
| `english()` | `ph→f (0.1)`, `gh→f (0.2)`, `ck→k (0.1)`, `qu→kw (0.1)`, `x→ks (0.1)`, `c→k (0.2)`, `c→s (0.2)` |
| `german()` | `sch→sh (0.1)`, `ch→x (0.1)`, `ß→ss (0.1)`, `ä→ae (0.1)`, `ö→oe (0.1)`, `ü→ue (0.1)` |
| `french()` | `eau→o (0.1)`, `aux→o (0.1)`, `ai→e (0.1)`, `ph→f (0.1)`, `qu→k (0.1)` |

> ⚠️ Rules apply **unconditionally**, ordered by descending priority. Represent left/right context
> or word-boundary conditioning by expanding it into explicit consumed-context rules
> ([design/phonetic-rewrite-wfst §4](../design/phonetic-rewrite-wfst.md#4-context-model)).
> So both `c→k` and `c→s` fire; disambiguate with `priority` if you need one to win.

## 2. Regex-based matching

With `features = ["phonetic-rules"]`, compile a phonetic regular expression into an NFA-backed WFST
fused with the dictionary ([design/phonetic-wfst](../design/phonetic-wfst.md)):

```rust,ignore
// Cargo.toml:  duallity = { version = "0.3", features = ["phonetic-rules"] }
use duallity::PhoneticWfstBuilder;
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;

let dict = DynamicDawgChar::<()>::from_terms(vec!["phone", "fone", "bone"]);
let _wfst = PhoneticWfstBuilder::new(dict, 2)
    .phonetic_weight(0.1)                       // cost of an exact phonetic step
    .expect("valid phonetic weight")
    .build_from_pattern("(ph|f)one")            // alternation, grouping, classes, optionality
    .expect("valid pattern");
```

Wide labels (`.` and negated or large character classes) are exact over the configured finite alphabet
([design/phonetic-nfa-wfst §4](../design/phonetic-nfa-wfst.md#4-alphabet-contract-for-wide-labels)).

## 3. One builder for both

[`PhoneticPipelineBuilder`](../design/phonetic-pipeline-builder.md) emits whichever stage you need from
one configuration (`build_rewrite_wfst`, `build_phonetic_nfa`, `build`). `phonetic_weight` scores
consuming phonetic transitions, while `edit_weight` scales the accepting edit-distance final weight
for dictionary-backed `build()`. Both weights must be finite and non-negative. Remember it produces
*stages* — you compose and search them yourself ([guides/03](03-composing-pipelines.md)).

## See also

- [design/phonetic-rewrite-wfst](../design/phonetic-rewrite-wfst.md), [design/phonetic-wfst](../design/phonetic-wfst.md)
- [theory/07 · Regular-language limits](../theory/07-regular-language-limits.md)
