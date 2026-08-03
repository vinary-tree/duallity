# Security

duallity is a **pure computation library**. It parses no untrusted formats, opens no sockets, spawns
no processes, and deserializes nothing: it computes weighted edit-distance paths over a query string
`` $`q`$ `` and an in-memory dictionary `` $`D`$ `` that the host supplies, and returns ranked
corrections. Consequently its attack surface is narrow and is dominated by a single class of concern —
**algorithmic resource exhaustion** (denial of service).

These pages state that surface precisely, bound the one realistic risk with concrete knobs, and
document the exact dictionary-node key together with the residual hash-table considerations around it.

| Document | Covers |
|----------|--------|
| [Threat model](threat-model.md) | The inputs and trust boundary, the (small) attack surface, and how to bound resource use with concrete numeric limits. |
| [Hashing and collisions](hashing-and-collisions.md) | The exact `DictionaryNodeKey`, why ordinary hash-table collisions cannot alias two dictionary nodes, and future hardening options. |
| [fzf resource bounds](fzf-resource-bounds.md) | Query/candidate ceilings, prefix-shared DP growth, top-k heap use, and path-sensitive lazy-state expansion. |

## STRIDE-lite assessment

[**STRIDE**](https://en.wikipedia.org/wiki/STRIDE_model) (Kohnfelder & Garg, Microsoft, 1999) is a
mnemonic for six threat categories: **S**poofing, **T**ampering, **R**epudiation, **I**nformation
disclosure, **D**enial of service, and **E**levation of privilege. It is a *system*-level framework;
applied to a self-contained, side-effect-free compute library, five of its six categories are
**structurally not applicable** because the mechanisms they describe (identities, persisted or
transmitted artifacts, audit logs, secret channels, privilege boundaries) simply do not exist inside
duallity. The sixth — denial of service — is the whole of the practical surface.

| STRIDE category | Applies to duallity? | Rationale |
|-----------------|----------------------|-----------|
| **S**poofing (falsifying identity) | **N/A** | duallity authenticates nothing and holds no principals, sessions, tokens, or credentials. There is no identity to spoof. |
| **T**ampering (violating integrity) | **N/A** in the library | With no persistence, no I/O, and no deserialization there is no on-disk or on-wire artifact whose integrity duallity could fail to protect. The one *logical* integrity question — could two distinct dictionary nodes be conflated? — is closed by the exact key ([hashing-and-collisions](hashing-and-collisions.md)). In-memory tampering is the host's memory-safety domain, and duallity adds **no `unsafe`** ([engineering/safety-and-panics](../engineering/safety-and-panics.md)). |
| **R**epudiation (denying an action) | **N/A** | duallity keeps no logs or transactions to repudiate; it is a deterministic pure function of `` $`(q, D, \text{config})`$ ``. Its [determinism](threat-model.md#6-determinism-and-reproducibility) actively *aids* the host's own auditing and issue reproduction. |
| **I**nformation disclosure (leaking data) | **N/A** | duallity processes strings, not secrets, and opens no channel over which to leak. It is not constant-time, but its running time is a function of **public** inputs only (the query, the dictionary, and `` $`k`$ ``); it branches on no secret, so timing reveals nothing sensitive. |
| **D**enial of service (exhausting resources) | **Yes — the one real risk** | An adversary who controls the query (and possibly influences the dictionary) can try to inflate the explored product-state band. This is the subject of [threat-model §3–§4](threat-model.md#3-the-realistic-risk-resource-exhaustion-dos) and is bounded by `max_distance`, query length, and `CachePolicy::Lru`. |
| **E**levation of privilege (gaining rights) | **N/A** | No privilege boundary is crossed: no `unsafe`, no FFI into privileged code, no process or shell invocation, and no dynamic code loading. duallity runs entirely within the host's existing privileges. |

The single live row — **denial of service** — is not left as an abstract worry: every vector is tied
to a concrete, enforceable bound in the [threat model's per-vector mitigation table](threat-model.md#4-per-vector-mitigations)
and, beneath those tunable knobs, to the crate's **hard structural ceilings** (the `u32` `StateId` and
`VocabId` spaces, which *fail closed* rather than overflow).

## Summary

- **No I/O, no network, no deserialization.** duallity only computes over a query string and an
  in-memory dictionary you supply; there is no parsing-of-untrusted-bytes, SSRF, path-traversal, or
  deserialization-gadget surface of its own.
- **No `unsafe`** ([engineering/safety-and-panics](../engineering/safety-and-panics.md)) — the crate
  introduces no memory-safety surface of its own making, and every fallible step reports through
  `Result`/`Option` rather than a wrapping overflow.
- **The realistic concern is algorithmic resource use** at large `` $`k`$ `` / large dictionaries.
  Bound it with a small `max_distance`, a query-length cap at the host boundary, and
  `CachePolicy::Lru` (see [threat-model](threat-model.md)).
- **Dictionary nodes use a compact exact `DictionaryNodeKey`.** Ordinary hash-table collisions do not
  alias nodes; the packing and its non-aliasing proof are in
  [hashing-and-collisions](hashing-and-collisions.md).

## See also

- [threat-model](threat-model.md) — inputs, attack surface, and resource bounds.
- [hashing-and-collisions](hashing-and-collisions.md) — the exact node key and its collision behavior.
- [engineering/safety-and-panics](../engineering/safety-and-panics.md) — the zero-`unsafe` story and the complete panic inventory.
- [guides/05 · Performance and tuning](../guides/05-performance-and-tuning.md) — the same knobs, framed for performance.
