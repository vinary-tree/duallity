# Phonetic NFA WFST

> **`PhoneticNfaWfst`** — a phonetic **regular-expression** NFA presented as a
> `Wfst<char, TropicalWeight>` by lazy subset construction. **Requires
> `features = ["phonetic-rules"]`.**

## 1. Intuition

Where [`RewriteWfst`](phonetic-rewrite-wfst.md) applies a fixed list of rules, `PhoneticNfaWfst`
matches a **pattern** — alternation `(ph|f)one`, character classes `[aeiou]`, optionality `a?`,
repetition `a*` — by wrapping a compiled phonetic NFA and exposing it as a deterministic-on-the-fly
transducer. Each consumed character is an **identity** transition (input equals output) charged a
single `phonetic_weight`, so composing this stage in front of a Levenshtein matcher lets a spelling
be *recognized* by the pattern while edits are scored downstream.

It is the **bare-NFA stage**: it carries no dictionary and no edit distance. To match a *pattern*
against a *dictionary* within `` $`k`$ `` edits — the usual end-to-end phonetic query — use
[`PhoneticWfst`](phonetic-wfst.md), which fuses this NFA with a Levenshtein automaton and a
dictionary. `PhoneticNfaWfst` is the right tool when you want the pattern transducer *itself* — to
compose by hand, to inspect, or to stack in a custom pipeline.

## 2. Operational semantics

