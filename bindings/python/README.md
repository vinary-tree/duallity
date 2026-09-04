# duallity for Python

Turn fuzzy dictionary queries into lazy weighted transducers that compose with
phonetic rewrites, grammars, language models, and custom lling-llang pipelines.

The `duallity` package is a typed Python facade over duallity's stable C ABI. It
accepts any live Vinary Tree Unicode dictionary resource, captures exactly one
immutable query-start revision, and returns a shared `ScalarWfst`. Terms are not
copied across the Python/native boundary and states are expanded only when they
are traversed.

The facade covers all four edit algorithms and all nine native transducer kinds.
It does not invent a Python-only composition algebra: relational composition is
owned by lling-llang, while intersections and products must use operations whose
semantics are explicitly defined by their owning library.

## Install

Released platform wheels bundle the duallity native library and depend on the
shared `vinary-tree-interop` Python package:

```sh
python -m pip install --pre duallity
```

The coordinated RC.6 development graph can be installed from sibling checkouts:

```sh
python -m pip install -e ../vinary-tree-interop/bindings/python
python -m pip install -e ../libdictenstein/bindings/python
python -m pip install -e ../lling-llang/bindings/python
python -m pip install -e ./bindings/python
```

During development, set `DUALLITY_LIBRARY` to a compatible shared library. A
wheel build instead accepts `DUALLITY_PREBUILT_LIBRARY`; if absent, `setup.py`
builds the exact source checkout with Cargo's `python-bindings` feature.

The source distribution is self-contained: it carries duallity's Rust sources
and a validated Cargo manifest whose sibling dependencies are exact RC.6
registry coordinates rather than checkout-relative paths. Building it requires
Rust 1.95 and the corresponding RC.6 family crates to be available from
crates.io. Prefer a platform wheel when one is available.

## Quickstart

`libdictenstein.DynamicDawg` implements the shared dictionary resource protocol,
so no adapter or term materialization is required:

```python
import duallity
import libdictenstein

with libdictenstein.DynamicDawg() as dictionary:
    dictionary.update_many((("cat", None), ("cot", None), ("dog", None)))
    with duallity.wfst(dictionary, "cat", maximum_distance=1) as fuzzy:
        print(fuzzy.start)
        print(fuzzy.arcs(fuzzy.start))
```

The complete executable family example is
[`examples/family_pipeline.py`](examples/family_pipeline.py). It feeds the
result into `lling_llang.compose` and preserves dictionary labels through a
case-mapping transducer.

## API, selectors, and domains

The public constructor is:

```python
duallity.wfst(
    dictionary,
    query,
    *,
    maximum_distance=1,
    algorithm=duallity.Algorithm.STANDARD,
    kind=duallity.WfstKind.LEVENSHTEIN,
)
```

`Algorithm` contains `STANDARD`, `TRANSPOSITION`, `MERGE_AND_SPLIT`, and
`DAMERAU_LEVENSHTEIN`. It selects the operation family of the parameterized
`LEVENSHTEIN` kind. Other native families encode their edit operations in the
`WfstKind` selector:

| `WfstKind` | Weight domain | Distance bound | Intended role |
|---|---|---:|---|
| `LEVENSHTEIN` | tropical `f64` | platform `size_t` | Parameterized edit distance |
| `UNIVERSAL_STANDARD` | tropical `f64` | 255 | Universal standard automaton |
| `UNIVERSAL_TRANSPOSITION` | tropical `f64` | 255 | Universal adjacent transposition |
| `UNIVERSAL_MERGE_AND_SPLIT` | tropical `f64` | 255 | Universal merge/split edits |
| `GENERALIZED_STANDARD` | tropical `f64` | 255 | Generalized standard operations |
| `GENERALIZED_TRANSPOSITION` | tropical `f64` | 255 | Generalized transposition |
| `GENERALIZED_MERGE_AND_SPLIT` | tropical `f64` | 255 | Generalized merge/split |
| `GENERALIZED_PHONETIC` | tropical `f64` | 255 | Generalized phonetic digraphs |
| `FZF` | Arctic `f64` | ignored | FZF-compatible path scoring |

Every result is a `duallity.Wfst`, which extends
`vinary_tree_interop.ScalarWfst`. Its shared API includes `start`,
`state_count`, `state_info(state)`, `arcs(state)`, `state(state)`, `snapshot()`,
`native_resource`, and `close()`.

## Host-defined dictionaries

Python can implement the input resource without a libdictenstein dependency.
Provide an immutable snapshot object with `root`, `__len__`, `is_final`,
`value`, and `edges`, then export it with `UnicodeDictionaryResource`:

```python
from vinary_tree_interop import UnicodeDictionaryResource
import duallity

snapshot = MyImmutableTrieSnapshot()
with UnicodeDictionaryResource(lambda: snapshot) as dictionary:
    graph = duallity.wfst(dictionary, "query", maximum_distance=2)

dictionary.close()  # graph retained the captured revision
with graph:
    print(graph.state(graph.start))
```

