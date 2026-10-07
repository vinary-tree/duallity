"""Native FZF scoring and traversal options. Lengths count Unicode scalars."""
Base.@kwdef struct FzfOptions
    case_sensitive::Bool = false
    scheme::FzfScheme = FZF_SCHEME_DEFAULT
    top_k::Int = 0
    max_query_chars::Int = 1_000
    max_candidate_chars::Int = 1_000_000
    max_work_units::Int = 100_000
    cache_policy::CachePolicy = CACHE_ALL
    cache_capacity::UInt64 = 0
end

"""One exact FZF score. `score` is absent when the candidate does not match."""
struct FzfScore
    score::Union{Nothing,Int32}
    maximum_score::Int32
end

"""A copied ranked term and its native FZF score."""
struct FzfHit
    term::String
    score::Int32
end

"""Native prefix traversal counters and retained result count."""
struct FzfStatistics
    columns_computed::UInt64
    candidates_scored::UInt64
    prefixes_pruned::UInt64
    score_bound_prefixes_pruned::UInt64
    length_prefixes_pruned::UInt64
    upper_bounds_computed::UInt64
    result_count::UInt64
    work_units::UInt64
end

"""A bounded ranking copied from an immutable dictionary revision."""
struct FzfRanking
    hits::Vector{FzfHit}
    statistics::FzfStatistics
end

function checked_fzf_bound(value::Integer, maximum::Integer, name::String)
    0 <= value <= maximum ||
        throw(ArgumentError("$name must be between 0 and $maximum"))
    UInt64(value)
end

function fzf_wire_options(options::FzfOptions)
    top_k = checked_fzf_bound(options.top_k, FZF_MAX_TOP_K, "top_k")
    max_query_chars = checked_fzf_bound(options.max_query_chars, 1_000,
        "max_query_chars")
    max_candidate_chars = checked_fzf_bound(options.max_candidate_chars,
        1_000_000, "max_candidate_chars")
    max_work_units = checked_fzf_bound(options.max_work_units,
        FZF_MAX_WORK_UNITS, "max_work_units")
    cache_capacity = checked_fzf_bound(options.cache_capacity,
        typemax(UInt64), "cache_capacity")
    raw = Ref(DuallityFzfConfigV1())
    checked(ccall(native(:duallity_fzf_config_default), UInt32,
        (Ref{DuallityFzfConfigV1},), raw), :duallity_fzf_config_default)
    DuallityFzfConfigV1(raw[].header, UInt32(options.case_sensitive),
        UInt32(options.scheme), top_k, max_query_chars, max_candidate_chars,
        max_work_units, UInt32(options.cache_policy), UInt32(0),
        cache_capacity, (UInt64(0), UInt64(0)))
end

fzf_bytes(value::AbstractString) = Vector{UInt8}(codeunits(String(value)))
fzf_pointer(bytes::Vector{UInt8}) = isempty(bytes) ? C_NULL : pointer(bytes)

"""
    fzf_score(query, candidate; options=FzfOptions())

Score one candidate through duallity's exact native FZF recurrence. A result
with `score === nothing` denotes no subsequence match.
"""
function fzf_score(query::AbstractString, candidate::AbstractString;
    options::FzfOptions=FzfOptions())
    query_bytes = fzf_bytes(query)
    candidate_bytes = fzf_bytes(candidate)
    config = Ref(fzf_wire_options(options))
    result = Ref(DuallityFzfScoreV1())
    status = GC.@preserve query_bytes candidate_bytes config result begin
        ccall(native(:duallity_fzf_score), UInt32,
            (Ptr{UInt8}, Csize_t, Ptr{UInt8}, Csize_t,
                Ref{DuallityFzfConfigV1}, Ref{DuallityFzfScoreV1}),
            fzf_pointer(query_bytes), length(query_bytes),
            fzf_pointer(candidate_bytes), length(candidate_bytes),
            config, result)
    end
    checked(status, :duallity_fzf_score)
    FzfScore(result[].matched == 0 ? nothing : result[].score,
        result[].maximum_score)
end

"""
    fzf_wfst(dictionary, query; options=FzfOptions())

Capture the dictionary once and return an owned Arctic-weighted FZF WFST.
`result.graph` may outlive the input dictionary; close `result` when done.
Cache controls and statistics use the same methods as other configured WFSTs.
"""
function fzf_wfst(dictionary::Union{VTI.Resource,VTI.Dictionary},
    query::AbstractString; options::FzfOptions=FzfOptions())
    query_bytes = fzf_bytes(query)
    config = Ref(fzf_wire_options(options))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    resource = Ref(raw_resource(dictionary))
    status = GC.@preserve dictionary query_bytes config resource output begin
        ccall(native(:duallity_fzf_wfst_new_ref), UInt32,
            (Ref{VTI.VtResourceRaw}, Ptr{UInt8}, Csize_t,
                Ref{DuallityFzfConfigV1}, Ref{Ptr{Cvoid}}),
            resource, fzf_pointer(query_bytes), length(query_bytes),
            config, output)
    end
    checked(status, :duallity_fzf_wfst_new_ref)
    handle = output[]
    handle != C_NULL || throw(NativeError(STATUS_PANIC,
        :duallity_fzf_wfst_new_ref, "native returned a null handle"))
    try
        result = ConfiguredWfst(adopted_wfst(handle), handle)
        finalizer(close, result)
        result
    catch
        ccall(native(:duallity_wfst_free), Cvoid, (Ptr{Cvoid},), handle)
        rethrow()
    end