> **Notation.** From the [master notation table](../theory/README.md#master-notation):
> `` $`Q`$ `` (state set), `` $`q_0`$ `` (initial state), `` $`F`$ `` (final states), `` $`\rho`$ ``
> (final weight), `` $`\varepsilon`$ `` (empty tape label), `` $`\Sigma`$ `` (a finite alphabet),
> `` $`\bar{0} = +\infty`$ ``, `` $`\bar{1} = 0`$ ``, and the edge notation
> `` $`\text{in}:\text{out}/w`$ ``. Weights live in the tropical semiring `` $`\mathbb{T}`$ ``. Let
> the wrapped NFA be `` $`N = (Q_N, \Sigma_N, E_N, q_N^{0}, F_N)`$ `` with epsilon and zero-width
> anchor labels; write `` $`\omega`$ `` for the configured `phonetic_weight`.

`PhoneticNfaWfst` realizes the classical **subset (powerset) construction** ([theory/07 ·
Regular-language limits](../theory/07-regular-language-limits.md); Rabin & Scott [3], Hopcroft,
Motwani & Ullman [4]) *lazily* — one DFA state is materialized per touch, never the whole powerset.

**States.** A WFST state is a **set of NFA states** `` $`S \subseteq Q_N`$ ``. Only reachable,
closure-saturated sets are minted, and each is interned to a dense `u32` id by a shared
`Arc<RwLock<NfaStateRegistry>>`. Write `` $`\mathrm{Eclose}(\cdot)`$ `` for epsilon
closure, `` $`\mathrm{SAclose}(\cdot)`$ `` for the epsilon **plus start-anchor** closure
(`^`, start-of-input), and `` $`\mathrm{EAclose}(\cdot)`$ `` for the epsilon **plus end-anchor**
closure (`$`, end-of-line, end-of-input). Then

```math
Q \;=\; \{\, S \subseteq Q_N : S \text{ reachable from } q_0 \,\}.
```

**Initial state.** Start anchors are valid only *before* the first character, so they are folded into
state id `` $`0`$ `` at construction rather than followed later:

```math
q_0 \;=\; \mathrm{SAclose}\bigl(\{\, q_N^{0} \,\}\bigr).
```

**Transition relation.** From a state `` $`S`$ ``, gather the concrete characters that its
input-consuming NFA labels admit (see §4), and for each such `` $`c`$ `` take the union of NFA
successors and epsilon-close it. Every consumed character is an **identity** edge charged
`` $`\omega`$ ``:

```math
\delta(S, c) \;=\; \mathrm{Eclose}\!\Bigl(\ \bigcup_{q \in S}\ \{\, q' \;:\; q \xrightarrow{\ \ell\ } q' \in E_N,\ \ell \text{ consumes } c \,\}\ \Bigr),
\qquad
S \;\xrightarrow{\ c\,:\,c\ /\ \omega\ }\; \delta(S, c).
```

Empty successor sets are dropped (no edge). Interning `` $`\delta(S,c)`$ `` is atomic under the shared
registry, so the lazy `expand` path and the immutable `compute_state` path assign identical ids.

**Final predicate and final weight.** A state is accepting iff it can reach a final NFA state through
epsilon and **end-anchor** labels; acceptance is free:

```math
F \;=\; \{\, S \in Q : \mathrm{EAclose}(S) \cap F_N \neq \varnothing \,\},
\qquad
\rho(S) \;=\;
\begin{cases}
\bar{1} = 0 & S \in F,\\[2pt]
\bar{0} = +\infty & S \notin F.
\end{cases}
```

The end-anchor closure in `` $`F`$ `` is what makes a pattern like `one$` accept exactly when the
consumed prefix ends the string (`src/phonetic_anchors.rs`).

## 3. API surface (duallity 4.0.0-rc.4)

`PhoneticNfaWfst` is exported from the crate root **behind `features = ["phonetic-rules"]`**
(`src/phonetic_nfa_wfst.rs`). It is a concrete, non-generic type wrapping
`liblevenshtein::phonetic::nfa::NFAChar`.

```rust,ignore
#[cfg(feature = "phonetic-rules")]
pub struct PhoneticNfaWfst {
    nfa: NFAChar,                          // the compiled phonetic NFA
    phonetic_weight: f64,                  // ω, charged per consumed transition
    alphabet: Arc<[char]>,                 // finite Σ for wide labels (`.`, negated classes)
    state_registry: Arc<RwLock<NfaStateRegistry>>,   // StateSet ⇄ StateId, shared by both views
    cache: LazyStateCache<CachedCharState>,
}

impl PhoneticNfaWfst {
    pub fn new(nfa: NFAChar) -> Self;                              // ω = 0.0, printable-ASCII Σ
    pub fn with_phonetic_weight(nfa: NFAChar, phonetic_weight: f64)
        -> Result<Self, InvalidWeightError>;
    pub fn with_alphabet<I: IntoIterator<Item = char>>(nfa: NFAChar, alphabet: I) -> Self;  // ω = 0.0
    pub fn with_phonetic_weight_and_alphabet<I: IntoIterator<Item = char>>(
        nfa: NFAChar, phonetic_weight: f64, alphabet: I,
    ) -> Result<Self, InvalidWeightError>;
    pub fn phonetic_weight(&self) -> f64;
    pub fn alphabet(&self) -> &[char];
    pub fn set_max_cache_size(&mut self, size: usize);
}
```

**Construction.** *Every* constructor seeds the registry with `` $`q_0 = \mathrm{SAclose}(\{q_N^0\})`$ ``
as state id `` $`0`$ `` and normalizes the alphabet (duplicate symbols are dropped after their first
occurrence, so transition order stays deterministic and bounded by the caller's ordering). The
default alphabet is the 95 **printable ASCII** scalars from `` `' '` `` (`` $`\texttt{0x20}`$ ``) to
`` `'~'` `` (`` $`\texttt{0x7E}`$ ``).

**Weight validation.** `with_phonetic_weight` and `with_phonetic_weight_and_alphabet` return
`Result<_, InvalidWeightError>`, rejecting `NaN`, infinities, and negatives before any
`TropicalWeight` is emitted. `new` and `with_alphabet` fix `` $`\omega = 0`$ `` and are infallible.

**Trait implementations.** `PhoneticNfaWfst` implements `Wfst<char, TropicalWeight>`,
`LazyWfst<char, TropicalWeight>`, and a **functional** `StateSource<char, TropicalWeight>`. The last
was made to agree with lazy expansion via the shared `Arc<RwLock<NfaStateRegistry>>`,
pinned by the integration test `phonetic_nfa_wfst_statesource_matches_lazy_expansion`
(`tests/phonetic_nfa_wfst.rs`). It also derives `Clone`. `num_states()` reports how many state sets
have been *registered* so far (lazy growth), not a static bound.

## 4. Alphabet contract for wide labels

`lling_llang::Wfst` transitions carry **concrete** labels, so a regular-expression label such as `.`
cannot mean "every Unicode scalar" without generating an infinite transition set. duallity therefore
treats the NFA WFST as **exact over a finite alphabet `` $`\Sigma`$ ``**. Candidate characters for a
state `` $`S`$ `` are gathered label-by-label (`collect_label_candidates`,
`collect_char_class_candidates`):

| NFA label | Candidate characters contributed |
|-----------|----------------------------------|
| `Char(c)` | exactly `` $`c`$ `` — **always exact**, even when `` $`c \notin \Sigma`$ `` |
| positive `CharClass` | every `` $`\sigma \in \Sigma`$ `` the class matches, **plus** every scalar in each explicit range of width `` $`\le 256`$ `` |
| negated `CharClass` | every `` $`\sigma \in \Sigma`$ `` the class matches (alphabet-relative) |
| `Any` (`.`) | every `` $`\sigma \in \Sigma`$ `` |

Because a positive class enumerates its own small explicit ranges (up to `MAX_EXACT_CLASS_RANGE_CHARS
= 256` scalars each), `[a-c]` is exact even with an empty custom alphabet. Negated classes and `.`
stay alphabet-relative because their mathematical denotation is otherwise unbounded. Supply a
domain-appropriate `` $`\Sigma`$ `` when the printable-ASCII default is wrong — for example
`['a','e','i','o','u','é']` for a vowel-focused stage — via `with_alphabet` or
`with_phonetic_weight_and_alphabet`.

## 5. Complexity

Let `` $`n = \lvert Q_N \rvert`$ `` be the NFA state count and `` $`\lvert\Sigma\rvert`$ `` the
alphabet size (default `95`). The reachable DFA has at most `` $`2^{n}`$ `` states — the powerset
bound (Rabin & Scott [3]) — but subset construction is **lazy**, so only states a search actually
visits are minted and cached. Expanding one state `` $`S`$ `` costs

```math
O\bigl(\lvert S \rvert \cdot \deg_N + \lvert\Sigma\rvert\bigr),
```

where `` $`\deg_N`$ `` is the per-state NFA out-degree: the candidate scan visits every outgoing NFA
transition of every member of `` $`S`$ ``, and wide labels iterate `` $`\Sigma`$ ``. Successor
grouping uses an `` $`O(\lvert\Sigma\rvert)`$ `` indexed structure once the candidate set is large
(`` $`\ge 16`$ ``), a linear scan below that. Interned successors are shared behind `Arc`, and
expanded states memoize in an LRU `LazyStateCache` (default capacity `50_000`), so each DFA state is
expanded once between evictions. There is no `` $`d \cdot M + a`$ `` radix here — state ids are dense
registry indices `` $`0, 1, 2, \ldots`$ `` (`next_nfa_state_id`).

## 6. Worked example

```rust,ignore
// Cargo.toml:  duallity = { version = "0.3", features = ["phonetic-rules"] }
use duallity::PhoneticNfaWfst;
use liblevenshtein::phonetic::{nfa::compile, regex::parse};
use lling_llang::prelude::*;

let ast = parse("(ph|f)one").expect("valid pattern");
let nfa = compile(&ast).expect("compiles");                 // Thompson construction [2]

let mut wfst = PhoneticNfaWfst::with_phonetic_weight(nfa, 0.1)
    .expect("valid phonetic weight");
let s0 = wfst.start();
wfst.expand(s0);
assert!(wfst.is_expanded(s0));

// From the start state, both branches of the alternation are live:
//   'p'  (opening the "ph" branch)  and  'f'  (the "f" branch),
// each an identity edge  c : c / 0.1  to the next NFA state set.
let starts: Vec<char> = wfst.transitions(s0).iter().filter_map(|t| t.input).collect();
assert!(starts.contains(&'p'));
assert!(starts.contains(&'f'));
```

Reading `p` advances into the `ph` branch (whose next required character is `h`); reading `f`
advances into the `f` branch (whose next required character is `o`). Both branches converge on the
shared suffix `one`, and the state reached after `…one` is final (its end-anchor closure contains a
final NFA state), with final weight `` $`\bar{1} = 0`$ ``. Each consumed character contributed
`` $`\omega = 0.1`$ ``, so an accepting walk over `phone` totals `` $`5 \times 0.1 = 0.5`$ `` and over
`fone` totals `` $`4 \times 0.1 = 0.4`$ ``.

## 7. ⚠ Honest limitations

- **The finite-alphabet contract is a genuine restriction.** `.`, negated classes, and unbounded
  positive classes are exact **only over the configured `` $`\Sigma`$ ``**. If your data contains
  scalars outside `` $`\Sigma`$ `` (and outside any small explicit `Char`/range), the corresponding
  wide-label transitions are simply not generated. Choose `` $`\Sigma`$ `` to cover your domain; do
  not assume `.` means "all of Unicode".
- **Bare NFA stage — no dictionary, no edit distance.** This transducer recognizes a *pattern*; it
  does not consult a dictionary and does not tolerate typos. For sound-alike matching against a
  dictionary within `` $`k`$ `` edits, use [`PhoneticWfst`](phonetic-wfst.md).
- **Feature-gated.** The type does not exist without `features = ["phonetic-rules"]`. The
  rule-based [`RewriteWfst`](phonetic-rewrite-wfst.md) is the always-available alternative when you
  need a preset-free build.
- **State growth is data-driven and unbounded in principle.** The `` $`2^{n}`$ `` powerset bound is
  worst-case; adversarial patterns over a large alphabet can inflate the registry. The LRU cache
  bounds *resident* transition storage, not the id space — size it with `set_max_cache_size`.

## 8. Diagram

A phonetic regex compiles (Thompson [2]) to an NFA, which `PhoneticNfaWfst` exposes by subset
construction; the same NFA is the left factor of the [`PhoneticWfst`](phonetic-wfst.md) product:

<img src="../diagrams/phonetic-regex-nfa-product.svg" alt="A phonetic regex (ph|f)one parses to an AST, compiles by Thompson construction to an NFA, and PhoneticNfaWfst exposes that NFA as a WFST by lazy subset construction; the same NFA becomes the left factor of the NFA × Levenshtein × Dictionary product" width="860"/>

## See also

- [theory/07 · Regular-language limits](../theory/07-regular-language-limits.md) — regex is regular; the subset construction and its Chomsky-hierarchy placement.
- [theory/05 · Universal automata](../theory/05-universal-automata.md) — the other query-agnostic construction in duallity.
- [design/phonetic-wfst](phonetic-wfst.md) — this NFA fused with Levenshtein and a dictionary.
- [design/phonetic-rewrite-wfst](phonetic-rewrite-wfst.md) — the rule-based, feature-free alternative.
- [design/phonetic-pipeline-builder](phonetic-pipeline-builder.md) — the `build_phonetic_nfa()` front-end.
- Source: `src/phonetic_nfa_wfst.rs`, `src/phonetic_nfa_support.rs`, `src/phonetic_anchors.rs`.

## References

1. **Mohri, M.** (1997). *Finite-State Transducers in Language and Speech Processing.* Computational
   Linguistics 23(2), 269–311. [ACL Anthology J97-2003](https://aclanthology.org/J97-2003/) — WFSTs
   as the composition substrate this stage plugs into.
2. **Thompson, K.** (1968). *Programming Techniques: Regular expression search algorithm.*
   Communications of the ACM 11(6), 419–422.
   [doi:10.1145/363347.363387](https://doi.org/10.1145/363347.363387) — compiling a regex to an NFA.
3. **Rabin, M. O., & Scott, D.** (1959). *Finite automata and their decision problems.* IBM Journal
   of Research and Development 3(2), 114–125. [doi:10.1147/rd.32.0114](https://doi.org/10.1147/rd.32.0114)
   — the subset (powerset) construction and the `` $`2^{n}`$ `` state bound.
4. **Hopcroft, J. E., Motwani, R., & Ullman, J. D.** (2006). *Introduction to Automata Theory,
   Languages, and Computation* (3rd ed.). Pearson. ISBN 978-0321455369 — subset construction and
   epsilon-closure as a textbook procedure.