Callback failures are contained by the interop trampoline, retained as
`last_callback_error`, and reported by duallity as `Status.PROVIDER_ERROR`.
Provider methods must obey the contracts documented by
[`vinary-tree-interop`](https://github.com/vinary-tree/vinary-tree-interop/tree/v4.0.0-rc.6/bindings/python).

## Ownership and memory model

`wfst()` borrows the two-word input resource only for the constructor call. The
native layer invokes its snapshot callback exactly once and retains that
revision. On success, the temporary project handle transfers one owned resource
retain into `Wfst`; no dictionary terms or WFST state arrays cross the boundary.

Use `with` for deterministic release. `close()` is idempotent, and a finalizer
is a last-resort leak guard rather than the normal lifecycle:

```python
with duallity.wfst(dictionary, "colour", maximum_distance=2) as graph:
    frozen = graph.snapshot()

with frozen:
    consume(frozen)
```

A snapshot and a source graph own independent retains. Closing either one does
not invalidate the other. Access after close raises `InteropError`.

## Errors

Native failures raise `duallity.NativeError`. Its `status` is a `Status` member
when known, or the future integer discriminant when a newer library reports an
unknown additive status. `operation` identifies the failing boundary call and
the exception message contains a copied thread-local diagnostic.

Python-side type and range failures use `TypeError` or `ValueError` before the
unsafe boundary is entered. Incompatible interfaces, non-Unicode dictionaries,
provider callback failures, and native resource limits retain distinct status
values.

```python
try:
    graph = duallity.wfst(
        dictionary,
        "cat",
        maximum_distance=256,
        kind=duallity.WfstKind.GENERALIZED_STANDARD,
    )
except duallity.NativeError as error:
    assert error.status is duallity.Status.INVALID_ARGUMENT
```

## Concurrency and zero-copy

Construction captures a constant-size `VtResource`; it does not serialize the
dictionary. The returned graph is lazy and advertises `WfstFlag.LAZY` and
`WfstFlag.IMMUTABLE`. Calls are serialized only when a provider does not opt
into the shared `PARALLEL_REENTRANT` contract. Python-hosted providers should
enable parallel reentrancy only when every callback and captured snapshot is
safe for concurrent entry.

The facade deliberately performs one native constructor call and one O(1)
resource handoff. Use the shared paged `arcs()` operation rather than issuing
one foreign call per arc. For measurement, run:

```sh
python bindings/python/benchmark/compare.py --iterations 100 --terms 10000
```

The benchmark reports construction separately from first-state expansion so
lazy work is not misattributed to the Python boundary.
The preregistered protocol and complete interpretation are recorded in the
[Python binding construction ledger](../../docs/scientific-ledger/python-binding-construction-2026-09-04.md).

## Security and provider trust

Foreign resource vtables are executable code. Only accept providers from the
same trust boundary as the process. duallity validates base ABI versions,
required callbacks, dictionary interface versions, Unicode-scalar domains,
status discriminants, pagination progress, counts, labels, and boolean flags;
validation limits damage but cannot make hostile native pointers memory-safe.

Queries are UTF-8 encoded with explicit lengths. No NUL termination is assumed.
Distances and selectors are range-checked before FFI. Keep resource bounds on
any application-level traversal because a valid lazy graph may still represent
a large reachable state space.

## Troubleshooting

- `ImportError: could not load duallity`: set `DUALLITY_LIBRARY` to the exact
  shared library or install a platform wheel.
- ABI mismatch during import: the wheel/facade and native library came from
  different release trains; do not mix release candidates.
- `INCOMPATIBLE_RESOURCE`: pass a live `vt.dictionary.v1` resource with the
  Unicode-scalar unit domain.
- `PROVIDER_ERROR`: inspect a Python provider's `last_callback_error`; for a
  native provider, inspect its own diagnostic facility.
- generalized or universal distance rejection: those kinds accept at most 255;
  use a lower distance or the parameterized Levenshtein kind.

## Version compatibility

This facade is version `4.0.0rc6`, requires `vinary-tree-interop==4.0.0rc6`,
requires duallity ABI version `1`, and requires native API revision `2` or
newer. Exact RC pins prevent accidental cross-train ABI mixtures. The import
guard accepts additive API revisions but rejects a different ABI version or an
older API revision.

Python 3.10 through 3.14 are supported. Wheels use the `py3-none-<platform>` tag
because Python code is stable across CPython ABIs while the bundled Rust shared
library is platform-specific.

## Executable conformance evidence

[`tests/test_api.py`](tests/test_api.py) checks all algorithms and transducer
kinds, domains and lazy flags, query-start snapshot lifetime, deterministic and
idempotent close, empty/non-ASCII/embedded-NUL UTF-8 queries, Python-hosted
provider failures, numeric and selector bounds, and a real libdictenstein →
duallity → lling-llang pipeline.

```sh
python -m unittest discover -s bindings/python/tests -v
python bindings/python/examples/family_pipeline.py
python -m build --wheel bindings/python
```

Repository CI also runs Ruff, Pyright, the binding-model drift gate, the
documentation gate, wheel installation in a clean environment, and native Rust
tests. The Rust constructor cross-product remains the exhaustive low-level
oracle in [`tests/ffi_constructor_matrix.rs`](../../tests/ffi_constructor_matrix.rs).

## Maintainer workflow

1. Change the authoritative enums, functions, or package coordinates in
   [`bindings/api.json`](../api.json).
2. Update the Rust ABI, C headers, generated facades, Python declarations,
   documentation, and conformance tests in the same change.
3. Run `python3 scripts/sync-release-version.py`; it must report no drift.
4. Run `python3 scripts/check-bindings.py` and
   `python3 scripts/check-binding-docs.py`.
5. Build and test under the repository's bounded-resource command policy.
6. Run `pgmcp bug-gate` before committing.

The release workflow builds platform wheels from an immutable tag and uses
PyPI trusted publishing. RC.6 preparation may be pushed for CI validation, but
must not be tagged or published until the coordinated release is authorized.
