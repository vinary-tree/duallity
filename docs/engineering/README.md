# Engineering

The engineering view of duallity: its safety story, its concurrency model, and how it is tested.
These pages document *properties of the implementation* rather than the matching algorithms.

| Document | Covers |
|----------|--------|
| [Safety and panics](safety-and-panics.md) | Zero `unsafe`; panic boundaries; `Send`/`Sync` bounds. |
| [Concurrency and locking](concurrency-and-locking.md) | `Arc<RwLock>` registries, the read/write dance, poison recovery, and cheap clones. |
| [Testing](testing.md) | The unit and integration test map, the label-preservation tests, and how to add a variant test. |

## Headline properties

- **Zero `unsafe`** across every module.
- Production failure paths use `Result`/`Option`; direct panic macros are not part of the public
  error surface.
- Registry lock poisoning is recovered by taking the inner guard, so a prior thread failure does not
  turn future read/write acquisitions into fresh panics.
- Every WFST is `Clone + Send + Sync`, so it composes and parallelizes freely.
- Preallocation throughout (`SmallVec` transition buffers, sized registries and caches).
