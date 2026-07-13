# References

This section is duallity's **citation apparatus** — the two documents every other chapter defers to
for its sources and its vocabulary. Prose and diagrams elsewhere in the documentation stay uncluttered
by pushing bibliographic detail and term definitions here, once.

| Document | Contents |
|----------|----------|
| [Bibliography](bibliography.md) | Every work cited across the documentation, grouped by topic, numbered 1–20, each with a resolvable DOI / ACL id / ISBN and a one-line note on its role. |
| [Glossary](glossary.md) | Every term, symbol, and acronym, defined alphabetically; the prose mirror of the [theory master-notation table](../theory/README.md#master-notation). |

---

## Citation conventions

Three rules govern every citation in this documentation set.

1. **Resolvable identifiers, by precedence.** Each bibliography entry carries the strongest available
   identifier. When more than one exists, the higher-precedence one is shown:

   | Priority | Identifier | Used when | Link form | Example entry |
   |:--------:|-----------|-----------|-----------|---------------|
   | 1 | **DOI** | a DOI is registered (the common case) | `https://doi.org/10.x/y` | Wagner–Fischer 1974 → [doi:10.1145/321796.321811](https://doi.org/10.1145/321796.321811) |
   | 2 | **ACL Anthology id** | ACL/CL venues without a DOI | `https://aclanthology.org/<id>/` | Mohri 1997 → [J97-2003](https://aclanthology.org/J97-2003/) |
   | 3 | **ISBN** | books with no chapter/volume DOI | ISBN string | Sakarovitch 2009 → ISBN 978-0521844253 |
   | — | **bibliographic coordinates** | pre-DOI works | venue, vol(issue), pages, year | Levenshtein 1966 (Soviet Physics Doklady 10(8), 707–710) |

   Every DOI in the bibliography was verified to **resolve** (a `https://doi.org/…` request returns
   HTTP 302 to the publisher) *and* to **match** its work's metadata — title, authors, venue, volume,
   issue, page range, year — against the [Crossref](https://www.crossref.org/) record, before being
   included.

2. **Inline markers are page-local.** Inside a chapter, a citation is a bracketed number such as
   `[5]`. That number resolves against **that chapter's own `## References` list** — not against the
   global bibliography numbering. Each chapter is self-contained: you can read it, follow its `[n]`
   markers to its own footer, and never leave the page.

3. **This page is the canonical, consolidated list.** The [bibliography](bibliography.md) collects
   every page-local reference into one topic-ordered list numbered **1–20 globally**. Where a page's
   local list and the global list disagree on a *number*, the global number is the one used by the
   bibliography's [mapping table](bibliography.md#how-these-map-to-the-documentation); where they
   disagree on *substance*, the global entry is authoritative.

## How a citation flows

The one subtlety worth internalizing: **page-local numbers and global numbers are different indices
into the same works.** A citation travels from an inline marker, to the chapter's own footer, to the
consolidated bibliography, to the source itself:

```
 chapter prose            page-local              consolidated               resolves to
 (theory/03)              References (footer)     bibliography               the source
 ┌────────────────┐       ┌──────────────────┐    ┌───────────────────┐      ┌────────────────┐
 │ "… Damerau [5]"│ ────▶ │ [5] Damerau, F.J.│──▶ │ no. 2 · Damerau   │ ───▶ │ doi.org/       │
 │  inline marker │ same  │  (1964). A tech… │ ↑  │  (Edit distance & │ DOI  │ 10.1145/       │
 │                │ page  │  page-local n =5 │ │  │  automata group)  │      │ 363958.363994  │
 └────────────────┘       └──────────────────┘ │  └───────────────────┘      └────────────────┘
                                                │            ▲
                        same underlying work ───┘            │
                                                             │
        bibliography mapping table (work no. ⇄ citing pages)─┘
```

**Worked example.** In [theory/03 · The Levenshtein automaton as a transducer](../theory/03-levenshtein-as-transducer.md),
Damerau (1964) is cited inline as `[5]` and appears fifth in *that page's* `## References`. In the
[bibliography](bibliography.md), the *same* paper is **no. 2**, in the *Edit distance and Levenshtein
automata* group. The bibliography's [mapping table](bibliography.md#how-these-map-to-the-documentation)
records the reverse direction — no. 2 is cited in `theory/03` and `design/levenshtein-wfst` — so you
can go from a work to every page that uses it. The two numbering schemes never need to agree; the
mapping table is the bridge.

## Topic groups

The bibliography is partitioned into four groups, each answering one question about the crate:

| Group | Question it answers | Bibliography nos. |
|-------|---------------------|:-----------------:|
| **Edit distance and Levenshtein automata** | What is the metric, and what automaton accepts a `` $`k`$ ``-neighborhood? | 1–7 |
| **Weighted transducers and semirings** | Why must a matcher *be* a WFST to compose, and over what weight algebra? | 8–14 |
| **Regular languages and complexity** | What can a finite-state machine express, and where are its limits? | 15–19 |
| **Systems and large-distance search** | How is similarity search kept tractable past the wall effect? | 20 |

## Adding or updating a citation

Follow this checklist so the local lists, the global list, and the mapping table stay coherent.

1. **Choose the strongest identifier** per the precedence table above (DOI ≻ ACL id ≻ ISBN ≻ raw
   coordinates).
2. **Verify it** — confirm the DOI resolves (`https://doi.org/<doi>` → 302) *and* that its Crossref
   record matches the title, authors, venue, volume, issue, pages, and year you are about to write.
3. **Place it in the correct topic group** in [bibliography.md](bibliography.md) and renumber the
   global sequence 1..N so numbering stays contiguous and topic-ordered.
4. **Add or adjust the page-local entry** in the `## References` footer of every chapter that cites
   the work, using that chapter's own local numbering and the [house entry
   format](bibliography.md) (`**Author, A.** (year). *Title.* Venue vol(issue), pp. [doi:…](…) —
   note.`).
5. **Update the mapping table** so the work's row lists every citing page (and each citing page's row
   lists the work).
6. **Sync the [glossary](glossary.md)** if the work introduces a term, symbol, or acronym that the
   documentation defines and reuses.

## Scope and the retired bibliography

Only works genuinely relevant to the shipped duallity crate appear here. An earlier project bequeathed
a large, forward-looking bibliography for an FST + CFG + neural **text-normalization** architecture
that duallity does not implement; those neural / CFG / text-normalization papers have been retired to
[`../archive/references/papers.md`](../archive/references/papers.md), alongside the rest of the
[inherited research archive](../archive/README.md). The handful of entries there that *are* relevant
(Schulz & Mihov 2002; Mohri 1997; Mohri, Pereira & Riley 2002) were promoted into the canonical
[bibliography](bibliography.md) and appear above.

## See also

- [Bibliography](bibliography.md) — the consolidated, canonical citation list and its
  documentation-mapping table.
- [Glossary](glossary.md) — every term, symbol, and acronym, defined alphabetically.
- [theory/README · Master notation](../theory/README.md#master-notation) — the single source of truth
  for the mathematical symbols the cited works are attached to.
