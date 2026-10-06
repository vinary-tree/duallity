"""Counts for an eagerly verified, finite WallBreaker result forest."""
struct WallBreakerStatistics
    results::Int
    states::Int
    arcs::Int
end

"""An owned, immutable Unicode scalar WFST built from native WallBreaker matches."""
mutable struct WallBreakerGraph{G}
    graph::G
    query::String
    maximum_distance::Int
    algorithm::Algorithm
    statistics::WallBreakerStatistics
end

Base.close(value::WallBreakerGraph) = close(value.graph)
Base.isopen(value::WallBreakerGraph) = isopen(value.graph)
wallbreaker_statistics(value::WallBreakerGraph) = value.statistics

function wallbreaker_algorithm(value::Algorithm)
    if value == ALGORITHM_STANDARD
        return Liblevenshtein.ALGORITHM_STANDARD
    elseif value == ALGORITHM_TRANSPOSITION
        return Liblevenshtein.ALGORITHM_TRANSPOSITION
    elseif value == ALGORITHM_MERGE_AND_SPLIT
        return Liblevenshtein.ALGORITHM_MERGE_AND_SPLIT
    end
    throw(ArgumentError("native WallBreaker does not support $value"))
end

"""
    wallbreaker_wfst(dictionary, query; maximum_distance=1,
        algorithm=ALGORITHM_STANDARD, limits=Liblevenshtein.WallBreakerLimits())

Capture one Unicode dictionary revision, run the bounded native WallBreaker
matcher over its complete terms, and construct an owned tropical identity WFST.
Each match produces one chain and its exact edit distance as final weight.
The graph can outlive `dictionary` and composes with `LlingLlang.compose` through
`result.graph`. Construction is eager and bounded by `limits`; the resulting
finite graph has no mutable state cache.
"""
function wallbreaker_wfst(dictionary::LD.Dictionary, query::AbstractString;
    maximum_distance::Integer=1, algorithm::Algorithm=ALGORITHM_STANDARD,
    limits::Liblevenshtein.WallBreakerLimits=Liblevenshtein.WallBreakerLimits())
    0 <= maximum_distance <= 8 ||
        throw(ArgumentError("WallBreaker maximum_distance must be in 0:8"))
    native_algorithm = wallbreaker_algorithm(algorithm)
    view = LD.snapshot(dictionary)
    terms = String[]
    bytes = 0
    try
        VTI.unit_domain(view) == VTI.UNIT_UNICODE_SCALAR ||
            throw(ArgumentError("WallBreaker requires Unicode scalar terms"))
        for entry in view
            term = String(first(entry))
            length(terms) < limits.max_terms ||
                throw(ArgumentError("WallBreaker term limit exceeded"))
            length(term) <= limits.max_term_scalars ||
                throw(ArgumentError("WallBreaker term scalar limit exceeded"))
            bytes += ncodeunits(term)
            bytes <= limits.max_total_term_bytes ||
                throw(ArgumentError("WallBreaker term byte limit exceeded"))
            push!(terms, term)
        end
    finally
        close(view)
    end
    matcher = Liblevenshtein.WallBreakerMatcher(terms;
        max_distance=maximum_distance, algorithm=native_algorithm, limits)
    matches = Liblevenshtein.WallBreakerMatch[]
    try
        cursor = Liblevenshtein.query(matcher, query)
        try
            append!(matches, cursor)
        finally
            close(cursor)
        end
    finally
        close(matcher)
    end
    builder = LlingLlang.WfstBuilder{Char,LlingLlang.TropicalWeight}()
    try
        root = LlingLlang.add_state!(builder)
        LlingLlang.set_start!(builder, root)
        arcs = 0
        states = 1
        for match in matches
            source = root
            for label in match.term
                target = LlingLlang.add_state!(builder)
                LlingLlang.add_arc!(builder, source, label, label, target)
                source = target
                arcs += 1
                states += 1
            end
            LlingLlang.set_final!(builder, source, match.distance)
        end
        graph = LlingLlang.build!(builder)
        WallBreakerGraph(graph, String(query), Int(maximum_distance), algorithm,
            WallBreakerStatistics(length(matches), states, arcs))
    finally
        close(builder)
    end
end
