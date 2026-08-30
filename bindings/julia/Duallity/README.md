# Duallity.jl

Composable fuzzy-query automata over Vinary Tree dictionaries for Julia.
Duallity turns one captured dictionary revision and one query into a lazy
**weighted finite-state transducer** (WFST): a directed graph whose arcs consume
an input label, produce an output label, and carry a weight. The returned
`VinaryTreeInterop.Wfst` composes directly with eager, native, or Julia-defined
automata from LlingLlang.jl.

The native adapter never copies the dictionary's terms during construction. It
retains an immutable snapshot and expands only reachable states. The complete
boundary is illustrated by the
[resource data-flow diagram](../../../docs/diagrams/duallity-resource-abi-dataflow.svg).

## Install

The feature branch is a source-only `4.0.0-rc.5` candidate. Develop the local
packages and build duallity with its Julia facade enabled:

```julia
using Pkg
Pkg.develop(path="../vinary-tree-interop/bindings/julia/VinaryTreeInterop")
Pkg.develop(path="bindings/julia/Duallity")
```

```sh
cargo build --release --no-default-features --features julia-bindings
export DUALLITY_LIBRARY="$PWD/target/release/libduallity.so"
```

Use `libduallity.dylib` on macOS and `duallity.dll` on Windows.

## Quickstart

```julia
using Duallity
import Libdictenstein as LD
import LlingLlang as LL
import VinaryTreeInterop as VTI

dictionary = LD.DynamicDawg()
LD.insert_batch!(dictionary,
    ["cat" => nothing, "cot" => nothing, "dog" => nothing])
view = LD.snapshot(dictionary)
graph = wfst(view, "cat"; maximum_distance=1)
close(view)

@assert VTI.weight_domain(graph) == VTI.WEIGHT_TROPICAL_F64
@assert !isempty(VTI.arcs(graph, VTI.start(graph)))
close(graph)
close(dictionary)
```

### Compose a fuzzy query with another transducer

Composition joins the first graph's output tape to the second graph's input
tape. For tropical weights, multiplication is addition:
`` $`w_1 \otimes w_2 = w_1 + w_2`$ ``. This lets an application combine a
fuzzy dictionary query with normalization, grammar, language-model, or custom
Julia providers without materializing the intermediate language.

```julia
mapper = LL.WfstBuilder(size_hint=1)
state = LL.add_state!(mapper)
LL.set_start!(mapper, state)
LL.set_final!(mapper, state)
for character in ['a', 'c', 'o', 't']
    LL.add_arc!(mapper, state, character, uppercase(character), state)
end
uppercase_graph = LL.build!(mapper)

product = product_automaton(graph, uppercase_graph)
try
    first_page = VTI.arcs(product, VTI.start(product))
finally
    close(product)
    close(uppercase_graph)
end
```

Pass additional WFSTs to construct a larger product in one call. Duallity
leaves caller-owned operands open, closes internal intermediate products as
soon as the next product retains them, and returns one owned graph.

### Algorithms and adapter kinds

`algorithm` selects the edit-operation family for `WFST_LEVENSHTEIN`:

| Value | Public meaning |
|---|---|
| `ALGORITHM_STANDARD` | insertion, deletion, and substitution |
| `ALGORITHM_TRANSPOSITION` | standard edits plus adjacent transposition |
| `ALGORITHM_MERGE_AND_SPLIT` | standard edits plus character merge/split |
| `ALGORITHM_DAMERAU_LEVENSHTEIN` | unrestricted Damerau-Levenshtein edits |

`kind` selects the graph construction:

| Value | Construction and weight domain |
|---|---|
| `WFST_LEVENSHTEIN` | parameterized Levenshtein product; tropical |
| `WFST_UNIVERSAL_STANDARD` | universal standard edit automaton; tropical |
| `WFST_UNIVERSAL_TRANSPOSITION` | universal adjacent-transposition automaton; tropical |
| `WFST_UNIVERSAL_MERGE_AND_SPLIT` | universal merge/split automaton; tropical |
| `WFST_GENERALIZED_STANDARD` | generalized standard operations; tropical |
| `WFST_GENERALIZED_TRANSPOSITION` | generalized transposition operations; tropical |
| `WFST_GENERALIZED_MERGE_AND_SPLIT` | generalized merge/split operations; tropical |
| `WFST_GENERALIZED_PHONETIC` | generalized phonetic-digraph operations; tropical |
| `WFST_FZF` | FZF-v2-style path ranking; Arctic (max-plus) |

