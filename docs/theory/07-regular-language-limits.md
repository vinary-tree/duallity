# 07 · Regular-language limits — what a Levenshtein/phonetic WFST can and cannot express

> **Prerequisites:** [01 · Semirings and WFSTs](01-semirings-and-wfsts.md).
> **Defines:** the expressivity ceiling of regular transducers; the generalized-operation taxonomy.

Every transducer in duallity is a **finite-state** device. Finite-stateness is the source of both its
speed and its limits. This chapter states, honestly, what that buys and what it forbids — so you reach
for duallity when it fits and reach elsewhere when it does not.

## 1. Where duallity sits in the Chomsky hierarchy

The Chomsky hierarchy (Chomsky, 1956 [[1]](#references)) ranks formal-language classes by the machine
that recognizes them:

| Type | Class | Machine | Can it count / nest? |
|------|-------|---------|----------------------|
| 3 | **Regular** | finite automaton / **FST** | no |
| 2 | Context-free | pushdown automaton | balanced nesting, one stack |
| 1 | Context-sensitive | linear-bounded automaton | bounded context |
| 0 | Recursively enumerable | Turing machine | anything computable |

A WFST recognizes a **regular relation** (a *rational* relation) — Type 3. duallity is a fast,
composable engine for regular relations over strings, weighted in the tropical semiring. It is **not**
a parser, a type checker, or a grammar engine, and it does not pretend to be.

## 2. What a regular transducer *can* express

Within Type 3, duallity is expressive and useful:

- **Edit-bounded similarity** — "all terms within edit distance `k`", with the distance as a weight
  (chapters [02](02-edit-distance-and-levenshtein-automata.md)–[03](03-levenshtein-as-transducer.md)).
- **Generalized edit metrics** — by enlarging the operation set: transpositions
  (Damerau–Levenshtein), merges/splits (OCR), and phonetic digraph rewrites. These are catalogued by
  `OperationType ⟨consume_x, consume_y, weight, restriction⟩`:

  <img src="../diagrams/operationtype-taxonomy.svg" alt="OperationType taxonomy: standard, transposition, merge/split, and phonetic-digraph operations" width="880"/>

  Each operation consumes `consume_x` dictionary characters and `consume_y` query characters at a
  given cost; restricted operations apply only to a named character-pair set (e.g. `ph↔f`). This is
  the runtime-configurable machinery of [design/generalized-wfst](../design/generalized-wfst.md).
- **Phonetic rewrites** — rule-based string rewrites (`ph→f`, `ck→k`) are regular relations, encodable
  as the char/ε transition chains of [design/phonetic-rewrite-wfst](../design/phonetic-rewrite-wfst.md).
- **Phonetic regular expressions** — alternation, optionality, grouping, character classes
  (`(ph|f)one`) compile (Thompson, 1968 [[2]](#references)) to an NFA and hence to a WFST
  ([design/phonetic-wfst](../design/phonetic-wfst.md)).

All of these compose (chapter [04](04-composition.md)), because regular relations are closed under
composition (Mohri, 1997 [[3]](#references)).

## 3. What a regular transducer *cannot* express

Finite state means **finite, bounded memory**. A WFST cannot:

- **Count without bound** — recognize `{ aⁿbⁿ : n ≥ 0 }`, or balance parentheses/brackets to
  arbitrary depth. The classic **pumping lemma for regular languages** proves this: any sufficiently
  long accepted string has a substring that can be repeated arbitrarily while staying in the language,
  which `aⁿbⁿ` cannot tolerate.
- **Enforce nested or long-range structure** — subject–verb agreement across an arbitrarily deep
  embedded clause, or matching `begin`/`end` nesting in code, needs a *stack* (Type 2) or more.
- **Reason about meaning** — word-sense disambiguation, factual consistency, and discourse coherence
  are not regular (and largely not symbolic at all).

A useful litmus test: **if solving the problem requires remembering an unbounded amount of earlier
input, it is not regular, and a WFST cannot do it.** Bounded edit distance *is* regular (the memory is
the `O((n+1)(2k+1))` band); balanced nesting is not.

## 4. Honest positioning

duallity occupies the regular tier deliberately and well: it is deterministic, fast, interpretable,
needs no training data, and composes cleanly. Problems that genuinely require Type-2 (context-free
parsing) or learned models are **out of scope** for this crate. A research design for stacking CFG
and neural tiers on top of an FST core is preserved under [`../roadmap/`](../roadmap/README.md); it
sits outside duallity's shipped crate surface.

## References

1. Chomsky, N. (1956). *Three models for the description of language.* IRE Transactions on Information
   Theory 2(3), 113–124. [doi:10.1109/TIT.1956.1056813](https://doi.org/10.1109/TIT.1956.1056813).
2. Thompson, K. (1968). *Programming Techniques: Regular expression search algorithm.* Communications
   of the ACM 11(6), 419–422. [doi:10.1145/363347.363387](https://doi.org/10.1145/363347.363387).
3. Mohri, M. (1997). *Finite-State Transducers in Language and Speech Processing.* Computational
   Linguistics 23(2), 269–311. [ACL Anthology J97-2003](https://aclanthology.org/J97-2003/).
4. Hopcroft, J. E., Motwani, R., & Ullman, J. D. (2006). *Introduction to Automata Theory, Languages,
   and Computation* (3rd ed.). Pearson. ISBN 978-0321455369.
