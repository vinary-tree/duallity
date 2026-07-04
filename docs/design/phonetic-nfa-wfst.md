# Phonetic NFA WFST

> **`PhoneticNfaWfst`** — a phonetic **regular-expression** NFA presented as a WFST by lazy subset
> construction. **Requires `features = ["phonetic-rules"]`.**

## 1. Intuition

Where [`RewriteWfst`](phonetic-rewrite-wfst.md) applies fixed rules, `PhoneticNfaWfst` matches a
**pattern** — `(ph|f)one`, character classes, optionality — by wrapping a compiled `NFAChar` and
exposing it as a deterministic-on-the-fly `Wfst<char, TropicalWeight>`. It is the bare-NFA stage; to
match against a dictionary, use [`PhoneticWfst`](phonetic-wfst.md).

<img src="../diagrams/phonetic-regex-nfa-product.svg" alt="Phonetic regex compiles to an NFA; PhoneticNfaWfst exposes it by subset construction" width="860"/>

## 2. Type

```rust,ignore
#[cfg(feature = "phonetic-rules")]
pub struct PhoneticNfaWfst {
    nfa: NFAChar,                 // liblevenshtein::phonetic::nfa::NFAChar
    phonetic_weight: f64,         // cost charged per NFA transition
    alphabet: Arc<[char]>,        // finite alphabet for wide labels (`.` and classes)
    /* shared StateSet ⇄ StateId registry, cache, … */
}
```

```rust,ignore
pub fn new(nfa: NFAChar) -> Self;                    // phonetic_weight = 0.0, printable ASCII alphabet
pub fn with_phonetic_weight(nfa: NFAChar, phonetic_weight: f64) -> Result<Self, InvalidWeightError>;
pub fn with_alphabet<I: IntoIterator<Item = char>>(nfa: NFAChar, alphabet: I) -> Self;
pub fn with_phonetic_weight_and_alphabet<I: IntoIterator<Item = char>>(
    nfa: NFAChar,
    phonetic_weight: f64,
    alphabet: I,
) -> Result<Self, InvalidWeightError>;
pub fn phonetic_weight(&self) -> f64;
pub fn alphabet(&self) -> &[char];
pub fn set_max_cache_size(&mut self, size: usize);
```

`with_phonetic_weight` seeds the registry with the **epsilon-closure of the NFA start state** as state
id 0.

## 3. Semantics — lazy subset (powerset) construction

`PhoneticNfaWfst` is a DFA-on-the-fly over the NFA. For a state (a set of NFA states):

1. gather the input-consuming transition labels as concrete `char` symbols:
   - `Char(c)` contributes exactly `c`;
   - positive `CharClass` labels contribute every configured alphabet symbol they match, plus every
     symbol from small explicit ranges;
   - negated `CharClass` labels and `Any` labels contribute matching symbols from the configured
     finite alphabet;
2. for each candidate character `c`, take the union of NFA successors, then its **epsilon closure**,
   intern that set as a new state id, and emit an **identity** transition `c : c` with weight
   `phonetic_weight`;
3. a state is **final** iff any NFA state in the set is final (final weight `0`, else `+∞`).

It implements `Wfst`, `LazyWfst`, and a **functional** `StateSource` (the `314f285` fix made
`compute_state` agree with lazy expansion via a shared `Arc<RwLock<NfaStateRegistry>>`, pinned by
`test_phonetic_nfa_wfst_statesource_matches_lazy_expansion`).

## 4. Alphabet contract for wide labels

`lling_llang::Wfst` transitions carry concrete labels, so a regular-expression label such as `.`
cannot mean "all Unicode scalar values" without generating an infinite transition set. duallity
therefore treats the NFA WFST as exact over a **finite alphabet** `Σ`.

The default `Σ` is printable ASCII (`' '` through `'~'`), which covers the broad classes used by most
phonetic rules while keeping each lazy expansion bounded. Use `with_alphabet` or
`with_phonetic_weight_and_alphabet` when the domain is different, for example `['a', 'e', 'i', 'o',
'u', 'é']` for a vowel-focused stage. Duplicate alphabet symbols are ignored after their first
occurrence, preserving deterministic transition order.

Positive character classes also enumerate explicit ranges of at most 256 scalar positions, so
`[a-c]` is exact even with an empty custom alphabet. Negated classes and `.` remain alphabet-relative
because their mathematical denotation is otherwise unbounded.

## 5. Example

```rust,ignore
// Cargo.toml:  duallity = { version = "0.3", features = ["phonetic-rules"] }
use duallity::PhoneticNfaWfst;
use liblevenshtein::phonetic::{nfa::compile, regex::parse};
use lling_llang::prelude::*;

let ast = parse("(ph|f)one").expect("valid pattern");
let nfa = compile(&ast).expect("compiles");

let mut wfst = PhoneticNfaWfst::with_phonetic_weight(nfa, 0.1)
    .expect("valid phonetic weight");
let s0 = wfst.start();
wfst.expand(s0);
// From the start, both 'p' (the ph branch) and 'f' branch are available.
assert!(wfst.is_expanded(s0));
```

## See also

- [theory/07 · Regular-language limits](../theory/07-regular-language-limits.md) (regex = regular)
- [design/phonetic-wfst](phonetic-wfst.md) (NFA × Levenshtein × Dictionary)
- [design/phonetic-pipeline-builder](phonetic-pipeline-builder.md) (`build_phonetic_nfa`)