end

function fzf_wfst(dictionary::LD.Dictionary, query::AbstractString;
    options::FzfOptions=FzfOptions())
    view = LD.snapshot(dictionary)
    try
        fzf_wfst(view, query; options)
    finally
        close(view)
    end
end

"""Read the captured FZF options and current effective cache settings."""
function effective_fzf_options(value::ConfiguredWfst)
    result = Ref(DuallityFzfConfigV1())
    checked(ccall(native(:duallity_fzf_wfst_config_get), UInt32,
        (Ptr{Cvoid}, Ref{DuallityFzfConfigV1}), live_handle(value), result),
        :duallity_fzf_wfst_config_get)
    raw = result[]
    FzfOptions(raw.case_sensitive != 0, FzfScheme(raw.scheme),
        Int(raw.top_k), Int(raw.max_query_chars), Int(raw.max_candidate_chars),
        Int(raw.max_work_units), CachePolicy(raw.cache_policy),
        raw.cache_capacity)
end

function copied_fzf_statistics(raw::DuallityFzfStatisticsV1)
    FzfStatistics(raw.columns_computed, raw.candidates_scored,
        raw.prefixes_pruned, raw.score_bound_prefixes_pruned,
        raw.length_prefixes_pruned, raw.upper_bounds_computed,
        raw.result_count, raw.work_units)
end

"""
    fzf_rank(dictionary, query; options=FzfOptions(top_k=10))

Return the exact top `top_k` native FZF matches, ordered by score descending
then UTF-8 term ascending. The native traversal has a hard work limit; if it
would exceed `max_work_units`, the call fails without returning partial hits.
"""
function fzf_rank(dictionary::Union{VTI.Resource,VTI.Dictionary},
    query::AbstractString; options::FzfOptions=FzfOptions(top_k=10))
    options.top_k > 0 || throw(ArgumentError("FZF ranking requires top_k > 0"))
    query_bytes = fzf_bytes(query)
    config = Ref(fzf_wire_options(options))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    resource = Ref(raw_resource(dictionary))
    status = GC.@preserve dictionary query_bytes config resource output begin
        ccall(native(:duallity_fzf_rank_ref), UInt32,
            (Ref{VTI.VtResourceRaw}, Ptr{UInt8}, Csize_t,
                Ref{DuallityFzfConfigV1}, Ref{Ptr{Cvoid}}),
            resource, fzf_pointer(query_bytes), length(query_bytes),
            config, output)
    end
    checked(status, :duallity_fzf_rank_ref)
    handle = output[]
    handle != C_NULL || throw(NativeError(STATUS_PANIC,
        :duallity_fzf_rank_ref, "native returned a null handle"))
    try
        count = Ref{UInt64}(0)
        checked(ccall(native(:duallity_fzf_ranking_len), UInt32,
            (Ptr{Cvoid}, Ref{UInt64}), handle, count),
            :duallity_fzf_ranking_len)
        count[] <= FZF_MAX_TOP_K ||
            throw(ArgumentError("native ranking exceeded the FZF result limit"))
        hits = FzfHit[]
        sizehint!(hits, Int(count[]))
        for index in 0:(Int(count[]) - 1)
            raw = Ref(DuallityFzfHitV1())
            checked(ccall(native(:duallity_fzf_ranking_get), UInt32,
                (Ptr{Cvoid}, UInt64, Ref{DuallityFzfHitV1}),
                handle, UInt64(index), raw), :duallity_fzf_ranking_get)
            text = raw[].term_len == 0 ? "" : begin
                raw[].term_data != C_NULL ||
                    throw(ArgumentError("native hit has null text"))
                raw[].term_len <= 1_000_000 * 4 ||
                    throw(ArgumentError("native hit text exceeds configured limit"))
                String(copy(unsafe_wrap(Vector{UInt8}, raw[].term_data,
                    Int(raw[].term_len))))
            end
            push!(hits, FzfHit(text, raw[].score))
        end
        statistics = Ref(DuallityFzfStatisticsV1())
        checked(ccall(native(:duallity_fzf_ranking_statistics), UInt32,
            (Ptr{Cvoid}, Ref{DuallityFzfStatisticsV1}),
            handle, statistics), :duallity_fzf_ranking_statistics)
        FzfRanking(hits, copied_fzf_statistics(statistics[]))
    finally
        ccall(native(:duallity_fzf_ranking_free), Cvoid, (Ptr{Cvoid},), handle)
    end
end

function fzf_rank(dictionary::LD.Dictionary, query::AbstractString;
    options::FzfOptions=FzfOptions(top_k=10))
    view = LD.snapshot(dictionary)
    try
        fzf_rank(view, query; options)
    finally
        close(view)
    end
end
