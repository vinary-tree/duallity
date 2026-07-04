# Bibliography

The works cited across duallity's documentation, grouped by topic. Every entry carries a resolvable
identifier (DOI, ACL Anthology id, or ISBN). These are the citations relevant to *this crate*; the
inherited off-scope bibliography is retired under [`../roadmap/references/papers.md`](../roadmap/references/papers.md).

## Edit distance and Levenshtein automata

1. **Levenshtein, V. I.** (1966). *Binary codes capable of correcting deletions, insertions, and
   reversals.* Soviet Physics Doklady 10(8), 707–710. (Originally *Doklady Akademii Nauk SSSR* 163(4),
   845–848, 1965.) — the edit distance.
2. **Wagner, R. A., & Fischer, M. J.** (1974). *The String-to-String Correction Problem.* Journal of
   the ACM 21(1), 168–173. [doi:10.1145/321796.321811](https://doi.org/10.1145/321796.321811) — the
   dynamic-programming edit lattice.
3. **Schulz, K. U., & Mihov, S.** (2002). *Fast String Correction with Levenshtein Automata.*
   International Journal on Document Analysis and Recognition (IJDAR) 5(1), 67–85.
   [doi:10.1007/s10032-002-0082-8](https://doi.org/10.1007/s10032-002-0082-8) — the automaton duallity
   wraps.
4. **Mihov, S., & Schulz, K. U.** (2004). *Fast Approximate Search in Large Dictionaries.*
   Computational Linguistics 30(4), 451–477.
   [doi:10.1162/0891201042544938](https://doi.org/10.1162/0891201042544938) — the universal automaton.

## WallBreaker (large-distance search)

5. **Gerdjikov, S., Mihov, S., Mitankin, P., & Schulz, K. U.** (2013). *WallBreaker: Overcoming the
   Wall Effect in Similarity Search.* In *Proceedings of the Joint EDBT/ICDT 2013 Workshops*, 366–369.
   ACM. Proceedings [doi:10.1145/2457317](https://doi.org/10.1145/2457317) ·
   [Semantic Scholar](https://www.semanticscholar.org/paper/58b0aec47f79ded87483f03951d48d182fbbc7d6).

## Weighted finite-state transducers and composition

6. **Mohri, M.** (1997). *Finite-State Transducers in Language and Speech Processing.* Computational
   Linguistics 23(2), 269–311. [ACL Anthology J97-2003](https://aclanthology.org/J97-2003/) — rational
   relations and their closure under composition.
7. **Mohri, M., Pereira, F., & Riley, M.** (2002). *Weighted Finite-State Transducers in Speech
   Recognition.* Computer Speech & Language 16(1), 69–88.
   [doi:10.1006/csla.2001.0184](https://doi.org/10.1006/csla.2001.0184) — composition and the tropical
   semiring.
8. **Mohri, M.** (2009). *Weighted Automata Algorithms.* In *Handbook of Weighted Automata*, 213–254.
   Springer. [doi:10.1007/978-3-642-01492-5_6](https://doi.org/10.1007/978-3-642-01492-5_6).
9. **Droste, M., & Kuich, W.** (2009). *Semirings and Formal Power Series.* In *Handbook of Weighted
   Automata*, 3–28. Springer.
   [doi:10.1007/978-3-642-01492-5_1](https://doi.org/10.1007/978-3-642-01492-5_1) — the semiring axioms.

## Regular expressions and formal-language theory

10. **Thompson, K.** (1968). *Programming Techniques: Regular expression search algorithm.*
    Communications of the ACM 11(6), 419–422.
    [doi:10.1145/363347.363387](https://doi.org/10.1145/363347.363387) — the regex→NFA construction.
11. **Chomsky, N.** (1956). *Three models for the description of language.* IRE Transactions on
    Information Theory 2(3), 113–124.
    [doi:10.1109/TIT.1956.1056813](https://doi.org/10.1109/TIT.1956.1056813) — the language hierarchy.
12. **Hopcroft, J. E., Motwani, R., & Ullman, J. D.** (2006). *Introduction to Automata Theory,
    Languages, and Computation* (3rd ed.). Pearson. ISBN 978-0321455369 — the pumping lemma and
    regular-language limits.

## How these map to the documentation

| Citation | Used in |
|----------|---------|
| 1, 2, 3 | [theory/02 · Edit distance and Levenshtein automata](../theory/02-edit-distance-and-levenshtein-automata.md) |
| 3, 4 | [theory/05 · Universal automata](../theory/05-universal-automata.md) |
| 5 | [theory/06 · WallBreaker and the wall effect](../theory/06-wallbreaker-and-the-wall-effect.md) |
| 6, 7, 8, 9 | [theory/01 · Semirings and WFSTs](../theory/01-semirings-and-wfsts.md), [theory/04 · Composition](../theory/04-composition.md) |
| 10, 11, 12 | [theory/07 · Regular-language limits](../theory/07-regular-language-limits.md) |