Universal and generalized variants represent distances through `UInt8`, so
their maximum distance is at most 255. The native boundary reports an error
instead of narrowing a larger value.

## Ownership & memory model

`wfst` borrows the input only for the call, captures its current immutable
revision exactly once, and returns one independently owned
`VinaryTreeInterop.Wfst`. Closing or mutating the source afterward cannot alter
the graph. LlingLlang composition captures another independent retain of each
operand. Call `close` deterministically; Julia finalizers are leak-safety
fallbacks.

## Errors

Native failures throw `NativeError` with a stable `Status`, operation, and
copied thread-local diagnostic. The Julia facade rejects negative distances
before crossing the ABI. Native validation rejects invalid UTF-8, unknown enum
values, incompatible/non-Unicode dictionary providers, malformed callback
pages, and unrepresentable distances. No Rust panic or foreign-provider
exception unwinds across the C boundary.

## Concurrency

Returned resources are immutable and reentrant. If a dictionary advertises
parallel-reentrant callbacks, independent expansion calls may execute
concurrently; otherwise duallity serializes calls into that provider. The
adapter does not hold provider locks while executing unrelated Julia code.
Share a graph only under the concurrency contract reported by its Vinary Tree
flags.

## Zero-copy paths

Construction and handoff are `` $`O(1)`$ `` in dictionary size: the adapter
captures one two-word resource handle and lling-llang receives another retained
two-word handle. State and arc expansion is lazy and paged. Shared registries
intern product states, so composition does not clone the reachable graph in
advance.

## Security and provider trust

A foreign dictionary is synchronous plugin code. Duallity negotiates the
`vt.dictionary.v1` capability and version, requires Unicode-scalar labels, and
validates statuses, booleans, Unicode scalars, page progress, page bounds, and
resource ownership. Applications must still constrain untrusted provider work
and verify any claimed immutability or parallel reentrancy. Never exchange raw
resource words across incompatible runtimes or processes.

## Troubleshooting

- Set `DUALLITY_LIBRARY` when the platform loader cannot locate the native
  library or one of its dependencies.
- `STATUS_INCOMPATIBLE_RESOURCE` means the source lacks `vt.dictionary.v1` or
  does not use Unicode-scalar units.
- `STATUS_PROVIDER_ERROR` means a dictionary callback failed or returned
  malformed data.
- An invalid-argument error for a universal/generalized kind commonly means
  `maximum_distance > 255`.
- Close all graphs in `finally` blocks when memory rises during repeated query
  construction.

## Version compatibility

| Component | Required value |
|---|---:|
| Duallity.jl | `4.0.0-rc.5` |
| duallity C ABI | `1` |
| duallity API revision | at least `2` |
| VinaryTreeInterop.jl | major version `4` |
| Julia | `1.10` or newer |

Module initialization validates the native ABI and minimum API revision.

## Executable conformance evidence

[`test/runtests.jl`](test/runtests.jl) constructs all nine kinds and all four
algorithms against a real libdictenstein dictionary, verifies weight domains,
proves capture-once behavior under live mutation, and composes the result with
an lling-llang case-mapping graph.

```sh
TMPDIR="$PWD/target/julia-tmp" \
DUALLITY_LIBRARY="$PWD/target/debug/libduallity.so" \
julia --project=bindings/julia/Duallity -e 'using Pkg; Pkg.test()'
```

[`benchmark/compare.jl`](benchmark/compare.jl) measures adapter construction and
first-state access separately from dictionary construction.

## Maintainer workflow

1. Change `bindings/api.json`, the C header, Rust exports, and generated enum
   files together.
2. Preserve existing ABI entry points; add pointer forms for aggregate-limited
   FFIs and raise only the additive API revision.
3. Run Rust FFI tests, every Julia test, strict Documenter output, binding and
   documentation drift gates, and the mandatory pgmcp bug gate.
4. Commit source, generated surfaces, package docs, and verification evidence
   together with a descriptive enumerated message.
5. Push only the approved feature branch. Do not tag or publish this candidate.
