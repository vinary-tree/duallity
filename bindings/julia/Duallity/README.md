# Duallity.jl

Composable fuzzy-query automata over Vinary Tree dictionaries for Julia.
Duallity turns one captured dictionary revision and one query into a lazy
**weighted finite-state transducer** (WFST): a directed graph whose arcs consume
an input label, produce an output label, and carry a weight. The returned
`VinaryTreeInterop.Wfst` composes directly with eager, native, or Julia-defined
automata from LlingLlang.jl.

The [published API guide](https://vinary-tree.github.io/duallity/dev/)
contains a doctested quickstart and the current development API reference.

The native adapter never copies the dictionary's terms during construction. It
retains an immutable snapshot and expands only reachable states. The complete
boundary is illustrated by the
[resource data-flow diagram](../../../docs/diagrams/duallity-resource-abi-dataflow.svg).

## Install

The feature branch is a source-only `4.0.0-rc.6` candidate. Develop the local
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
$`w_1 \otimes w_2 = w_1 + w_2`$. This lets an application combine a
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

product = LL.compose(graph, uppercase_graph)
try
    first_page = VTI.arcs(product, VTI.start(product))
finally
    close(product)
    close(uppercase_graph)
end
```

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

### Revision-3 configurable WFST and cache controls

`default_options()` returns the native versioned `DuallityWfstOptionsV1`
record. Build a modified raw record, then call `configured_wfst`. It returns
an owned `ConfiguredWfst`; use `.graph` for traversal or composition, and
close the holder to release both the graph retain and configuration handle.

```julia
using Duallity
import Libdictenstein as LD
import VinaryTreeInterop as VTI

dictionary = LD.DynamicDawg()
dictionary["cat"] = nothing
view = LD.snapshot(dictionary)
base = default_options()
options = DuallityWfstOptionsV1(base.header,
    UInt32(WFST_LEVENSHTEIN), base.algorithm, 1,
    UInt32(LRU), 0, 64, base.limits, base.operations,
    base.operation_count, base.operation_stride, base.reserved)
configured = configured_wfst(view, "cat", options)
try
    graph = configured.graph
    @assert VTI.start(graph) >= 0
    @assert effective_options(configured).cache_policy == LRU
    clear_cache!(configured)
    @assert cache_statistics(configured).clears >= 1
    set_cache_policy!(configured, NO_CACHE)
finally
    close(configured)
    close(view)
    close(dictionary)
end
```

Custom operations and limits use the generated `DuallityOperationV1`,
`DuallityRestrictionV1`, and `DuallityGeneralizedLimitsV1` wire records.
Pass `keepalive=(operation_names, operations, limits, ...)` when their pointers
are non-null. Julia arrays provide contiguous record storage; the native
constructor borrows those buffers for one call and deep-copies them. The
[configuration mirror guide](../../../docs/design/revision3-julia-raku-config-mirrors.md)
explains the record layout, copied readback, malformed-record errors, and
the separate high-level typed-option work item.

### Phonetic patterns, rewrites, and dictionary products

The generalized phonetic selector above enables *digraph edit operations*;
it is not the same as a phonetic regular-expression NFA. Duallity.jl also
exposes the three native phonetic WFST forms and their composition:

| Julia constructor | Native graph | Intended use |
|---|---|---|
| `phonetic_nfa(pattern)` | `PhoneticNfaWfst` | Compile alternatives and character classes into a standalone language graph. |
| `phonetic_product(dictionary, pattern)` | `PhoneticStateSource` / `PhoneticWfst` semantics | Match a pattern through a bounded edit automaton against one captured dictionary revision. |
| `rewrite_wfst(rules)` or `rewrite_wfst(locale)` | `RewriteWfst` | Apply unconditional phonetic substitutions, deletions, and insertions. |
| `phonetic_pipeline(dictionary, pattern; rewrite_rules=..., locale=...)` | Lazy lling-llang composition | Chain either custom rules or a built-in locale with the dictionary-backed product. |

This is the same native algorithm family described in the
[phonetic architecture guide](../../../docs/design/phonetic-pipeline-builder.md)
and [route diagram](../../../docs/diagrams/phonetic-route-decision.svg).
Julia supplies configuration and resource ownership, not a second regex or
edit-distance implementation.

```julia
using Duallity
import Libdictenstein as LD

dictionary = LD.DynamicDawg()
LD.insert_batch!(dictionary, ["phone" => nothing, "fone" => nothing])

pattern = phonetic_nfa("(ph|f)one"; phonetic_weight=0.25)
product = phonetic_product(dictionary, "(ph|f)one";
    maximum_distance=0, cache=:lru, capacity=1024)
try
    @assert Set(match.term for match in phonetic_matches(product;
        max_visits=1000, max_output_scalars=16)) == Set(["phone", "fone"])
finally
    close(product)
    close(pattern)
    close(dictionary)
end
```

`phonetic_nfa` uses a printable-ASCII finite alphabet for wide regex labels by
default; pass `alphabet="éö..."` to enumerate other Unicode scalars for wide
classes. Literal Unicode labels are exact regardless of this alphabet.
Invalid and empty patterns fail with `NativeError`, preserving the native
parser's contract. The product's distance is an *unweighted edit count* in
`0:255`; `phonetic_weight` charges consumed NFA edges and `edit_weight` scales
accepted edit distance. Both must be finite and nonnegative. Neither changes
the threshold.

Rewrite rules are ordered by descending priority, with insertion order
breaking ties; the rule strings are copied into native storage. An empty
output represents deletion. Built-in `:en`, `:de`, and `:fr` selectors use the
native English, German, and French unconditional rule sets:

```julia
using Duallity
import Libdictenstein as LD

dictionary = LD.DynamicDawg()
dictionary["fone"] = nothing
rule = PhoneticRewriteRule("ph", "f"; cost=0.5, priority=2)
rewrite = rewrite_wfst([rule]; allow_identity=false, cache=:none)
german = rewrite_wfst(:de)
pipeline = phonetic_pipeline(dictionary, "fone";
    rewrite_rules=[rule], maximum_distance=1)
try
    candidates = phonetic_matches(pipeline;
        max_visits=10_000, max_output_scalars=32,
        max_results=20, max_cost=2.0)
finally
    close(pipeline)
    close(german)
    close(rewrite)
    close(dictionary)
end
```

Use `phonetic_pipeline(dictionary, pattern; locale=:de)` for a built-in
rewrite stage, or `rewrite_rules=[...]` for custom rules; specifying both is
an argument error. `rewrite_cache`/`rewrite_capacity` configure the rewrite
stage separately from the product's `cache`/`capacity` keywords.

The convenience search enumerates at most `max_visits` distinct state/output
pairs, bounds output by Unicode scalar count, filters by cost, deduplicates
terms at their least *observed* cost, and returns cost-then-lexicographically
ordered `PhoneticMatch(term, cost)` values. When a traversal bound cuts the
graph, the result is deliberately not advertised as globally optimal top-k.
An application needing exhaustive ranking must choose bounds sufficient for
its language. Zero-cost cycles remain safe under finite visit/output bounds.

Locale names select rule sets only: neither native duallity nor this facade
implicitly case-folds or Unicode-normalizes input. Normalize text explicitly
and consistently at dictionary construction and query time if the application
needs that policy; implicit normalization would change exact label semantics.
All four constructors yield owned, tropical Unicode-scalar graphs and can be
composed with lling-llang or custom `vt.scalar-wfst.1` providers. The cache
selector is `:all`, `:none`, or `:lru` (with a positive capacity or native
default); it changes residency, not accepted language or weights.

## Ownership & memory model

`wfst` borrows the input only for the call, captures its current immutable
revision exactly once, and returns one independently owned
`VinaryTreeInterop.Wfst`. Closing or mutating the source afterward cannot alter
the graph. LlingLlang composition captures another independent retain of each
operand. Call `close` deterministically; Julia finalizers are leak-safety
fallbacks.

`configured_wfst` retains both a graph and a native handle. Closing its
`ConfiguredWfst` holder releases both. `effective_options` copies nested
native strings and records, so its result remains valid after the holder
closes; calling handle methods after close raises `ArgumentError`.

## Errors

Native failures throw `NativeError` with a stable `Status`, operation, and
copied thread-local diagnostic. The Julia facade rejects negative distances
before crossing the ABI. Native validation rejects invalid UTF-8, unknown enum
values, incompatible/non-Unicode dictionary providers, malformed callback
pages, and unrepresentable distances. No Rust panic or foreign-provider
exception unwinds across the C boundary.

Phonetic constructors additionally reject malformed/empty patterns, unknown
locales, invalid rewrite records, and nonfinite or negative weights. The C
boundary clears every output resource on failure; failed construction never
leaks a dictionary retain. A composed pipeline closes its intermediate
graphs after lling-llang captures independent operand retains.

## Concurrency

Returned resources are immutable and reentrant. If a dictionary advertises
parallel-reentrant callbacks, independent expansion calls may execute
concurrently; otherwise duallity serializes calls into that provider. The
adapter does not hold provider locks while executing unrelated Julia code.
Share a graph only under the concurrency contract reported by its Vinary Tree
flags.

## Zero-copy paths

Construction and handoff are $`\mathcal{O}(1)`$ in dictionary size: the adapter
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
| Duallity.jl | `4.0.0-rc.6` |
| duallity C ABI | `1` |
| duallity API revision | at least `4` for phonetic constructors |
| VinaryTreeInterop.jl | major version `4` |
| Julia | `1.10` or newer |

Module initialization validates the native ABI and minimum API revision.

## Executable conformance evidence

[`test/runtests.jl`](test/runtests.jl) constructs all nine kinds and all four
algorithms against a real libdictenstein dictionary, verifies weight domains,
proves capture-once behavior under live mutation, and composes the result with
an lling-llang case-mapping graph. It also exercises phonetic ambiguity,
Unicode, empty-pattern errors, edit thresholds, priority rewrites, locale
presets, cache policies, resource independence, bounded traversal, custom
revision-3 operations and limits, copied readback, and malformed records.

```sh
TMPDIR="$PWD/target/julia-tmp" \
DUALLITY_LIBRARY="$PWD/target/debug/libduallity.so" \
julia --project=bindings/julia/Duallity -e 'using Pkg; Pkg.test()'
```

[`benchmark/compare.jl`](benchmark/compare.jl) measures adapter construction and
first-state access separately from dictionary construction.

## Maintainer workflow

1. Change `bindings/api.json`, the C header, Rust exports, and generated ABI
   files together; run `python3 scripts/generate-config-abi.py --check`.
2. Preserve existing ABI entry points; add pointer forms for aggregate-limited
   FFIs and raise only the additive API revision.
3. Run Rust FFI tests, every Julia test, strict Documenter output, binding and
   documentation drift gates, and the mandatory pgmcp bug gate.
4. Commit source, generated surfaces, package docs, and verification evidence
   together with a descriptive enumerated message.
5. Push only the approved feature branch. Do not tag or publish this candidate.
