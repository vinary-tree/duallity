# Security

duallity is a pure computation library — it parses no untrusted formats, opens no sockets, and
deserializes nothing. Its security surface is therefore narrow and is dominated by **resource
exhaustion** (denial of service). These pages also document the exact dictionary-node key used by the
registries and the residual hash-table considerations around it.

| Document | Covers |
|----------|--------|
| [Threat model](threat-model.md) | What the inputs are, what the (small) attack surface is, and how to bound resource use. |
| [Hashing and collisions](hashing-and-collisions.md) | The exact dictionary-node key, why ordinary hash-table collisions do not alias nodes, and future hardening options. |

## Summary

- **No I/O, no network, no deserialization** — duallity only computes over a query string and an
  in-memory dictionary you supply.
- **No `unsafe`** ([engineering/safety-and-panics](../engineering/safety-and-panics.md)) — no memory-safety
  surface of duallity's own making.
- The realistic concern is **algorithmic resource use** at large `k` / large dictionaries; bound it
  with `max_distance` and `CachePolicy::Lru` (see [threat-model](threat-model.md)).
- Dictionary nodes use a compact exact `DictionaryNodeKey`; ordinary hash-table collisions do not
  alias nodes and are discussed in [hashing-and-collisions](hashing-and-collisions.md).
