# Engineering

The engineering view of duallity: its **safety** story, its **concurrency** model, and how it is
**tested**. Where the [theory](../theory/) and [design](../design/) sections describe *what the
matching algorithms compute*, these pages describe *properties of the implementation that hold
regardless of which variant you instantiate* — the invariants a caller can rely on when embedding the
crate in a larger, possibly multi-threaded, pipeline.

Every claim on these pages is traceable to a specific symbol in `src/`. The tables below are the
index: each headline property links both to the page that **proves** it and to the **source of
truth** that grounds it.

## The three engineering pages

| Document | Covers | New diagrams |
|----------|--------|--------------|
| [Safety and panics](safety-and-panics.md) | zero `unsafe`; the fallible (`Result`/`Option`) error surface; the complete panic-boundary inventory; `Send`/`Sync`/`Clone` bounds; weight-domain validity | `panic-safety-boundary` |
| [Concurrency and locking](concurrency-and-locking.md) | the private-cache / shared-registry split; the `Arc<RwLock>` read/write dance; poison recovery; cheap `Arc` clones; the lock-free/persistent future direction | `rwlock-lock-lifecycle` |
| [Testing](testing.md) | the unit + integration test map; the label-preservation contract with its `(input, output, weight)` triples; a coverage-by-surface table; how to add a variant test | `test-suite-map` |

## Properties at a glance

Each row is a promise the crate keeps, the page that **proves** it, and the `src/` **symbol** that
makes it true. All symbols are `pub` unless marked `pub(crate)`.

| Property | What it guarantees to a caller | Proven in | Grounded in (`src/`) |
|----------|-------------------------------|-----------|----------------------|
| **Zero `unsafe`** | no memory-safety obligation is delegated to the caller; the crate cannot exhibit undefined behaviour | [safety §1](safety-and-panics.md#1-zero-unsafe) | every module — `grep -rn unsafe src/` is empty |
| **Fallible by construction** | every partial operation returns `Result`/`Option`; no fallible step aborts the process | [safety §2](safety-and-panics.md#2-fallible-by-construction-result-and-option) | `state_encoding::{try_encode, decode}`, `DictionaryBackend::try_intern`, `validate_finite_nonnegative_weight` |
| **Panic-free production surface** | all `.expect`/`.unwrap`/`panic!` live under `#[cfg(test)]` or in doc examples; release paths use checked/saturating arithmetic and bounds-guarded indexing | [safety §3](safety-and-panics.md#3-the-panic-boundary-inventory) | `checked_mul`/`checked_add` in `state_encoding`, `saturating_*` throughout, `usize_from_u32` via `unwrap_or` |
| **Invalid weights rejected** | a `TropicalWeight` cost is always finite and non-negative; `NaN`, `` $`-\infty`$ ``, and negative values are refused at the constructor with a typed error | [safety §5](safety-and-panics.md#5-weight-domain-safety) | `InvalidWeightError`, `validate_finite_nonnegative_weight` in `lib.rs` |
| **`Clone + Send + Sync`** | every WFST and state source moves across threads, is shareable behind `Arc`, and clones cheaply; `compose` (which clones operands) and data-parallel querying are sound | [safety §4](safety-and-panics.md#4-send--sync-and-clone) | `DictionaryBackend<D>` bounds (`backend.rs`), `Wfst: Clone` re-export |
| **Poison recovery** | a panic in one thread while holding a registry lock does not turn every later acquisition into a fresh panic; interning continues | [concurrency §5](concurrency-and-locking.md#5-lock-poisoning-and-recovery) | `read_lock`/`write_lock` → `PoisonError::into_inner` (`lib.rs:267`) |
| **Private caches, shared registries** | per-WFST expansion mutates only `&mut self`; only the interning registries cross threads | [concurrency §1](concurrency-and-locking.md#1-shared-and-private-state) | `LazyStateCache` (`lazy_cache.rs`) vs `Arc<RwLock<…Registry>>` (`node_registry.rs`) |
| **Preallocation throughout** | collections are sized to a bounded estimate up front, so hot paths do not reallocate; speculative hints are capped | [architecture/04 §4](../architecture/04-lazy-evaluation-and-caching.md#4-cache-policy-and-deterministic-eviction) | `capped_size_hint_capacity`, `fx_hash_map_with_capacity`, `reserve_lru_capacity` (`lazy_cache.rs`), `Vec::with_capacity` (`node_registry.rs`, `backend.rs`) |
| **Executable semantics** | the transducer label orientation and per-variant costs are pinned by tests that fail loudly on regression | [testing §4](testing.md#4-the-label-preservation-tests-the-semantic-contract) | `*_transition_labels_preserve_transducer_sides`, `tests/*.rs` |

### How to read this table

- **"Proven in"** points at the prose + tables that argue the property from first principles.
- **"Grounded in"** names the exact code, so a reviewer can confirm the doc has not drifted from the
  implementation. When you change one of these symbols, update the linked page in the same commit.

## Design stance (the "why")

duallity deliberately trades a little raw speed for a **small, auditable trust surface**:

- **Safety over microoptimization.** No `unsafe` means the performance budget comes from data
  structures (`FxHashMap`, `SmallVec`, `Arc`) and laziness, not from unchecked code — see
  [architecture/04](../architecture/04-lazy-evaluation-and-caching.md).
- **Total functions over panics.** Anything a caller can get wrong (an over-large product space, an
  exhausted id space, an invalid weight) is a value (`None` / `Err`), never a crash. This is what lets
  duallity sit inside a long-running service without becoming its most fragile dependency.
- **Explicit, minimal sharing.** The only cross-thread state is a set of append-only interning
  registries; everything else is `&mut self`-private. That keeps the concurrency argument short enough
  to fit on [one page](concurrency-and-locking.md).

## See also

- [architecture/05 · Registries and interning](../architecture/05-registries-and-interning.md) — the
  shared state these pages reason about.
- [security/threat-model](../security/threat-model.md) — the adversarial framing of the same
  resource-bound and hashing considerations.
- [theory/README · Master notation](../theory/README.md#master-notation) — the single definition of
  every symbol (`` $`\oplus`$ ``, `` $`\otimes`$ ``, `` $`\bar{0}`$ ``, `` $`\bar{1}`$ ``) used below.
