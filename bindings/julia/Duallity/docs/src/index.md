# Duallity.jl

Duallity.jl captures a Vinary Tree dictionary revision as a lazy fuzzy-query
weighted finite-state transducer and hands it to Julia or lling-llang without
materializing the accepted language. Its separate WallBreaker adapter builds a
bounded finite result graph eagerly. The package
[README](https://github.com/vinary-tree/duallity/tree/master/bindings/julia/Duallity#readme)
defines the nine adapter kinds, four edit algorithms, ownership, composition,
concurrency, security, and executable examples.

## A captured fuzzy query and its composition

The following example runs during documentation builds. It captures one
dictionary revision, constructs a bounded fuzzy-query graph, and composes that
graph with a transducer that uppercases its accepted output. Every owned native
resource is closed deterministically, including when construction fails.

```@example first-query
using Duallity
import Libdictenstein as LD
import LlingLlang as LL
import VinaryTreeInterop as VTI

function fuzzy_uppercase_example()
    dictionary = LD.DynamicDawg()
    graph = try
        LD.insert_batch!(dictionary,
            ["cat" => nothing, "cot" => nothing, "dog" => nothing])
        view = LD.snapshot(dictionary)
        try
            wfst(view, "cat";
                maximum_distance=1,
                algorithm=ALGORITHM_STANDARD,
                kind=WFST_LEVENSHTEIN)
        finally
            close(view)
        end
    finally
        close(dictionary)
    end

    try
        @assert VTI.weight_domain(graph) == VTI.WEIGHT_TROPICAL_F64

        mapper = LL.WfstBuilder(size_hint=1)
        state = LL.add_state!(mapper)
        LL.set_start!(mapper, state)
        LL.set_final!(mapper, state)
        for character in ['a', 'c', 'o', 't']
            LL.add_arc!(mapper, state, character, uppercase(character), state)
        end
        uppercase_graph = LL.build!(mapper)
        try
            product = LL.compose(graph, uppercase_graph)
            try
                product_view = VTI.snapshot(product)
                try
                    @assert VTI.weight_domain(product_view) ==
                        VTI.WEIGHT_TROPICAL_F64
                    @assert !isempty(VTI.arcs(product_view,
                        VTI.start(product_view)))
                    "captured and composed"
                finally
                    close(product_view)
                end
            finally
                close(product)
            end
        finally
            close(uppercase_graph)
        end
    finally
        close(graph)
    end
end

fuzzy_uppercase_example()
```

The source dictionary can close before the fuzzy graph is traversed because
`wfst` retains the captured revision. Composition retains each graph again;
closing the product and its snapshot does not change either operand.

## Native WallBreaker result graph

`wallbreaker_wfst` uses the bounded native liblevenshtein WallBreaker matcher
and builds an owned tropical result forest. Its finite graph can outlive the
source dictionary. Native expansion-cache statistics, clearing, and policy
changes are available through `cache_statistics`, `clear_cache!`, and
`set_cache_policy!` on the returned `WallBreakerGraph`. The package README
describes its limits and supported algorithms.

```@example wallbreaker
using Duallity
import Libdictenstein as LD
import VinaryTreeInterop as VTI

dictionary = LD.Scdawg()
try
    LD.insert_batch!(dictionary, ["cat" => nothing, "cot" => nothing])
    wall = wallbreaker_wfst(dictionary, "cat"; maximum_distance=1)
    try
        @assert wallbreaker_statistics(wall).results == 2
        VTI.start(wall.graph)
    finally
        close(wall)
    end
finally
    close(dictionary)
end
```

## Native FZF ranking and configured graph

FZF ranks an ordered query subsequence inside each dictionary term. The
`FZF_SCHEME_PATH` bonus table favors path boundaries; `top_k` selects an exact
ranked result capacity. Native traversal shares prefix score columns and
returns work counters. A configured graph uses Arctic weights, where higher
scores are better; close its owner after use.

```@example fzf
using Duallity
import Libdictenstein as LD
import VinaryTreeInterop as VTI

dictionary = LD.DynamicDawg()
try
    LD.insert_batch!(dictionary,
        ["foo/bar" => nothing, "FooBar" => nothing, "far" => nothing])
    options = FzfOptions(scheme=FZF_SCHEME_PATH, top_k=2,
        max_query_chars=16, max_candidate_chars=64,
        max_work_units=1_000, cache_policy=LRU, cache_capacity=2)
    ranking = fzf_rank(dictionary, "fb"; options)
    @assert length(ranking.hits) <= 2
    @assert ranking.statistics.work_units <= options.max_work_units
    graph = fzf_wfst(dictionary, "fb"; options)
    try
        @assert VTI.weight_domain(graph.graph) == VTI.WEIGHT_ARCTIC_F64
        clear_cache!(graph)
        set_cache_policy!(graph, NO_CACHE)
        @assert effective_fzf_options(graph).cache_policy == NO_CACHE
    finally
        close(graph)
    end
finally
    close(dictionary)
end
```

The [FZF design note](https://github.com/vinary-tree/duallity/blob/master/docs/design/fzf-wfst.md)
derives the score bound and explains why the graph uses Arctic weights.

## Public API

```@autodocs
Modules = [Duallity]
Private = false
```
