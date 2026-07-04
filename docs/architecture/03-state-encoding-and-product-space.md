# 03 · State encoding and the product space

> **Defines:** how a product state `(dict_node, automaton_state)` packs into one `StateId`, the radix
> `M`, and the `is_valid_state` heuristic.

## 1. The product state space

Every duallity WFST walks **two structures at once**: a dictionary (a trie/DAWG of terms) and an
automaton (the Levenshtein band, a universal-automaton position set, or an NFA×Levenshtein product).
A state of the WFST is therefore a **pair**:

```
state  =  (dict_node, automaton_state)
```

`lling_llang` identifies states by a single `StateId` (a `u32`). duallity packs the pair into that
`u32` with a **mixed-radix encoding** — the public `state_encoding` module in `lib.rs`:

```rust,ignore
// lib.rs — state_encoding
pub fn try_encode(
    dict_node: u32,
    automaton_state: u32,
    max_automaton_states: u32,
) -> Option<StateId> {
    if max_automaton_states == 0 || automaton_state >= max_automaton_states {
        return None;
    }

    dict_node
        .checked_mul(max_automaton_states)
        .and_then(|base| base.checked_add(automaton_state))
}

pub fn decode(state_id: StateId, max_automaton_states: u32) -> Option<(u32, u32)> {
    if max_automaton_states == 0 {
        return None;
    }

    let automaton_state = state_id % max_automaton_states;
    let dict_node       = state_id / max_automaton_states;
    Some((dict_node, automaton_state))
}
```

This is a bijection as long as `M > 0` and `automaton_state < M`, where
`M = max_automaton_states` is the **radix**. `try_encode` rejects out-of-range components and
overflow, and `decode` rejects a zero radix instead of dividing by zero:

<img src="../diagrams/state-encoding-bijection.svg" alt="(dict_node, automaton_state) packs into StateId = dict_node·M + automaton_state, with decode as the inverse" width="760"/>

The round-trip is asserted in `lib.rs`'s unit tests (`test_state_encoding_roundtrip`): for every
`(d, a)` in a grid, `try_encode(d, a, M).and_then(|s| decode(s, M)) == Some((d, a))`.

## 2. Choosing the radix `M`

`M` must exceed the largest reachable `automaton_state`, or two different pairs would collide. Each
engine sizes `M` from its own state space:

| WFST / state source | `M` (max_automaton_states) | Rationale |
|---------------------|----------------------------|-----------|
| `LevenshteinWfst` / `LevenshteinStateSource` | `(n+1)·(k+1)·c`, where `c` is the number of enabled continuation-state classes | the normal lattice is `(query_position, edit_cost)`; transposition and merge/split reserve disjoint continuation ranges. |
| `UniversalLevenshteinWfst` | a registry-derived bound on the number of distinct universal states and query cursors | universal states are deduplicated to sequential ids. |
| `PhoneticWfst` / `PhoneticStateSource` | `max_product_states = ((k+1)·1000).max(10_000)` | a generous bound on NFA×Levenshtein product states. |

The parameterized source uses a compact normal lattice for the ordinary states and allocates
additional contiguous ranges only for algorithms that need a one-step continuation. That keeps the
state space tighter than a diagonal-band over-estimate while preserving a constant-time decode.

**Worked example.** Query `"helo"` (`n = 4`), `k = 2`, standard edits only ⇒
`M = 5·3 = 15`. The product state `(dict_node = 3, automaton_state = 2)` encodes to
`3·15 + 2 = 47`, and `decode(47, 15) = Some((3, 2))`. ✓

## 3. `is_valid_state` checks registered product components

The `Wfst::is_valid_state` check decodes the product id and verifies that both components have been
registered by the lazy expansion frontier:

```rust,ignore
fn is_valid_state(&self, state: StateId) -> bool {
    let Some((dict_node, automaton_state)) =
        state_encoding::decode(state, self.max_automaton_states)
    else {
        return false;
    };

    self.node_registry.contains(dict_node)
        && self.automaton_registry.contains(automaton_state)
}
```

This is still cheap because the registries are hash/vector lookups, but it no longer treats every
syntactically decodable id as reachable. Invalid ids therefore return empty lazy transitions without
polluting the cache.

## 4. Why pack at all?

Packing keeps the entire `lling_llang` machinery — `compose`, shortest-path search, the cache —
working in terms of a single opaque `StateId`, with no knowledge that a state is "really" a pair. The
two-structure walk is duallity's private business; the encoding is the seam that hides it. The same
trick is reused by the phonetic product source (`PhoneticStateSource`), which packs
`(dict_node, product_state)` with its own radix.
