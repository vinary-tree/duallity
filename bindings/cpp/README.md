# duallity — C++ binding

A header-only **RAII** (**R**esource **A**cquisition **I**s **I**nitialization) wrapper over duallity's
versioned C ABI. It turns a dictionary resource into a lazy, composable **WFST** (**W**eighted
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
| `default_options()` | returns an initialized revision-3 options record |
| `configuration_snapshot` | owns a deep copy of effective options, limits, and operation text |
| `class error : std::runtime_error` | thrown on any non-`OK` status; `.status()` returns the `DuallityStatus` |
| `void check(DuallityStatus)` | throws `error` unless the status is `OK` |

Both `wfst` and `resource` are **move-only** (copy is deleted), so a handle has exactly one owner.

## Install / build

The recommended installed integration uses the versioned CMake packages:

```cmake
find_package(duallity 4.0 CONFIG REQUIRED)
target_link_libraries(your_target PRIVATE duallity::duallity)
target_compile_features(your_target PRIVATE cxx_std_17)
```

The config performs `find_dependency(vinary-tree-interop 4.0 CONFIG)`; place
both package roots on `CMAKE_PREFIX_PATH` when they are not installed
system-wide. Its imported target propagates the family ABI header and all
platform libraries for shared and static linkage. Set
`DUALLITY_LINKAGE=STATIC` before `find_package` to select the static archive.

For a manual build, put `duallity.hpp`, `duallity.h`, and the interop header
`vinary_tree_interop.h` on the include path, then link the `duallity`
library. Override the interop header name with `VT_INTEROP_HEADER` if your
build vends it elsewhere. On Windows, define `DUALLITY_USING_DLL` when
consuming a DLL. The header requires C++17 (`std::string_view`,
`[[nodiscard]]`). Native packages and a pkg-config file are staged by
[`scripts/stage-native-package.sh`](../../scripts/stage-native-package.sh).

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

## Configured construction and cache controls

The original constructor remains valid. To use API revision 3, initialize the
versioned record with `default_options()`, change the fields you need, then
construct with `wfst(dict, query, options)`. For example:

```cpp
auto options = default_options();
options.maximum_distance = 1;
options.cache_policy = DUALLITY_LRU;
options.cache_capacity = 128;
wfst graph(dict, "cat", options);
auto effective = graph.options();  // copied text and limits outlive graph
auto statistics = graph.cache_statistics();
graph.clear_cache();
graph.set_cache_policy(DUALLITY_NO_CACHE);
```

`options.operations` and nested restriction buffers are borrowed only during
construction, then deep-copied natively. Populate each nested record's header
with its actual `sizeof`, record version `1`, and zero reserved word; use
`operation_stride` and `restriction_stride` for arrays. Invalid sizes, versions,
reserved fields, enum values, or combinations raise `error` with the native
status. `wfst::options()` copies names and restriction text before returning,
so no pointer from `duallity_wfst_options_get` escapes the live handle.
`set_cache_policy` clears residency but preserves the captured dictionary
revision and state identities. The handle and each `retained_resource()` own
independent lifetimes.

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
misbehaving provider, `INVALID_ARGUMENT` for an out-of-range selector or a $`k > 255`$ distance on
a universal/generalized kind, and `PANIC` if a Rust panic was caught at the boundary. Because the
boundary is caught, an exception here is a *reported* error, never undefined behavior.

## Concurrency and zero-copy

- **Distinct handles are independent**; do not free/release the *same* handle from two threads.
- **The produced resource is reentrant** — concurrent expansion shares the registries and snapshot
  behind reference-counted structural sharing.
- **Capture and handoff are $`\mathcal{O}(1)`$**; expansion is lazy and provider buffers are leased for the
  duration of one callback ([architecture/06 §7](../../docs/architecture/06-resource-abi-and-bindings.md#7-provider-fault-handling-and-validation)).
- `duallity_last_error_message()` is thread-local, so `error::what()` reflects this thread's failure.

## Version compatibility

Negotiate with `duallity_abi_version()` (currently `1`) and `duallity_api_revision()`
(currently `3` for minimal FFI builds or `4` with native phonetic constructors)
at load time; refuse an ABI version you do not understand. The C++ facade exposes both its
original constructor and the additive revision-3 constructor/readback/cache controls.
This binding tracks crate `duallity 4.0.0-rc.6`
(**MSRV 1.95**) and `vinary-tree-interop 4.0.0-rc.6` (ABI version `1`). The living version record is the
[bindings findings ledger](../../docs/scientific-ledger/bindings-findings-ledger.md).

## See also

- [docs/guides/07 · Language bindings](../../docs/guides/07-language-bindings.md) — the nine-section cross-language guide.
- [docs/architecture/06 · The resource ABI and language bindings](../../docs/architecture/06-resource-abi-and-bindings.md) — the ABI reference.
- [bindings/javascript/README](../javascript/README.md) — the JS / TS / cljs facade.

## Executable conformance evidence

[`tests/package_smoke.cpp`](tests/package_smoke.cpp) is compiled against the
staged CMake package and checks installed-header version negotiation, record
defaults, move-only types, and old/new constructor error mapping:

```sh
cmake -S bindings/cpp/tests/package -B target/duallity-cpp-package
cmake --build target/duallity-cpp-package
ctest --test-dir target/duallity-cpp-package --output-on-failure
```

[`tests/configured_consumer.cpp`](tests/configured_consumer.cpp) is compiled
and run by [`bindings/c/tests/build-and-run.sh`](../c/tests/build-and-run.sh)
against independent family shared libraries. It constructs both old and
revision-3 WFSTs from a real Unicode dictionary, checks copied custom-operation
readback after caller mutation and handle destruction, switches and clears the
cache, keeps a retained resource beyond handle/source destruction, and rejects
a malformed record. The C family pipeline independently verifies cross-library
snapshot isolation, exact result parity, and both teardown orders.

## Security and provider trust

RAII balances local owners but cannot make an arbitrary dictionary resource
trustworthy. Construction validates the base vtable, dictionary interface and
version, Unicode-scalar domain, node/page output, optional values, and provider
statuses before publishing the WFST. Never create a `resource` by copying raw
words unless the corresponding retain succeeded. See the
[threat model](../../docs/security/threat-model.md).

## Troubleshooting

| Symptom | Likely cause and response |
|---|---|
| `INCOMPATIBLE_RESOURCE` | Verify `vt.dictionary.v1`, its interface version, and Unicode-scalar domain. |
| provider error during traversal | Preserve the diagnostic and audit the provider's paging, IDs, ordering, and lifetimes. |
| interop header not found | Install the family header or set `VT_INTEROP_HEADER` deliberately. |
| shared library not found | Check staged `lib/`, loader path/rpath, target triple, and debug/release profile. |
| resource survives but WFST was destroyed | Expected: each `retained_resource()` result owns an independent retain. |

## Maintainer workflow

1. Update [`bindings/api.json`](../api.json), the C header, and architecture reference together.
2. Preserve move-only ownership and total status-to-exception mapping.
3. Extend installed-package smoke tests, including failure and teardown cases.
4. Run both binding gates and the four-library C family pipeline.
5. Verify shared/static staging and update compatibility/security prose before release.
