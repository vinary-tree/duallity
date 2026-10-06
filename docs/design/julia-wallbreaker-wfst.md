# Julia WallBreaker result WFST

`wallbreaker_wfst` captures one `Libdictenstein.Dictionary` revision and
enumerates its Unicode terms under the native matcher limits. The source
snapshot is closed after enumeration. The liblevenshtein native matcher copies
the terms into its own SCDAWG, runs the selected WallBreaker algorithm, and
returns copied `(term, distance)` results. The matcher and result cursor close
before the returned graph is built.

The graph is an owned `vt.scalar-wfst.1` resource. State zero is
the start. Every result contributes one independent chain of identity-labelled
arcs, with zero arc weights and the exact native edit distance on its final
state. An empty result term makes the start state final. Traversing accepting
paths therefore yields the same term and cost pairs as the native matcher.
The graph composes with another scalar WFST through `LlingLlang.compose` and
remains valid after the source dictionary closes.

The Julia facade passes the complete native result set through the sized
`DuallityWallBreakerResultV1` C record. The revision-5 constructor checks
record shape, Unicode, duplicate terms, distance bounds, and native hard ceilings,
then copies each term. The returned `WallBreakerGraph` owns both the resource
retain and the C handle. The resource uses duallity's shared provider expansion
cache. `cache_statistics` reads its cumulative counters, `clear_cache!` evicts
resident states, and `set_cache_policy!` changes policy and starts an empty
cache generation. These operations do not change graph states, paths, or
weights. `wallbreaker_statistics` separately reports immutable result, state,
and arc counts.

`maximum_distance` is restricted to `0:8`, and
the native matcher's positive hard ceilings apply to terms, bytes, scalar
lengths, candidate cloning, results, and result bytes. The adapter accepts
standard, transposition, and merge/split algorithms; unrestricted Damerau is
not implemented by the native WallBreaker matcher. Only Unicode scalar
dictionaries are accepted.

The conformance suite compares the graph language and tropical weights
against direct native matcher results for each supported algorithm, checks
composition, Unicode labels, empty dictionaries, bounded failures, ownership
after the input closes, and cache behavior under all three policies. Rust
boundary tests additionally reject malformed records and check cache sharing
through the exported scalar resource.
