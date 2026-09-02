# Duallity for Raku

Composable fuzzy-query automata over Vinary Tree dictionaries for Raku.
Duallity captures one immutable dictionary revision and exposes the query as a
lazy **weighted finite-state transducer** (WFST), a graph whose arcs consume an
input label, produce an output label, and carry a weight. The result is a
standard `Vinary::Tree::Interop::Wfst`, so it composes directly with eager,
native, or Raku-defined automata from `Lling::Llang`.

## Install

This branch is a source-only `4.0.0-rc.6` candidate. Build the native adapter
and make the sibling Raku packages visible:

```sh
cargo build --release --no-default-features --features raku-bindings
export DUALLITY_LIBRARY="$PWD/target/release/libduallity.so"
export RAKULIB="$PWD/bindings/raku/lib,$PWD/../vinary-tree-interop/bindings/raku/lib"
```

Use `libduallity.dylib` on macOS and `duallity.dll` on Windows.

## Quickstart

```raku
use Duallity;
use Libdictenstein;

my $dictionary = dynamic-dawg;
$dictionary.insert-batch([cat => Nil, cot => Nil, dog => Nil]);
my $view = $dictionary.snapshot;
my $graph = wfst($view, 'cat', maximum-distance => 1);
$view.close;

say $graph.arcs($graph.start).elems;
$graph.close;
$dictionary.close;
```

### Compose with lling-llang

Composition joins the duallity output tape to another transducer's input tape.
Tropical multiplication adds weights:
$`w_1 \otimes w_2 = w_1 + w_2`$. No intermediate language is
materialized.

```raku
use Lling::Llang;

my $mapper = WfstBuilder.new(size-hint => 1);
my $state = $mapper.add-state;
$mapper.set-start($state).set-final($state);
for <a c o t> -> $character {
    $mapper.add-arc($state, $character, $character.uc, $state);
}
my $uppercase = $mapper.build;
my $product = compose($graph, $uppercase);
$product.close;
$uppercase.close;
```

### Complete selector surface

The `Algorithm` enum contains `STANDARD`, `TRANSPOSITION`,
`MERGE-AND-SPLIT`, and `DAMERAU-LEVENSHTEIN`. The `WfstKind` enum contains:

| Kind | Meaning and weight domain |
|---|---|
| `LEVENSHTEIN` | parameterized edit product; tropical |
| `UNIVERSAL-STANDARD` | universal standard edits; tropical |
| `UNIVERSAL-TRANSPOSITION` | universal adjacent transposition; tropical |
| `UNIVERSAL-MERGE-AND-SPLIT` | universal merge/split; tropical |
| `GENERALIZED-STANDARD` | generalized standard operations; tropical |
| `GENERALIZED-TRANSPOSITION` | generalized transposition; tropical |
| `GENERALIZED-MERGE-AND-SPLIT` | generalized merge/split; tropical |
| `GENERALIZED-PHONETIC` | generalized phonetic digraphs; tropical |
| `FZF` | FZF-v2-style path ranking; Arctic (max-plus) |

Universal/generalized maximum distances are bounded by `UInt8` and therefore
cannot exceed 255. FZF ignores the distance selector.

## Ownership & memory model

`wfst` borrows the source only during construction, captures its current
immutable revision exactly once, and returns one independent owned WFST.
Closing or mutating the source afterward cannot change that graph. Composition
retains independent snapshots of both operands. Call `.close`
deterministically; `DESTROY` is a leak-safety fallback.

## Errors

Native failures throw `X::Duallity` with a stable `Status`, operation, and
copied diagnostic. Native validation rejects invalid UTF-8, unknown selectors,
non-Unicode/incompatible dictionary resources, malformed callback pages, and
unrepresentable distances. Panics and provider failures are contained at the C
boundary.

## Concurrency

Returned resources are immutable and reentrant. Duallity invokes dictionary
callbacks concurrently only when the provider advertises parallel reentrancy;
otherwise it serializes that provider. No Raku callback is introduced by this
facade, and resource handoff does not hold a provider lock.

## Zero-copy paths

Construction and handoff are $`\mathcal{O}(1)`$ in dictionary size. The native
adapter retains a two-word snapshot resource, state expansion is lazy and
paged, and lling-llang composition receives another retained two-word handle.
Shared registries intern reachable product states rather than cloning a graph.

## Security and provider trust

Treat a foreign dictionary like synchronous plugin code. Duallity negotiates
`vt.dictionary.v1`, requires Unicode-scalar units, and validates statuses,
booleans, labels, page progress/bounds, and resource ownership. Applications
must constrain untrusted callback work and verify claimed immutability and
parallel reentrancy. Raw handles must not cross runtimes or processes.

## Troubleshooting

- Set `DUALLITY_LIBRARY` when the native library cannot be loaded.
- `INCOMPATIBLE-RESOURCE` means the source lacks `vt.dictionary.v1` or uses a
  non-Unicode unit domain.
- `PROVIDER-ERROR` means a dictionary callback failed or returned malformed
  data.
- Reduce `maximum-distance` to at most 255 for universal/generalized kinds.
- Close every graph in `LEAVE`/`CATCH`-safe code during repeated construction.

## Version compatibility

| Component | Required value |
|---|---:|
| Duallity Raku package | `4.0.0-rc.6` |
| duallity C ABI | `1` |
| duallity API revision | at least `2` |
| Vinary Tree Interop | `4.0.0` compatible |
| Raku | language version `6.d` |

Module initialization checks the native ABI and API revision.

## Executable conformance evidence

[`t/01-conformance.rakutest`](t/01-conformance.rakutest) constructs all nine
kinds and four algorithms over a real libdictenstein dictionary, checks weight
domains, mutates the live source to prove capture-once isolation, and composes
the adapter with an lling-llang case mapper.

```sh
TMPDIR="$PWD/target/raku-tmp" \
DUALLITY_LIBRARY="$PWD/target/debug/libduallity.so" \
RAKULIB="$PWD/bindings/raku/lib,$PWD/../vinary-tree-interop/bindings/raku/lib,$PWD/../libdictenstein/bindings/raku/lib,$PWD/../lling-llang/bindings/raku/lib" \
raku bindings/raku/t/01-conformance.rakutest
```

[`benchmark/compare.raku`](benchmark/compare.raku) measures adapter construction
and first-state access independently of dictionary construction.

## Maintainer workflow

1. Update `bindings/api.json`, Rust exports, the C header, and generated enums
   together.
2. Preserve by-value ABI entry points and use additive pointer forms for
   NativeCall.
3. Run Rust FFI tests, Raku conformance, rendered Pod, both binding gates, and
   the mandatory pgmcp bug gate.
4. Commit implementation, documentation, tests, benchmark, and evidence with a
   descriptive enumerated message.
5. Push only the approved feature branch; do not tag or publish this candidate.
