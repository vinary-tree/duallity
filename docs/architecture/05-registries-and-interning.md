# 05 · Registries and interning

> **Defines:** the dictionary-node registry family, automaton/product state registries, the compact
> exact node key, and the `Arc<RwLock>` concurrency model.

## 1. Why interning is needed

A `StateId` is `(dict_node, automaton_state)` packed into a `u32`
([architecture/03](03-state-encoding-and-product-space.md)). But neither component arrives as a `u32`:

- a **dictionary node** is an opaque handle from libdictenstein with no inherent integer id;
- a **universal-automaton state** is a *set of positions*; a **product-automaton state** is an
  `(NFA state set, edit distance)` pair; an **NFA state** is a set of NFA states.

Each must be assigned a **stable, sequential `u32`** so it can be a component of a `StateId`. That is
the job of a **registry**: a bidirectional map between rich objects and dense ids, allocated on first
sight.

## 2. The registry families

| Registry | Keys | Used by |
|----------|------|---------|
| `DictionaryNodeRegistry<N>` | dictionary nodes, by compact exact **path-step key** | `LevenshteinStateSource`, `PhoneticStateSource`, `GeneralizedWfst` |
| `DepthDictionaryNodeRegistry<N>` | dictionary nodes, by compact exact **path-step key**, plus character depth | `UniversalLevenshteinStateSource` |
| `UniversalStateRegistry<V>` | universal-automaton states, by serialized **position set** plus exact query-label cursor | `UniversalLevenshteinStateSource` |
| `ProductStateRegistry` | product-automaton states, by serialized `(NFA states, edit distance)` | `PhoneticStateSource` |
| `NfaStateRegistry` | NFA states, by **sorted state-set** key | `PhoneticNfaWfst` |

Each follows the same shape: a forward map `key → u32` (an `FxHashMap`) plus a reverse vector
indexed by the id. The id `0` is reserved for the root/initial object. The depth-tracking
dictionary-node registry stores `(node, depth)` in its reverse vector; the other registries store the
interned object directly.

For the universal WFST, dictionary depth lives in `DepthDictionaryNodeRegistry` and the consumed
query-label cursor lives in `UniversalStateRegistry`. Keeping both cursors explicit prevents the
wrapper from recovering label position from universal offsets, which are abstract automaton data
rather than a WFST label cursor.

## 3. The compact exact node key

Dictionary nodes lack ids, so the dictionary-node registries key a child node by the exact path step
that reaches it: `(parent id, edge label)`. That pair is packed into a `DictionaryNodeKey(u64)`
instead of being hashed as opaque data:

```rust,ignore
pub(crate) struct DictionaryNodeKey(u64);

impl DictionaryNodeKey {
    pub(crate) const ROOT: Self = Self(u64::MAX);

    pub(crate) fn child(parent_id: u32, edge_label: char) -> Self {
        let codepoint = edge_label as u64;        // Unicode scalar value: at most 21 bits
        Self((u64::from(parent_id) << 21) | codepoint)
    }
}
```

<img src="../diagrams/noderegistry-interning.svg" alt="A dictionary node is keyed by the exact parent and edge path step into a u32 id; the reverse vector recovers the node" width="820"/>

This is exact for the represented domain: `parent_id` is `u32`, a Rust `char` is a Unicode scalar
value needing at most 21 bits, and `u64::MAX` is reserved for the root. Child keys therefore occupy
only the lower 53 bits and cannot equal the root sentinel. `FxHashMap` still hashes the key internally
for table lookup, but normal hash-table collisions are resolved by `Eq`; they do not alias dictionary
nodes.

The remaining future hardening option is for dictionaries to expose true stable node identities, which
would remove path-step interning altogether. The current exact key and the residual hash-table
considerations are documented in [security/hashing-and-collisions](../security/hashing-and-collisions.md).

## 4. Concurrency model

Registries are shared and mutated during lazy expansion, so each is wrapped in
`Arc<RwLock<…>>`:

- **reads** (resolve a node/state by id) take a read lock — many can proceed concurrently;
- **writes** (register a newly seen child node/state) take a write lock — exclusive, brief.

`Arc` makes a whole state source cheap to **clone** (clones share the registries), which matters
because `Wfst: Clone` and composition clones operands. The per-WFST transition cache
([architecture/04](04-lazy-evaluation-and-caching.md)) is *not* shared — it is guarded by `&mut self`
— so only the registries cross threads.

### Lock poisoning

If a thread panics while holding a registry lock, Rust marks the lock as **poisoned**. duallity
acquires registry locks through crate-local `read_lock` / `write_lock` helpers, which recover the
inner guard with `PoisonError::into_inner()` instead of turning every later acquisition into a second
panic. This keeps the lazy state graph usable after a caller-side thread failure while preserving
Rust's memory-safety guarantees.

For write-heavy workloads, lock-free registries (e.g. a concurrent map or a persistent structure)
remain a compatible alternative; see
[engineering/concurrency-and-locking](../engineering/concurrency-and-locking.md).
