# Safety and panics

duallity is engineered for a clean safety story. This page states it precisely and gives the
**complete inventory** of places the crate can panic — so callers know exactly what they are exposed
to.

## 1. Zero `unsafe`

Every module in `src/` is written in safe Rust. There is **no `unsafe`** anywhere in the crate. The
performance comes from data-structure choices (`FxHashMap`, `SmallVec`, `Arc`) and laziness, not from
unchecked code.

## 2. No direct panic or body-replacement macros

Production code contains **no** direct panic macro invocations and no macros that substitute for a
real implementation body. Fallible internal steps use `Result` or `Option`; public builders report
caller errors through `Result` (e.g. `build()` returns `Err("Query not set")`, and invalid weights return
`InvalidWeightError`).

## 3. Panic boundaries

Registry lock poisoning is **not** a production panic boundary. Each registry is an `Arc<RwLock<…>>`
([architecture/05](../architecture/05-registries-and-interning.md)), but acquisitions go through
crate-local helpers that recover poisoned guards:

```rust,ignore
lock.read().unwrap_or_else(|poisoned| poisoned.into_inner())
lock.write().unwrap_or_else(|poisoned| poisoned.into_inner())
```

The remaining production panic boundary is an explicit state-space invariant check:

| Boundary | Condition | Caller-facing alternative |
|----------|-----------|---------------------------|
| state-space construction | a requested WFST product exceeds `u32` `StateId` capacity | reduce query length, edit bound, or operation set |

The state-encoding API is fallible: `state_encoding::try_encode` returns `None` for a zero radix,
out-of-range component, or `StateId` overflow. All `.expect` / `.unwrap` occurrences are inside
`#[cfg(test)]` modules or ignored doc examples.

Weight validation is also fallible rather than panic-based: phonetic weights, edit weights, and
rewrite-rule costs must be finite, non-negative `f64` values, and the public constructors/builders
reject invalid values with `InvalidWeightError`.

## 4. `Send + Sync` and `Clone`

Every WFST and state source is bounded `Clone + Send + Sync`, with the dictionary node and unit types
carried as `Send + Sync` (and, for the char variants, `Into<char> + TryFrom<char> + Copy`). The
practical consequences:

- a WFST can be **moved across threads** and **shared** behind an `Arc`;
- a WFST **clones cheaply** — registries are behind `Arc`, so a clone shares them rather than copying
  ([architecture/05](../architecture/05-registries-and-interning.md));
- `compose` (which clones operands) and data-parallel query processing are sound by construction.

## 5. Weight-domain safety

`TropicalWeight` rejects `NaN`/`-∞` at construction (it admits `ℝ ∪ {+∞}`), so a weight is always a
well-formed tropical value; `lling_llang` checks the semiring laws against a machine-verified model.
The only subtlety is the [`zero()` = `+∞` / `one()` = `0` naming](../theory/01-semirings-and-wfsts.md#3-the-tropical-min--semiring),
which is a *readability* hazard, not a safety one.

## See also

- [architecture/05 · Registries and interning](../architecture/05-registries-and-interning.md)
- [engineering/concurrency-and-locking](concurrency-and-locking.md)
- [security/threat-model](../security/threat-model.md)
