# duallity — C++ binding

A header-only **RAII** (**R**esource **A**cquisition **I**s **I**nitialization) wrapper over duallity's
seven-function C ABI. It turns a dictionary resource into a lazy, composable **WFST** (**W**eighted
**F**inite-**S**tate **T**ransducer) resource with move-only handles and exception-based errors, so
lifetimes are managed by scope rather than by hand.

The wrapper is [`include/duallity.hpp`](../../include/duallity.hpp) (it includes the C header
[`include/duallity.h`](../../include/duallity.h)). This README is the C++-specific guide; the full
cross-language walkthrough is [docs/guides/07](../../docs/guides/07-language-bindings.md) and the ABI
reference is [docs/architecture/06](../../docs/architecture/06-resource-abi-and-bindings.md).

## Surface

Everything lives in namespace `vinary_tree::duallity`:

| Type / function | Role |
|-----------------|------|
| `class wfst` | owns a `DuallityWfst*`; frees it in its destructor |
| `class resource` | owns one `VtResource` retain; releases it in its destructor |
| `class error : std::runtime_error` | thrown on any non-`OK` status; `.status()` returns the `DuallityStatus` |
| `void check(DuallityStatus)` | throws `error` unless the status is `OK` |

Both `wfst` and `resource` are **move-only** (copy is deleted), so a handle has exactly one owner.

## Install / build

Put `duallity.hpp` and `duallity.h` on the include path and link the `duallity` library. `duallity.h`
pulls in the interop header `vinary_tree_interop.h`; override the name with the `VT_INTEROP_HEADER`
macro if your build vends it elsewhere. On Windows, define `DUALLITY_USING_DLL` when consuming a DLL.
The header requires C++17 (`std::string_view`, `[[nodiscard]]`). Native packages and a pkg-config file
are staged by [`scripts/stage-native-package.sh`](../../scripts/stage-native-package.sh).

## Quickstart

```cpp
#include "duallity.hpp"
using namespace vinary_tree::duallity;

// `dict` is a VtResource implementing vt.dictionary.v1 (UnicodeScalar units),
// obtained from your dictionary library. It is borrowed, not consumed.
VtResource dict = /* … your vt.dictionary.v1 provider … */;

try {
    wfst edit(dict, "helo", /*maximum_distance=*/2,
              DUALLITY_ALGORITHM_STANDARD, DUALLITY_WFST_LEVENSHTEIN);

    // Hand a retained, composable resource to lling-llang.
    resource composable = edit.retained_resource();
    // … lling_llang::compose(composable.get(), language_model) …

    // `composable` releases on scope exit; `edit` frees on scope exit.
} catch (const error& e) {
    // e.what() is the boundary message; e.status() is the DuallityStatus.
}
```

The constructor's `algorithm` (default `DUALLITY_ALGORITHM_STANDARD`) is consumed by the Levenshtein
kind only; `kind` (default `DUALLITY_WFST_LEVENSHTEIN`) selects one of the nine adapters — see
[architecture/06 §4](../../docs/architecture/06-resource-abi-and-bindings.md#4-the-nine-automaton-kinds-and-their-algorithms).

## Ownership and memory model

RAII binds the two C lifecycles to scope:

- a `wfst` frees its `DuallityWfst*` exactly once, in its destructor;
- a `resource` releases its one `VtResource` retain in its destructor;
- the two are **independent** — destroying the `wfst` does not release resources already obtained via
  `retained_resource()`, and vice versa.

The `dict` argument is borrowed for the constructor call only. duallity captures its snapshot once
([the capture-once rule](../../docs/architecture/06-resource-abi-and-bindings.md#5-the-capture-once-rule)),
so the `wfst` — and any `resource` taken from it — keeps matching against that immutable revision even
after you release `dict`.

## Errors

`check()` converts any non-`OK` status into a thrown `error` carrying `duallity_last_error_message()`
and the `DuallityStatus`. The status mapping is **total**
([error-mapping totality](../../docs/guides/07-language-bindings.md#5-error-mapping-totality)):
`INCOMPATIBLE_RESOURCE` for a non-`UnicodeScalar` dictionary or stale ABI, `PROVIDER_ERROR` for a
misbehaving provider, `INVALID_ARGUMENT` for an out-of-range selector or a `` $`k > 255`$ `` distance on
a universal/generalized kind, and `PANIC` if a Rust panic was caught at the boundary. Because the
boundary is caught, an exception here is a *reported* error, never undefined behavior.

## Concurrency and zero-copy

- **Distinct handles are independent**; do not free/release the *same* handle from two threads.
- **The produced resource is reentrant** — concurrent expansion shares the registries and snapshot
  behind reference-counted structural sharing.
- **Capture and handoff are `` $`O(1)`$ ``**; expansion is lazy and provider buffers are leased for the
  duration of one callback ([architecture/06 §7](../../docs/architecture/06-resource-abi-and-bindings.md#7-provider-fault-handling-and-validation)).
- `duallity_last_error_message()` is thread-local, so `error::what()` reflects this thread's failure.

## Version compatibility

Negotiate with `duallity_abi_version()` (currently `1`) and `duallity_api_revision()` (currently `1`)
at load time; refuse a major you do not understand. This binding tracks crate `duallity 0.3.0`
(**MSRV 1.95**) and `vinary-tree-interop 0.1.0` (ABI version `1`). The living version record is the
[bindings findings ledger](../../docs/scientific-ledger/bindings-findings-ledger.md).

## See also

- [docs/guides/07 · Language bindings](../../docs/guides/07-language-bindings.md) — the nine-section cross-language guide.
- [docs/architecture/06 · The resource ABI and language bindings](../../docs/architecture/06-resource-abi-and-bindings.md) — the ABI reference.
- [bindings/javascript/README](../javascript/README.md) — the JS / TS / cljs facade.
