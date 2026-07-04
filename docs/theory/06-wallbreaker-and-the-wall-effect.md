# 06 · WallBreaker and the wall effect

> **Prerequisites:** [02 · Edit distance and Levenshtein automata](02-edit-distance-and-levenshtein-automata.md).
> **Defines:** the wall effect, the pigeonhole split, exact-substring seeding, bidirectional extension.

## 1. The wall effect

The Levenshtein automaton of chapter [02](02-edit-distance-and-levenshtein-automata.md) is fast when
`k` is small, because its reachable band is narrow. But the band has width `2k+1`, and at the **start**
of the dictionary traversal nothing has yet been matched, so the first `k` characters cannot prune
*anything*: every dictionary prefix of length `≤ k` is still a live candidate. The search must expand
all of them before the automaton can begin to discriminate. This combinatorial barrier is the
**wall effect** — it makes large-`k` search over a big dictionary intractable for the plain automaton
(e.g. 100-character patterns at distance 16 over a 750 K-word dictionary).

## 2. WallBreaker: jump past the wall

Gerdjikov, Mihov, Mitankin & Schulz (2013) [[1]](#references) overcome the wall with a different
strategy: instead of growing candidates character by character from the left, **find an exact piece
of the query somewhere in the dictionary first, then extend outward**. duallity wraps this as
`WallBreakerWfst`; the algorithm itself lives in `liblevenshtein::wallbreaker` and runs in four
stages:

<img src="../diagrams/wallbreaker-pipeline.svg" alt="WallBreaker pipeline: pigeonhole split, exact-substring seed on the SCDAWG, bidirectional extension, verify and dedup" width="900"/>

### Stage 1 — Pigeonhole split

Cut the query into contiguous **pieces**. The piece count is chosen so that the pigeonhole principle
guarantees at least one piece survives the error budget untouched:

| Algorithm | Pieces | Why |
|-----------|--------|-----|
| `Standard` | `k + 1` | `k` edits can damage at most `k` of `k+1` pieces ⇒ ≥1 is clean. |
| `Transposition` | `2k + 1` | a boundary-spanning transposition can corrupt **two** adjacent pieces. |
| `MergeAndSplit` | `2k + 1` | a boundary-spanning merge/split can likewise corrupt two pieces. |

<img src="../diagrams/pigeonhole-principle.svg" alt="k edits over k+1 pieces leave at least one piece uncorrupted; transposition needs 2k+1" width="780"/>

These counts are not folklore — they are **formally verified in Coq** upstream (the pigeonhole
theorem for each metric), with explicit counterexamples (`"ABCDE" → "ACBDX"`, `"abcdef" → "aXYf"`)
showing why `k+1` is insufficient once transpositions or merges are allowed.

### Stage 2 — Exact-substring seeds

For each piece, look it up as an **exact substring** of the dictionary using
`SubstringDictionary::find_exact_substring`. The canonical implementor is the **SCDAWG** (Symmetric
Compact Directed Acyclic Word Graph), which answers "where does this substring occur, and in which
terms?" in time **linear in the piece length and independent of `k`**. Because at least one piece is
uncorrupted, at least one lookup lands on the matching term — this is the move that vaults over the
wall. Each hit is a seed: a node, a term, and a position.

### Stage 3 — Bidirectional extension

From each seed, reconstruct the full term and re-derive its distance to the *whole* query by a small
Levenshtein-bounded depth-first search in both directions:

- **left** — walk *backward* toward the dictionary root via `parent()` / `parent_label()`, matching
  the query prefix (the characters before the piece);
- **right** — walk *forward* toward the leaves via `edges()`, matching the query suffix.

Each step tries the four edit operations with `distance ≤ k` pruning, accepting only when the right
walk reaches a terminal node. Because the dictionary nodes are bidirectional
(`BidirectionalDictionaryNode`), the same code serves byte and Unicode dictionaries.

### Stage 4 — Verify and deduplicate

Extension distances are provisional. Each candidate term is **re-verified** with the exact distance
function for the chosen algorithm, kept only if `≤ k`, and deduplicated through a `HashSet<String>`.
The survivors are `WallBreakerResult { term, distance }`.

## 3. From results to a WFST

`WallBreakerWfst` runs all four stages **eagerly at construction**, then presents the result set as a
lazy WFST: a single super-start state fanning out one **identity-labelled linear chain per matched
term**, each chain's accepting terminal carrying `final_weight = distance`. The structure and its
state keys are detailed in [design/wallbreaker-wfst](../design/wallbreaker-wfst.md) and pictured in
diagram D9.

## 4. When to reach for it

| Situation | Prefer |
|-----------|--------|
| small `k` (1–2), interactive correction | [`LevenshteinWfst`](../design/levenshtein-wfst.md) or universal |
| **large `k`** over a big dictionary | **`WallBreakerWfst`** (needs a `SubstringDictionary` / SCDAWG) |
| many queries, same dictionary, small `k` | [`BoundUniversalWfst`](../design/universal-wfst.md) |

See [guides/02 · Choosing a variant](../guides/02-choosing-a-variant.md) for the full decision guide.

## References

1. Gerdjikov, S., Mihov, S., Mitankin, P., & Schulz, K. U. (2013). *Wall-Breaker: Overcoming the Wall
   Effect in Similarity Search.* In *Proceedings of the Joint EDBT/ICDT 2013 Workshops*, 366–373.
   ACM. See [`references/bibliography.md`](../references/bibliography.md) for the resolved DOI.
2. Schulz, K. U., & Mihov, S. (2002). *Fast String Correction with Levenshtein Automata.* IJDAR 5(1),
   67–85. [doi:10.1007/s10032-002-0082-8](https://doi.org/10.1007/s10032-002-0082-8).
