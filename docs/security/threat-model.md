# Threat model

This page states what duallity is exposed to, what it is **not** exposed to, and how to bound the one
realistic risk — resource exhaustion.

## 1. Trust boundary and inputs

duallity is a library embedded in a host application. Its only inputs are:

| Input | Source | Notes |
|-------|--------|-------|
| the **query** string | often untrusted (e.g. a user's search box) | bounded-length text |
| the **dictionary** | usually trusted (built by the host) | an in-memory libdictenstein container |
| configuration (`max_distance`, `CachePolicy`, weights, rules) | the host | controls resource use |

duallity performs **no I/O**: it opens no files or sockets, makes no network calls, spawns no
processes, and **deserializes nothing**. There is therefore no parsing-of-untrusted-bytes surface, no
SSRF/path-traversal surface, and no deserialization-gadget surface of duallity's own.

## 2. What is *not* in scope

- **Memory-safety exploits** — duallity contains no `unsafe`
  ([engineering/safety-and-panics](../engineering/safety-and-panics.md)); it has no raw-pointer or
  uninitialized-memory surface.
- **Injection** — there is no query language, template engine, or shell invocation.
- **Secrets** — duallity holds none; it processes strings, not credentials.

## 3. The realistic risk: resource exhaustion (DoS)

The genuine concern is **algorithmic**: an adversary who controls the query (and possibly influences
the dictionary) can try to make a single correction expensive.

| Vector | Mechanism | Bound it with |
|--------|-----------|---------------|
| large `k` | the reachable state band widens with `2k+1`, and at large `k` the "wall effect" forces wide exploration ([theory/06](../theory/06-wallbreaker-and-the-wall-effect.md)) | cap `max_distance` to a small constant for untrusted input; use [`WallBreakerWfst`](../design/wallbreaker-wfst.md) when large `k` is genuinely required |
| unbounded cache growth | `CachePolicy::CacheAll` keeps every visited state | use `CachePolicy::Lru { max_states }` / `set_max_cache_size(n)` for long-lived services ([guides/05](../guides/05-performance-and-tuning.md)) |
| long queries | state count scales with `(n+1)·(2k+1)` | bound query length at the host boundary |
| huge result sets (WallBreaker) | construction runs the full query eagerly and stores every match | cap `max_distance`; check `num_results()` and treat very large counts as suspicious |

### Recommended posture for untrusted queries

```rust,ignore
use lling_llang::prelude::*;

// Small, fixed distance bound and a bounded cache for adversarial input.
let mut lev = duallity::LevenshteinWfst::new(&dict, untrusted_query_capped_in_length, 2);
lev.set_cache_policy(CachePolicy::Lru { max_states: 50_000 });
```

- Keep `max_distance` small (1–2) for interactive, untrusted queries.
- Bound the query length before constructing the WFST.
- Prefer a bounded `CachePolicy` for any long-lived process.
- For large-`k` features, gate them behind authenticated/trusted call paths.

## 4. Determinism and reproducibility

duallity is deterministic: the same query, dictionary, and configuration always produce the same
weighted paths. There is no nondeterminism from threading (the registries are linearizable under their
locks) — useful for auditability and for reproducing reported issues.

## See also

- [guides/05 · Performance and tuning](../guides/05-performance-and-tuning.md) (the same knobs, for performance)
- [security/hashing-and-collisions](hashing-and-collisions.md)
- [engineering/safety-and-panics](../engineering/safety-and-panics.md)
