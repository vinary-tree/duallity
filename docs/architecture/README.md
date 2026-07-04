# Architecture

This section explains *how* duallity is built — the crate family it bridges, the trait surface every
WFST satisfies, the product-state encoding, the lazy evaluation and caching machinery, and the
registries that give dictionary nodes and automaton states stable identities.

Where [theory](../theory/) answers "what is a Levenshtein WFST and why", this section answers "how is
it realized in Rust". The [design](../design/) section then documents each concrete WFST variant.

| # | Document | What you will learn |
|---|----------|---------------------|
| 01 | [Crate family and dependency graph](01-crate-family-and-dependency-graph.md) | The four crates, why duallity is a separate crate, and how to migrate from liblevenshtein's old `wfst` module. |
| 02 | [The WFST trait surface](02-wfst-trait-surface.md) | `Wfst`, `LazyWfst`, `StateSource`, `LatticeBackend`, and `CachePolicy`. |
| 03 | [State encoding and the product space](03-state-encoding-and-product-space.md) | How `(dict_node, automaton_state)` pairs pack into a single `StateId`. |
| 04 | [Lazy evaluation and caching](04-lazy-evaluation-and-caching.md) | `expand → compute_state → cache`, deterministic LRU, and the immutable/mutable split. |
| 05 | [Registries and interning](05-registries-and-interning.md) | How nodes and abstract states get stable `u32` ids, and the concurrency model. |

All symbols are from the [theory notation table](../theory/README.md#master-notation).
