# Foreign-language bindings

**Navigation:** [documentation index](../README.md) ·
[binding walkthrough](../guides/07-language-bindings.md) ·
[resource ABI](../architecture/06-resource-abi-and-bindings.md) ·
[threat model](../security/threat-model.md)

duallity exposes one eight-function C ABI and five standalone facade guides.
JavaScript, TypeScript, and ClojureScript share the same npm package and
singleton runtime, so their common laws and language-specific syntax live in
one guide.

| Guide | Languages | Boundary | Executable evidence |
|---|---|---|---|
| [C](../../bindings/c/README.md) | C17/C23 | Direct `duallity_*` ABI | [`family_pipeline.c`](../../bindings/c/tests/family_pipeline.c) |
| [C++](../../bindings/cpp/README.md) | C++17+ | Header-only move-only RAII | [`package_smoke.cpp`](../../bindings/cpp/tests/package_smoke.cpp) |
| [JavaScript family](../../bindings/javascript/README.md) | JavaScript, TypeScript, ClojureScript | npm facade over N-API/WebAssembly/WASI | [`facades.test.mjs`](../../bindings/javascript/test/facades.test.mjs) |
| [Julia](../../bindings/julia/Duallity/README.md) | Julia 1.10+ | `ccall` facade returning standard `VinaryTreeInterop.Wfst` resources | [`runtests.jl`](../../bindings/julia/Duallity/test/runtests.jl) |
| [Raku](../../bindings/raku/README.md) | Raku 6.d | NativeCall facade returning standard `Vinary::Tree::Interop::Wfst` resources | [`01-conformance.rakutest`](../../bindings/raku/t/01-conformance.rakutest) |

The architecture is a double adapter: a dictionary provider crosses the family
ABI into duallity, which emits a scalar-WFST provider consumed by lling-llang or
another family package. Both crossings retain immutable snapshots and validate
untrusted providers.

![End-to-end resource data flow and trust boundaries.](../diagrams/duallity-resource-abi-dataflow.svg)

## Documentation governance

[`bindings/api.json`](../../bindings/api.json) declares every supported language,
guide, checked example, and required topic. CI runs:

```sh
python3 scripts/check-bindings.py
python3 scripts/check-binding-docs.py
```

The documentation gate rejects inventory drift, missing evidence, broken local
links, untagged code fences, placeholders, and absent installation, API,
ownership, error, concurrency, performance, security, compatibility, or
maintainer coverage.
