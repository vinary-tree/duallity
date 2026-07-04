# Hashing and collisions

duallity gives dictionary nodes stable integer ids without relying on probabilistic path hashes. This
page explains the current exact key, the difference between logical node identity and ordinary
hash-table lookup, and the remaining hardening path if dictionaries eventually expose native node ids.

## 1. The mechanism

Dictionary nodes arrive from libdictenstein without inherent integer ids, but a `StateId` needs a
`u32` node component ([architecture/03](../architecture/03-state-encoding-and-product-space.md)).
`DictionaryNodeRegistry` and its depth-tracking universal variant therefore key a child node by the
exact `(parent id, edge label)` step that led to it. The key is compact, not probabilistic:

```rust,ignore
pub(crate) struct DictionaryNodeKey(u64);

impl DictionaryNodeKey {
    pub(crate) const ROOT: Self = Self(u64::MAX);

    pub(crate) fn child(parent_id: u32, edge_label: char) -> Self {
        let codepoint = edge_label as u64;
        Self((u64::from(parent_id) << 21) | codepoint)
    }
}
```

<img src="../diagrams/noderegistry-interning.svg" alt="A node is keyed by its exact parent and edge path step into a u32 id; ordinary hash-table collisions do not alias nodes" width="820"/>

## 2. Why the key is exact

The packing formula is:

```text
child_key = (u64(parent_id) << 21) | u64(edge_label)
root_key  = u64::MAX
```

A Rust `char` is a Unicode scalar value, so `u64(edge_label) ≤ 0x10FFFF < 2²¹`. A packed child key
therefore uses at most `32 + 21 = 53` bits, while `root_key = 2⁶⁴ - 1`. Two child keys compare equal
only when both their `parent_id` and `edge_label` are equal.

| Property | Status |
|----------|--------|
| Logical node key | exact packed `(parent id, edge label)` plus root sentinel |
| Key width | 8 bytes |
| Hash-table implementation | `FxHashMap<DictionaryNodeKey, u32>` |
| Consequence of ordinary hash-table collision | bucket probe plus `Eq` check; no node aliasing |
| Memory safety affected? | **No** — the crate remains safe Rust |

## 3. Residual considerations

- `FxHashMap` is still a non-cryptographic hash table. Its internal hash collisions affect lookup
  cost, not correctness, because keys are compared for equality after hashing.
- The dictionary itself is still treated as trusted, host-built input
  ([threat-model](threat-model.md)). Untrusted callers should be bounded by query length,
  `max_distance`, and cache policy rather than by hash-table assumptions.
- The exact path-step key identifies nodes by traversal path. A future dictionary API with native
  stable node ids would be even better for shared suffixes in minimized DAWGs because it could intern
  by the dictionary's intrinsic state identity.

## 4. Threat consideration

The previous path-hash design could, in principle, alias two distinct dictionary nodes if their path
hashes collided. The packed `DictionaryNodeKey` removes that logical aliasing failure mode. The
remaining adversarial concern is algorithmic resource use: extremely large dictionaries, high
`max_distance`, or very long queries can still expand many states. That concern is handled by the
same controls as the rest of the crate: bounded edit distance, bounded query length, and LRU cache
policies.

## 5. Hardening

The current implementation already uses option (2) from the old risk analysis: exact path-step keys.
Further hardening is therefore about reducing overhead or improving sharing, not repairing a
correctness hazard:

1. **True node identities (preferred).** Have the dictionary backend expose a stable, collision-free
   node id (e.g. a DAWG state index), and key the dictionary-node registry on that. This would
   preserve suffix sharing that path-step interning cannot see.
2. **Concurrent registries.** Replace `Arc<RwLock<FxHashMap<...>>>` with a concurrent map if profiling
   shows write-lock contention during parallel lazy expansion.
3. **Hasher choice.** Keep `FxHashMap` for speed in trusted dictionaries, or switch to a keyed hasher
   if future embedding contexts expose dictionary construction to adversaries.

Tracked alongside the lock-free registry work in
[engineering/concurrency-and-locking](../engineering/concurrency-and-locking.md).

## See also

- [architecture/05 · Registries and interning](../architecture/05-registries-and-interning.md)
- [security/threat-model](threat-model.md)
