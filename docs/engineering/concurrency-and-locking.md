# Concurrency and locking

duallity's concurrency model is small and explicit: per-WFST caches are `&mut self`-private, and the
only shared mutable state is a set of interning **registries**, each behind an `Arc<RwLock>`.

## 1. What is shared and what is not

| State | Sharing | Guard |
|-------|---------|-------|
| transition cache (`FxHashMap<StateId, CachedState>`) | **not shared** — one per WFST | `&mut self` |
| `DictionaryNodeRegistry`, `DepthDictionaryNodeRegistry`, `UniversalStateRegistry`, `ProductStateRegistry`, `NfaStateRegistry` | shared (cloned WFSTs share them) | `Arc<RwLock<…>>` |

Because the cache is private, expanding a state mutates only `self`. The registries are the sole
cross-thread surface, and they exist because dictionary nodes and abstract automaton states need
**stable, shared ids** ([architecture/05](../architecture/05-registries-and-interning.md)).

## 2. The read/write lock dance

During `compute_transitions`, a state source:

1. takes a **read lock** to resolve the current node/state by id (many readers may proceed at once);
2. for each newly seen child, takes a **write lock** briefly to register it and mint its id.

```text
compute_transitions(dict_node_id, …):
    registry.read()  → resolve dict_node                     ▷ shared read
    for each edge (c, child):
        if child absent from registry:
            registry.write() → register child, assign id     ▷ exclusive, brief
```

Reads dominate; writes happen only on the *first* visit to each node/state, after which the id is
cached in the registry and subsequent visits are read-only. This keeps contention low even under
parallel query processing.

## 3. Cheap clones via `Arc`

Every registry lives behind `Arc`, so cloning a WFST (which `Wfst: Clone` and `compose` both do)
**shares** the registries rather than duplicating them. The query characters are likewise an
`Arc<Vec<char>>`. A clone is therefore a few `Arc` bumps plus a fresh (empty or copied) cache — not a
deep copy of the dictionary or the interned state.

## 4. Lock poisoning

If a thread panics while holding a registry lock, Rust marks the lock as **poisoned**. duallity's
registry acquisitions go through crate-local helpers:

```rust,ignore
fn read_lock<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn write_lock<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(|poisoned| poisoned.into_inner())
}
```

The helpers recover the inner guard instead of turning every later acquisition into a second panic.
That behavior is appropriate for these registries because entries are append-only/idempotent
interning tables: once an id has been assigned, later reads can continue to use the stored vectors
and maps, and later writes can continue assigning fresh ids.

## 5. Lock-free registry variants

The `RwLock` registries are the one place duallity blocks. Replacing them with **lock-free** or
**persistent** structures — a concurrent hash map (e.g. sharded/CAS-based) or a persistent trie for
the id maps — would:

- improve write-heavy parallelism (many threads interning new nodes at once);
- align with the project's preference for non-blocking, persistent data structures.

This is an alternative architecture for write-heavy workloads. The cache eviction policy
([architecture/04](../architecture/04-lazy-evaluation-and-caching.md)) is a separate, independent
dimension that does not touch the locking model.

## See also

- [architecture/05 · Registries and interning](../architecture/05-registries-and-interning.md)
- [engineering/safety-and-panics](safety-and-panics.md)
- [security/hashing-and-collisions](../security/hashing-and-collisions.md)
