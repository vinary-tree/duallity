"""Counts for a native WallBreaker result forest."""
struct WallBreakerStatistics
    results::Int
    states::Int
    arcs::Int
end

"""An owned Unicode scalar WFST and its cache-controlled native handle."""
mutable struct WallBreakerGraph{G}
    graph::G
    handle::Ptr{Cvoid}
    query::String
    maximum_distance::Int
    algorithm::Algorithm
    statistics::WallBreakerStatistics
end

Base.isopen(value::WallBreakerGraph) = value.handle != C_NULL

function Base.close(value::WallBreakerGraph)
    handle = value.handle
    handle == C_NULL && return nothing
    value.handle = C_NULL
    try
        close(value.graph)
    finally
        ccall(native(:duallity_wfst_free), Cvoid, (Ptr{Cvoid},), handle)
    end
    nothing
end

function live_handle(value::WallBreakerGraph)
    isopen(value) || throw(ArgumentError("WallBreaker WFST is closed"))
    value.handle
end

"""Copy cumulative native expansion-cache counters and current residency."""
function cache_statistics(value::WallBreakerGraph)
    result = Ref(DuallityCacheStatisticsV1())
    checked(ccall(native(:duallity_wfst_cache_statistics), UInt32,
        (Ptr{Cvoid}, Ref{DuallityCacheStatisticsV1}), live_handle(value), result),
        :duallity_wfst_cache_statistics)
    result[]
end

"""Clear cached state expansions while preserving graph semantics."""
function clear_cache!(value::WallBreakerGraph)
    checked(ccall(native(:duallity_wfst_cache_clear), UInt32,
        (Ptr{Cvoid},), live_handle(value)), :duallity_wfst_cache_clear)
    value
end

"""Atomically change the native cache policy and clear old residency."""
function set_cache_policy!(value::WallBreakerGraph, policy::CachePolicy;
    capacity::Integer=0)
    0 <= capacity <= typemax(UInt64) ||
        throw(ArgumentError("cache capacity must fit UInt64"))
    checked(ccall(native(:duallity_wfst_cache_set_policy), UInt32,
        (Ptr{Cvoid}, UInt32, UInt64), live_handle(value), UInt32(policy),
        UInt64(capacity)), :duallity_wfst_cache_set_policy)
    value
end

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
        algorithm=ALGORITHM_STANDARD, limits=Liblevenshtein.WallBreakerLimits(),
        cache_policy=CACHE_ALL, cache_capacity=0)

Capture one Unicode dictionary revision and run the bounded native WallBreaker
matcher over its complete terms. The resulting tropical identity WFST owns its
match forest, so it can outlive `dictionary`; each match has its exact edit
distance as final weight. `result.graph` composes with `LlingLlang.compose`.
Construction is eager and bounded by `limits`. State expansion uses the native
provider cache; `cache_statistics`, `clear_cache!`, and `set_cache_policy!`
operate on that cache while the handle is open.
"""
function wallbreaker_wfst(dictionary::LD.Dictionary, query::AbstractString;
    maximum_distance::Integer=1, algorithm::Algorithm=ALGORITHM_STANDARD,
    limits::Liblevenshtein.WallBreakerLimits=Liblevenshtein.WallBreakerLimits(),
    cache_policy::CachePolicy=CACHE_ALL, cache_capacity::Integer=0)
    0 <= maximum_distance <= 8 ||
        throw(ArgumentError("WallBreaker maximum_distance must be in 0:8"))
    0 <= cache_capacity <= typemax(UInt64) ||
        throw(ArgumentError("cache capacity must fit UInt64"))
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
    query_bytes = Vector{UInt8}(codeunits(String(query)))
    term_bytes = [Vector{UInt8}(codeunits(match.term)) for match in matches]
    records = [DuallityWallBreakerResultV1(
        record_header(DuallityWallBreakerResultV1),
        isempty(bytes) ? Ptr{UInt8}(C_NULL) : pointer(bytes),
        UInt64(length(bytes)), UInt64(match.distance), (UInt64(0), UInt64(0)))
        for (match, bytes) in zip(matches, term_bytes)]
    output = Ref{Ptr{Cvoid}}(C_NULL)
    status = GC.@preserve query_bytes term_bytes records begin
        ccall(native(:duallity_wallbreaker_wfst_new_results), UInt32,
            (Ptr{UInt8}, Csize_t, UInt32, UInt64,
                Ptr{DuallityWallBreakerResultV1}, UInt64, UInt64, UInt32, UInt64,
                Ref{Ptr{Cvoid}}),
            isempty(query_bytes) ? Ptr{UInt8}(C_NULL) : pointer(query_bytes),
            length(query_bytes), UInt32(algorithm), UInt64(maximum_distance),
            isempty(records) ? Ptr{DuallityWallBreakerResultV1}(C_NULL) : pointer(records),
            UInt64(length(records)),
            isempty(records) ? UInt64(0) : UInt64(sizeof(DuallityWallBreakerResultV1)),
            UInt32(cache_policy), UInt64(cache_capacity), output)
    end
    checked(status, :duallity_wallbreaker_wfst_new_results)
    handle = output[]
    handle != C_NULL || throw(NativeError(STATUS_PANIC,
        :duallity_wallbreaker_wfst_new_results, "native returned a null handle"))
    try
        arcs = sum((length(match.term) for match in matches); init=0)
        result = WallBreakerGraph(adopted_wfst(handle), handle, String(query),
            Int(maximum_distance), algorithm,
            WallBreakerStatistics(length(matches), arcs + 1, arcs))
        finalizer(close, result)
        result
    catch
        ccall(native(:duallity_wfst_free), Cvoid, (Ptr{Cvoid},), handle)
        rethrow()
    end
end
