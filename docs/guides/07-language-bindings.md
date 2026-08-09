# 07 · Language bindings — the C ABI and the JavaScript facade

This guide is the task-oriented companion to the reference in
[architecture/06 · The resource ABI and language bindings](../architecture/06-resource-abi-and-bindings.md).
It shows how to *use* duallity from outside Rust: through the stable **C ABI** (**A**pplication **B**inary
**I**nterface) and through the **JavaScript / TypeScript / ClojureScript** facade published as
[`@vinary-tree/duallity`](../../bindings/javascript/README.md). A separate C++ header
([`include/duallity.hpp`](../../include/duallity.hpp)) provides a thin **RAII** (**R**esource
**A**cquisition **I**s **I**nitialization) wrapper over the C ABI; it is documented in the
[C++ binding README](../../bindings/cpp/README.md).

Every binding is a thin skin over the same seven `duallity_*` C functions, so this one guide follows the
**nine-section per-language template** the vinary-tree family uses for each of them:

| # | Section | What it answers |
|---|---------|-----------------|
| 1 | [Surface summary](#1-surface-summary) | what the binding exposes |
| 2 | [Install](#2-install) | how to depend on it |
| 3 | [Runnable quickstart](#3-runnable-quickstart) | the shortest end-to-end program |
| 4 | [Ownership and memory model](#4-ownership-and-memory-model) | who frees what, and when |
| 5 | [Error-mapping totality](#5-error-mapping-totality) | every failure a call can report |
| 6 | [Concurrency truth](#6-concurrency-truth) | what is safe to call from where |
| 7 | [Zero-copy paths](#7-zero-copy-paths) | where nothing is serialized or copied |
| 8 | [Troubleshooting](#8-troubleshooting) | the common mistakes and their symptoms |
| 9 | [Version compatibility](#9-version-compatibility) | which versions interoperate |

## 1. Surface summary

The binding takes a **dictionary** — a `VtResource` implementing the `vt.dictionary.v1` interface,
built by any conforming library — plus a **query**, an **edit bound** `` $`k`$ ``, an **algorithm**,
and a **kind**, and returns a lazy **WFST** (**W**eighted **F**inite-**S**tate **T**ransducer) exposed
as a `vt.scalar-wfst.1` resource. That resource composes in `` $`O(1)`$ `` with any other lling-llang
transducer — a language model, a phonetic rewriter — with no serialization.

- **C ABI** — seven functions in [`include/duallity.h`](../../include/duallity.h):
  `duallity_abi_version`, `duallity_api_revision`, `duallity_last_error_message`, `duallity_wfst_new`,
  `duallity_wfst_resource`, `duallity_wfst_free`, `duallity_resource_release`.
- **JavaScript facade** — [`@vinary-tree/duallity`](../../bindings/javascript/README.md) exposes
  `wfst(dictionary, query, maximumDistance, algorithm?, kind?)` returning a `WfstResource`, plus a
  `runtimeIdentity` guard. TypeScript types ship in `index.d.ts`; a ClojureScript namespace
  (`vinary-tree.duallity`, functions `wfst` / `start` / `state` / `close!`) ships alongside.
- **C++ facade** — [`include/duallity.hpp`](../../include/duallity.hpp) wraps the C ABI in
  `vinary_tree::duallity::{wfst, resource, error}` with move-only handles and exception-based errors.

The nine WFST **kinds** and four edit **algorithms** are enumerated in
[architecture/06 §4](../architecture/06-resource-abi-and-bindings.md#4-the-nine-automaton-kinds-and-their-algorithms).

## 2. Install

**C / C++.** Link the `duallity` static or shared library and put both headers on the include path;
`duallity.h` includes the interop header `vinary_tree_interop.h` (override the name with the
`VT_INTEROP_HEADER` macro if your build vends it elsewhere). Define `DUALLITY_USING_DLL` when consuming
a Windows DLL. A pkg-config file and staged native packages are produced by
[`scripts/stage-native-package.sh`](../../scripts/stage-native-package.sh).

**JavaScript.** Install the scoped package and the interop peer:

```sh
npm install @vinary-tree/duallity @vinary-tree/interop
```

The facade depends on `@vinary-tree/vinary-tree` (the umbrella native/WASM runtime) and requires
**Node 22.14 or newer**. It exposes native (N-API), WASM, and WASI-preview-1 entry points; Node
defaults to the native N-API build.

## 3. Runnable quickstart

**C++ (RAII facade).** The `wfst` handle frees itself; the `resource` handle releases itself.

```cpp
#include "duallity.hpp"
using namespace vinary_tree::duallity;

// `dict` is a VtResource implementing vt.dictionary.v1, obtained from your
// dictionary library (e.g. libdictenstein's C ABI). It is borrowed, not consumed.
VtResource dict = /* … your vt.dictionary.v1 provider … */;

wfst edit(dict, "helo", /*maximum_distance=*/2,
          DUALLITY_ALGORITHM_STANDARD, DUALLITY_WFST_LEVENSHTEIN);

// Hand the composable WFST resource to lling-llang; it owns one retain.
resource composable = edit.retained_resource();
// … lling_llang::compose(composable.get(), language_model) …
// `composable` releases on scope exit; `edit` frees on scope exit.
```

**C (raw ABI).** The same flow, managing lifetimes by hand:

```c
#include "duallity.h"

VtResource dict = /* … your vt.dictionary.v1 provider … */;
DuallityWfst *edit = NULL;
DuallityStatus st = duallity_wfst_new(
    dict, (const uint8_t*)"helo", 4 /*bytes*/, 2 /*k*/,
    DUALLITY_ALGORITHM_STANDARD, DUALLITY_WFST_LEVENSHTEIN, &edit);
if (st != DUALLITY_STATUS_OK) { /* inspect duallity_last_error_message() */ }

VtResource composable = {0};                       /* one retain on success */
st = duallity_wfst_resource(edit, &composable);
/* … compose composable with a downstream WFST … */

duallity_resource_release(composable);            /* release the retain */
duallity_wfst_free(edit);                          /* free the handle    */
```

**JavaScript.** The facade returns a resource that composes with the rest of the family in-process:

```js
import { wfst } from "@vinary-tree/duallity";

// `dictionary` is a DictionaryResource from a @vinary-tree dictionary package.
const edit = wfst(dictionary, "helo", 2, "standard", "levenshtein");
// … compose `edit` with a downstream WFST via @vinary-tree/lling-llang …
edit.close();   // release the retained resource (or use the cljs `close!`)
```

## 4. Ownership and memory model

Two owning things flow across the boundary, and they have **separate lifecycles**:

| Owned thing | Created by | Released by | Notes |
|-------------|-----------|-------------|-------|
| `DuallityWfst*` handle | `duallity_wfst_new` | `duallity_wfst_free` (null-safe) | free exactly once |
| `vt.scalar-wfst.1` `VtResource` | `duallity_wfst_resource` | `duallity_resource_release` (null-safe) | one retain per call; may **outlive** the handle |

The **dictionary** argument is **borrowed for the call only**: duallity takes its own retain of the
*snapshot* (the capture-once rule, [architecture/06 §5](../architecture/06-resource-abi-and-bindings.md#5-the-capture-once-rule)),
so you may free your dictionary immediately after `duallity_wfst_new` returns and the WFST keeps
matching against the captured revision. The C++ and JS facades bind these rules to scope/GC: the C++
`wfst` and `resource` are move-only and release in their destructors; the JS resource releases on
`close()` (finalizers are a backstop, not a guarantee — call `close()`).

The one easy mistake is conflating the two: **freeing the handle does not release resources already
handed out**, and releasing the resource does not free the handle. Balance each independently.

## 5. Error-mapping totality

Every fallible C call returns a `DuallityStatus`; the facades convert a non-`OK` status into an
idiomatic error (a thrown `vinary_tree::duallity::error` in C++, a thrown `Error` carrying the boundary
message in JS) while preserving the code. The mapping is **total** — every variant below is a defined
outcome, and [`proofs/coq/StatusMapping.v`](../../proofs/coq/StatusMapping.v) machine-checks the
internal-error to status map:

| `DuallityStatus` | C `#define` | JS surfaces as | Cause |
|------------------|-------------|----------------|-------|
| `OK` (`0`) | `DUALLITY_STATUS_OK` | resolved value | success |
| `INVALID_ARGUMENT` (`1`) | `DUALLITY_STATUS_INVALID_ARGUMENT` | thrown error | `algorithm > 3`, `kind > 8`, `u8`-overflowing distance, builder rejection |
| `INVALID_UTF8` (`2`) | `DUALLITY_STATUS_INVALID_UTF8` | thrown error | query bytes are not valid UTF-8 (JS strings are UTF-8-encoded before the call, so this is a C-side concern) |
| `NULL_POINTER` (`3`) | `DUALLITY_STATUS_NULL_POINTER` | thrown error | a required pointer/handle is null |
| `PANIC` (`4`) | `DUALLITY_STATUS_PANIC` | thrown error | a Rust panic was caught at the boundary |
| `INCOMPATIBLE_RESOURCE` (`5`) | `DUALLITY_STATUS_INCOMPATIBLE_RESOURCE` | thrown error | wrong ABI, no dictionary interface, or non-`UnicodeScalar` unit domain |
| `PROVIDER_ERROR` (`6`) | `DUALLITY_STATUS_PROVIDER_ERROR` | thrown error | a foreign dictionary callback failed or violated the contract |
| `LIMIT_EXCEEDED` (`7`) | `DUALLITY_STATUS_LIMIT_EXCEEDED` | thrown error | **reserved** — not produced at ABI v1 (see [architecture/06 §3](../architecture/06-resource-abi-and-bindings.md#3-status-code-totality)) |

`duallity_last_error_message()` returns a human-readable, thread-local string for the *most recent*
failing call on the calling thread; read it immediately after a non-`OK` status and before the next
`duallity_*` call on that thread.

## 6. Concurrency truth

- **Distinct handles are independent.** Constructing, expanding, freeing, or releasing *different*
  handles/resources concurrently is safe. Operating on the *same* handle/resource from two threads at
  once (especially free/release) is a use-after-free you must prevent, exactly as for any
  reference-counted handle.
- **The produced resource is reentrant.** Expanding the returned `vt.scalar-wfst.1` from multiple
  threads is safe: each `state` call clones a lightweight WFST shell while sharing the registries and
  dictionary snapshot behind `Arc` ([architecture/06 §6](../architecture/06-resource-abi-and-bindings.md#6-the-double-adapter-bridge)).
- **The provider gate is honored.** If your dictionary advertises `PARALLEL_REENTRANT`, its callbacks
  run without added locking; otherwise duallity serializes them through a per-provider mutex, so a
  non-reentrant dictionary is never entered concurrently
  ([architecture/06 §7](../architecture/06-resource-abi-and-bindings.md#7-provider-fault-handling-and-validation)).
- **Diagnostics are thread-local.** `duallity_last_error_message()` reads *this* thread's slot; a
  concurrent failure on another thread cannot clobber it.
- **Same-runtime handoff (JS).** The facade guards a `runtimeIdentity`: composing resources produced by
  the *same* underlying runtime (native, WASM, or WASI) is a zero-copy in-process handoff; crossing
  runtimes is refused rather than silently copying.

## 7. Zero-copy paths

duallity is built so the expensive things never happen:

- **Dictionary capture is `` $`O(1)`$ ``.** No terms are serialized or copied; the snapshot is a
  structural-sharing handle (the persistent-revision model of the capture-once rule).
- **Resource handoff is `` $`O(1)`$ ``.** `duallity_wfst_resource` returns a two-word `(context,
  vtable)` `VtResource` with one added retain — no graph is materialized or marshaled.
- **Expansion is lazy.** Product states are computed only as a shortest-path search visits them; the
  full `` $`(\text{dictionary} \times \text{automaton})`$ `` space is never built.
- **Provider buffers are leased.** Edge and arc pages are written into caller-owned contiguous storage
  and borrowed only for the duration of one callback — no per-edge allocation crosses the boundary.
- **Same-runtime JS handoff is copy-free.** Guarded by `runtimeIdentity`, a resource passes to
  lling-llang composition as a handle, not a serialized graph.

The one unavoidable copy is the **query**: its bytes are validated as UTF-8 and decoded into an
`Arc<[char]>` for per-scalar edit distance. That is `` $`O(n)`$ `` in the query length `` $`n`$ ``, not
in the dictionary size.

## 8. Troubleshooting

| Symptom | Likely cause | Fix |
|---------|--------------|-----|
| `INCOMPATIBLE_RESOURCE` on construct | dictionary is byte- or `u64`-domain, or a stale interop ABI | build a `UnicodeScalar` dictionary; align `vinary-tree-interop` versions across producer and consumer |
| `PROVIDER_ERROR` on construct or during search | a foreign dictionary callback failed or returned malformed output | validate your provider against `vt.dictionary.v1`; check `duallity_last_error_message()` |
| `INVALID_UTF8` | non-UTF-8 query bytes on the C side | pass UTF-8; JS strings are already UTF-8-encoded by the facade |
| `INVALID_ARGUMENT` with a large `` $`k`$ `` | universal/generalized kinds cap `maximum_distance` to `u8` | keep `` $`k \le 255`$ `` for those kinds, or use the Levenshtein kind |
| leaked memory | a handle freed but its handed-out resource never released (or vice versa) | balance `free` with the handle and `release` with each resource (§4) |
| stale error text | reading `duallity_last_error_message()` after a later `duallity_*` call | read it immediately after the failing call, on the same thread |
| JS `runtimeIdentity` mismatch on compose | mixing native and WASM resources | keep a pipeline on one runtime |

## 9. Version compatibility

A binding negotiates by calling `duallity_abi_version()` (currently `1`) at load time and refusing a
major it does not understand; `duallity_api_revision()` (currently `1`) advertises additive additions.
The interop layer evolves additively behind a `struct_size` vtable prefix, so a newer consumer safely
reads an older, shorter provider vtable.

| Component | Pinned version |
|-----------|----------------|
| crate `duallity` | `0.3.0` (edition 2021, **MSRV 1.95**) |
| `vinary-tree-interop` | `0.1.0` (ABI version `1`) |
| npm `@vinary-tree/duallity` | `0.3.0` |
| npm `@vinary-tree/interop` (peer) | `0.1.0` |
| npm `@vinary-tree/vinary-tree` (runtime) | `0.10.0` |
| Node engine | `>= 22.14` |

> **Release note.** As recorded in the
> [bindings findings ledger (DUAL-B2)](../scientific-ledger/bindings-findings-ledger.md), the family
> pins name `duallity 0.3.0` / `v0.3.0` ahead of the tag-and-publish event. Until that release lands,
> resolving `duallity = "0.3"` from crates.io or the `v0.3.0` git ref will not find published
> artifacts; build against the in-repo tree or a path/git dependency in the meantime.

## See also

- [architecture/06 · The resource ABI and language bindings](../architecture/06-resource-abi-and-bindings.md) — the reference this guide operationalizes.
- [bindings/javascript/README](../../bindings/javascript/README.md) · [bindings/cpp/README](../../bindings/cpp/README.md) — the per-language guides.
- [security/threat-model](../security/threat-model.md) — why a foreign dictionary is untrusted input, and how the boundary contains it.
- [guides/03 · Composing pipelines](03-composing-pipelines.md) — what to do with the WFST once you have it.
