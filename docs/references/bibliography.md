# Bibliography

The works cited across duallity's documentation, grouped by topic. This page is the **consolidated,
canonical** citation list; each numbered entry below is mirrored, verbatim in substance, by the
page-local `## References` sections of the [theory](../theory/), [design](../design/), and
[architecture](../architecture/) chapters that use it.

**Identifiers.** Every entry carries the strongest available **resolvable identifier**, in this order
of precedence:

1. a **DOI**, linked as `https://doi.org/…` — preferred wherever one is registered;
2. an **ACL Anthology** id, for ACL/CL venues that predate or omit a DOI (e.g. Mohri 1997);
3. an **ISBN**, for books without a chapter- or volume-level DOI (e.g. Sakarovitch 2009).

The sole pre-DOI item — Levenshtein (1966) — is pinned instead by its full bibliographic coordinates
and its original *Doklady* citation. Every DOI in this file was checked to resolve (HTTP 302 →
publisher) and to match its work's title, venue, volume, issue, page range, and year against the
[Crossref](https://www.crossref.org/) metadata record.

**Numbering.** Entries are numbered **1–22 globally**, in topic order, and this global number is the
one the [mapping table](#how-these-map-to-the-documentation) uses. It is *independent* of the
bracketed `[n]` markers inside each chapter: those are **page-local** and resolve within that
chapter's own `## References` list (see [references/README](README.md#how-a-citation-flows)).

**Scope.** These are the citations relevant to *this crate*. The inherited, off-scope bibliography
from an earlier project (the FST + CFG + neural text-normalization research notes) is retired under
[`../archive/references/papers.md`](../archive/references/papers.md); only works genuinely relevant to
duallity appear here.

---

## Edit distance and Levenshtein automata

The metric duallity computes, the dynamic program that defines it, and the automata that accept a
`` $`k`$ ``-neighborhood without recomputing the lattice per candidate.

1. **Levenshtein, V. I.** (1966). *Binary codes capable of correcting deletions, insertions, and
   reversals.* Soviet Physics Doklady 10(8), 707–710. (Originally *Doklady Akademii Nauk SSSR*
   163(4), 845–848, 1965.) — the edit distance `` $`d_{\mathrm{lev}}`$ `` itself.
2. **Damerau, F. J.** (1964). *A technique for computer detection and correction of spelling errors.*
   Communications of the ACM 7(3), 171–176.
   [doi:10.1145/363958.363994](https://doi.org/10.1145/363958.363994) — adjacent transposition as a
   unit-cost edit; the `` $`d_{\mathrm{DL}}`$ `` metric behind the `Transposition` variant.
3. **Wagner, R. A., & Fischer, M. J.** (1974). *The String-to-String Correction Problem.* Journal of
   the ACM 21(1), 168–173. [doi:10.1145/321796.321811](https://doi.org/10.1145/321796.321811) — the
   dynamic-programming edit lattice `` $`\Delta`$ ``.
4. **Ukkonen, E.** (1985). *Algorithms for approximate string matching.* Information and Control
   64(1–3), 100–118. [doi:10.1016/S0019-9958(85)80046-2](https://doi.org/10.1016/S0019-9958(85)80046-2)
   — the diagonal-band (cutoff) dynamic program duallity's `` $`O(k)`$ ``-frontier evaluation
   reproduces.
5. **Myers, G.** (1999). *A fast bit-vector algorithm for approximate string matching based on
   dynamic programming.* Journal of the ACM 46(3), 395–415.
   [doi:10.1145/316542.316550](https://doi.org/10.1145/316542.316550) — bit-parallel edit distance,
   the word-packed relative of the characteristic-vector transition.
6. **Schulz, K. U., & Mihov, S.** (2002). *Fast String Correction with Levenshtein Automata.*
   International Journal on Document Analysis and Recognition (IJDAR) 5(1), 67–85.
   [doi:10.1007/s10032-002-0082-8](https://doi.org/10.1007/s10032-002-0082-8) — the automaton duallity
   wraps: elementary transitions, the relevant subword, and Proposition 11.
7. **Mihov, S., & Schulz, K. U.** (2004). *Fast Approximate Search in Large Dictionaries.*
   Computational Linguistics 30(4), 451–477.
   [doi:10.1162/0891201042544938](https://doi.org/10.1162/0891201042544938) — the universal
   (query-agnostic) automaton and its characteristic-vector transition function.

## Weighted transducers and semirings

Why a fuzzy matcher must *be* a WFST to compose, and the algebra of weights it composes over.

8. **Elgot, C. C., & Mezei, J. E.** (1965). *On Relations Defined by Generalized Finite Automata.*
   IBM Journal of Research and Development 9(1), 47–68.
   [doi:10.1147/rd.91.0047](https://doi.org/10.1147/rd.91.0047) — finite transducers realize exactly
   the **rational relations**, and these are closed under composition.
9. **Mohri, M.** (1997). *Finite-State Transducers in Language and Speech Processing.* Computational
   Linguistics 23(2), 269–311. [ACL Anthology J97-2003](https://aclanthology.org/J97-2003/) — the
   canonical WFST treatment: the path-weight functional `` $`T(x, y)`$ `` and closure under composition.
10. **Mohri, M., Pereira, F., & Riley, M.** (2002). *Weighted Finite-State Transducers in Speech
    Recognition.* Computer Speech & Language 16(1), 69–88.
    [doi:10.1006/csla.2001.0184](https://doi.org/10.1006/csla.2001.0184) — weighted composition, the
    `` $`\varepsilon`$ ``-filter, and the tropical `` $`(\min, +)`$ `` semiring as the decoding weight.
11. **Droste, M., Kuich, W., & Vogler, H.** (Eds.) (2009). *Handbook of Weighted Automata.* EATCS
    Monographs in Theoretical Computer Science. Springer.
    [doi:10.1007/978-3-642-01492-5](https://doi.org/10.1007/978-3-642-01492-5) — the reference volume
    for weighted automata, semirings, and their algorithms; nos. 12 and 13 are chapters within it.
12. **Droste, M., & Kuich, W.** (2009). *Semirings and Formal Power Series.* In *Handbook of Weighted
    Automata* (no. 11), 3–28. Springer.
    [doi:10.1007/978-3-642-01492-5_1](https://doi.org/10.1007/978-3-642-01492-5_1) — the semiring
    axioms and well-definedness of finite `` $`\oplus`$ ``-sums over a commutative monoid.
13. **Mohri, M.** (2009). *Weighted Automata Algorithms.* In *Handbook of Weighted Automata* (no. 11),
    213–254. Springer.
    [doi:10.1007/978-3-642-01492-5_6](https://doi.org/10.1007/978-3-642-01492-5_6) — the canonical
    three-state `` $`\varepsilon`$ ``-filter for composition and its completeness.
14. **Sakarovitch, J.** (2009). *Elements of Automata Theory.* Cambridge University Press.
    ISBN 978-0521844253 — rational relations, the composition theorem, and their non-closure under
    intersection, in textbook form.

## Regular languages and complexity

The finite-state model, the regex `` $`\to`$ `` NFA pipeline, and the tools that bound what a
Levenshtein or phonetic WFST can and cannot express.

15. **Chomsky, N.** (1956). *Three models for the description of language.* IRE Transactions on
    Information Theory 2(3), 113–124.
    [doi:10.1109/TIT.1956.1056813](https://doi.org/10.1109/TIT.1956.1056813) — the language hierarchy
    that places every duallity WFST at Type 3 (regular).
16. **Nerode, A.** (1958). *Linear automaton transformations.* Proceedings of the American
    Mathematical Society 9(4), 541–544. [doi:10.2307/2033204](https://doi.org/10.2307/2033204) — the
    right-congruence (Myhill–Nerode) characterization of regular languages.
17. **Rabin, M. O., & Scott, D.** (1959). *Finite Automata and Their Decision Problems.* IBM Journal
    of Research and Development 3(2), 114–125.
    [doi:10.1147/rd.32.0114](https://doi.org/10.1147/rd.32.0114) — nondeterministic finite automata
    and the subset (powerset) construction with its `` $`2^{n}`$ `` state bound.
18. **Thompson, K.** (1968). *Programming Techniques: Regular expression search algorithm.*
    Communications of the ACM 11(6), 419–422.
    [doi:10.1145/363347.363387](https://doi.org/10.1145/363347.363387) — the regex `` $`\to`$ `` NFA
    construction the phonetic front-end runs.
19. **Hopcroft, J. E., Motwani, R., & Ullman, J. D.** (2006). *Introduction to Automata Theory,
    Languages, and Computation* (3rd ed.). Pearson. ISBN 978-0321455369 — the pumping lemma, the
    Myhill–Nerode theorem, and the regular-language limits of chapter 07.

## Systems and large-distance search

The engineering contribution that makes similarity search tractable past the point where the
Levenshtein band collapses.

20. **Gerdjikov, S., Mihov, S., Mitankin, P., & Schulz, K. U.** (2013). *WallBreaker: Overcoming the
    Wall Effect in Similarity Search.* In *Proceedings of the Joint EDBT/ICDT 2013 Workshops*,
    366–369. ACM. Proceedings [doi:10.1145/2457317](https://doi.org/10.1145/2457317) ·
    [Semantic Scholar](https://www.semanticscholar.org/paper/58b0aec47f79ded87483f03951d48d182fbbc7d6)
    — the split/seed/extend/verify algorithm and its pigeonhole piece-count proofs
    (`` $`k+1`$ `` for Levenshtein, `` $`2k+1`$ `` for transposition and merge/split).

## Software architecture

Design guidance cited by the architecture chapters for duallity's crate-boundary rationale.

21. **Martin, R. C.** (2000). *Design Principles and Design Patterns.* Object Mentor. — the Acyclic
    Dependencies Principle and the Dependency-Inversion Principle; cited by architecture/01 for why the
    WFST adapters are extracted into a separate crate that sits *above* liblevenshtein and lling-llang,
    keeping the dependency graph acyclic.

## Persistence and the resource ABI

The immutable-revision model behind duallity's capture-once dictionary snapshot, which lets a WFST
resource outlive the source handle it was built from.

22. **Driscoll, J. R., Sarnak, N., Sleator, D. D., & Tarjan, R. E.** (1989). *Making Data Structures
    Persistent.* Journal of Computer and System Sciences 38(1), 86–124.
    [doi:10.1016/0022-0000(89)90034-2](https://doi.org/10.1016/0022-0000(89)90034-2) — a persistent
    structure exposes an immutable prior *version* that survives later updates; duallity's
    `snapshot`-captured dictionary revision ([architecture/06](../architecture/06-resource-abi-and-bindings.md))
    is exactly such a version, which is why a WFST can outlive its source dictionary handle.

---

## How these map to the documentation

Each row lists the **global entry numbers** (above) a page cites in its own `## References`. Together
the three tables cover every chapter of [theory](../theory/) 01–07 and every
[design](../design/)/[architecture](../architecture/) page that carries a citation.

### Theory

| Chapter | Cites |
|---------|-------|
| [theory/01 · Semirings and WFSTs](../theory/01-semirings-and-wfsts.md) | 9, 10, 11, 12 |
| [theory/02 · Edit distance and Levenshtein automata](../theory/02-edit-distance-and-levenshtein-automata.md) | 1, 3, 4, 5, 6 |
| [theory/03 · The Levenshtein automaton as a transducer](../theory/03-levenshtein-as-transducer.md) | 1, 2, 3, 6, 10 |
| [theory/04 · Composition](../theory/04-composition.md) | 8, 9, 10, 12, 13, 14 |
| [theory/05 · Universal automata](../theory/05-universal-automata.md) | 6, 7, 17 |
| [theory/06 · WallBreaker and the wall effect](../theory/06-wallbreaker-and-the-wall-effect.md) | 6, 7, 20 |
| [theory/07 · Regular-language limits](../theory/07-regular-language-limits.md) | 8, 9, 14, 15, 16, 18, 19 |

### Design

| Design page | Cites |
|-------------|-------|
| [levenshtein-wfst](../design/levenshtein-wfst.md) | 1, 2, 3, 6, 9 |
| [universal-wfst](../design/universal-wfst.md) | 1, 6, 7, 9 |
| [generalized-wfst](../design/generalized-wfst.md) | 6, 7, 10 |
| [phonetic-wfst](../design/phonetic-wfst.md) | 6, 10, 18 |
| [phonetic-nfa-wfst](../design/phonetic-nfa-wfst.md) | 9, 17, 18, 19 |
| [phonetic-rewrite-wfst](../design/phonetic-rewrite-wfst.md) | 9, 10 |
| [phonetic-pipeline-builder](../design/phonetic-pipeline-builder.md) | 10 |
| [wallbreaker-wfst](../design/wallbreaker-wfst.md) | 6, 10, 20 |

### Architecture

| Architecture page | Cites |
|-------------------|-------|
| [architecture/01 · Crate family and dependency graph](../architecture/01-crate-family-and-dependency-graph.md) | 9, 10 † |
| [architecture/06 · The resource ABI and language bindings](../architecture/06-resource-abi-and-bindings.md) | 6, 7, 9, 10, 21, 22 |

> † architecture/01's inline `[13]` maps to **Martin, R. C.** (2000) — entry **21** above; its
> page-local number differs from the global one, per the numbering scheme in
> [references/README](README.md).

## See also

- [references/README](README.md) — citation conventions, identifier precedence, and how a citation
  flows from a chapter's inline `[n]` to this consolidated list.
- [glossary](glossary.md) — every term, symbol, and acronym, defined alphabetically.
- [theory/README · Master notation](../theory/README.md#master-notation) — the single source of truth
  for the symbols (`` $`d_{\mathrm{lev}}`$ ``, `` $`\oplus`$ ``, `` $`\varepsilon`$ ``, …) these
  entries reference.
