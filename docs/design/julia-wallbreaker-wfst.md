# Julia WallBreaker result WFST

`wallbreaker_wfst` captures one `Libdictenstein.Dictionary` revision and
enumerates its Unicode terms under the native matcher limits. The source
snapshot is closed after enumeration. The liblevenshtein native matcher copies
the terms into its own SCDAWG, runs the selected WallBreaker algorithm, and
returns copied `(term, distance)` results. The matcher and result cursor close
before the returned graph is built.

The graph is an owned `LlingLlang.Wfst{Char,TropicalWeight}`. State zero is
the start. Every result contributes one independent chain of identity-labelled
arcs, with zero arc weights and the exact native edit distance on its final
state. An empty result term makes the start state final. Traversing accepting
paths therefore yields the same term and cost pairs as the native matcher.
The graph composes with another scalar WFST through `LlingLlang.compose` and
remains valid after the source dictionary closes.

The finite graph is built eagerly. It does not have the mutable state cache of
duallity's lazy `ConfiguredWfst` resource. Its immutable statistics report
result, state, and arc counts. `maximum_distance` is restricted to `0:8`, and
the native matcher's positive hard ceilings apply to terms, bytes, scalar
lengths, candidate cloning, results, and result bytes. The adapter accepts
standard, transposition, and merge/split algorithms; unrestricted Damerau is
not implemented by the native WallBreaker matcher. Only Unicode scalar
dictionaries are accepted.

The conformance suite compares the graph language and tropical weights
against direct native matcher results for each supported algorithm, checks
composition, Unicode labels, empty dictionaries, bounded failures, and
ownership after the input closes.
