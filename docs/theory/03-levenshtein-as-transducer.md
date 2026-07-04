# 03 · The Levenshtein automaton as a transducer

> **Prerequisites:** [02 · Edit distance and Levenshtein automata](02-edit-distance-and-levenshtein-automata.md).
> **Defines:** the exact label/weight semantics duallity emits — **input = query side, output =
> dictionary side**.

## 1. The transducer contract

duallity presents the Levenshtein automaton as a transducer whose two tapes are fixed by contract
(this is the orientation that commit `be3dc6a` made canonical, and that the integration tests pin):

- **input label = the query side** — a character of `q`, or `ε`;
- **output label = the dictionary side** — a character of the term `w`, or `ε`;
- **weight = the tropical cost** of that step (`0` for a free match, `1` for one edit).

A product state is the pair `(dict_node, query_pos)`: how far we have walked into the dictionary
(`dict_node`) and how much of the query we have consumed (`query_pos`). The four edit operations are
the four ways to leave such a state:

<img src="../diagrams/transducer-two-tape.svg" alt="The four edit operations as labelled, weighted transitions out of a product state" width="760"/>

| Operation | Condition | input : output | weight | Successor state |
|-----------|-----------|----------------|--------|-----------------|
| **match** | `pos < n` and `q[pos] = c` | `q[pos] : c` | `0` | `(child_node, pos+1)` |
| **substitute** | `pos < n` and `q[pos] ≠ c` | `q[pos] : c` | `1` | `(child_node, pos+1)` |
| **insert** | for every dictionary edge `c` | `ε : c` | `1` | `(child_node, pos)` |
| **delete** | `pos < n` | `q[pos] : ε` | `1` | `(dict_node, pos+1)` |

Here `c` ranges over the labels of the outgoing dictionary edges of `dict_node`, and `child_node` is
the dictionary node that edge leads to. Note the asymmetry that makes the two tapes meaningful:

- **insert** advances the **dictionary** (`dict_node → child_node`) but **not** the query — it
  accounts for an *extra* character that the term has and the query lacks, so the query side is `ε`.
- **delete** advances the **query** (`pos → pos+1`) but **not** the dictionary — it accounts for a
  character the query has and the term lacks, so the dictionary side is `ε`.

## 2. Acceptance and final weight

A product state `(dict_node, query_pos)` is **accepting** when the dictionary node is a terminal
(end of a real word) *and* the query can be finished within the remaining budget:

```
is_final  ⟺  dict_node.is_final()  ∧  remaining ≤ k        where  remaining = n − query_pos
```

When accepting, the **final weight** is `remaining` — the cost of deleting the query characters not
yet consumed (each a unit-cost delete). Otherwise the final weight is `TropicalWeight::zero()`, which
is `+∞` (recall the [gotcha](README.md#semirings-and-weights): `zero()` means "no accepting path
here", not "cost 0").

## 3. The transition kernel, as literate pseudocode

The whole semantics live in one function, `LevenshteinStateSource::compute_transitions`. In
literate-programming form (Knuth):

```
function COMPUTE-TRANSITIONS(dict_node, query_pos):
    ⟨ resolve the dictionary node and prepare an empty transition buffer ⟩
    transitions ← [ ]

    ⟨ Match / Substitute / Insert: one pass over the dictionary node's outgoing edges ⟩
    for each edge (c, child_node) of dict_node:
        if query_pos < n and q[query_pos] = c:
            ▷ match: consume one query char and one dict char, free
            emit  q[query_pos] : c  / 0   →  (child_node, query_pos + 1)
        else if query_pos < n:
            ▷ substitute: consume one of each, cost 1
            emit  q[query_pos] : c  / 1   →  (child_node, query_pos + 1)
        ▷ insert: the term has an extra char; advance the dictionary only, cost 1
        emit  ε : c  / 1              →  (child_node, query_pos)

    ⟨ Delete: the term is missing a char; advance the query only, cost 1 ⟩
    if query_pos < n:
        emit  q[query_pos] : ε  / 1   →  (dict_node, query_pos + 1)

    ⟨ Decide acceptance and the final (deletion-of-tail) weight ⟩
    remaining ← n − query_pos
    if dict_node.is_final() and remaining ≤ k:
        is_final, final_weight ← true,  TropicalWeight(remaining)
    else:
        is_final, final_weight ← false, TropicalWeight::zero()   ▷ +∞

    return (is_final, final_weight, transitions)
```

The real implementation buffers transitions in a `SmallVec<[WeightedTransition<char, TropicalWeight>; 4]>`
(four is the exact branching of one cell) and packs each successor `(node, pos)` back into a single
`StateId` via the encoding of
[architecture/03](../architecture/03-state-encoding-and-product-space.md).

## 4. Worked example: `"cat"` versus the four operations

duallity's tests (`state_source.rs::test_state_source_transition_labels_preserve_transducer_sides`)
nail down each operation's labels against a one-word dictionary:

| Dictionary | Query | Produced transition | Operation |
|------------|-------|---------------------|-----------|
| `["cat"]` | `"cat"` | `('c' : 'c') / 0` | match |
| `["cat"]` | `"bat"` | `('b' : 'c') / 1` | substitute (query `b`, dict `c`) |
| `["cat"]` | `"at"`  | `(ε : 'c') / 1`   | insert (term has an extra `c`) |
| `["at"]`  | `"cat"` | `('c' : ε) / 1`   | delete (query has an extra `c`) |

Read the substitution row carefully: the **input** label is the query character `b` and the
**output** label is the dictionary character `c`. That direction is the contract — and it is exactly
what makes the Levenshtein WFST composable on its output tape with a downstream transducer (chapter
[04](04-composition.md)).

> **A note on transposition.** In the *parameterized* `LevenshteinStateSource`, selecting
> `Algorithm::Transposition` adds an adjacent-swap path without changing the single-`char` WFST label
> type. The swap is split into two transitions: `(qᵢ : qᵢ₊₁)/1`, then `(qᵢ₊₁ : qᵢ)/0`. This preserves
> the total Damerau–Levenshtein cost of `1` while keeping each arc compatible with ordinary WFST
> composition. See [design/levenshtein-wfst](../design/levenshtein-wfst.md).

The same `ε`-on-one-tape convention reappears when phonetic rewrite rules expand or contract a
string — see the char/ε chains in [design/phonetic-rewrite-wfst](../design/phonetic-rewrite-wfst.md).
