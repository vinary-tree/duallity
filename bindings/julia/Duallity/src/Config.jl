"""An owned revision-3 WFST handle and its independent interop graph retain."""
mutable struct ConfiguredWfst{G}
    graph::G
    handle::Ptr{Cvoid}
end

Base.isopen(value::ConfiguredWfst) = value.handle != C_NULL

function Base.close(value::ConfiguredWfst)
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

function live_handle(value::ConfiguredWfst)
    isopen(value) || throw(ArgumentError("configured WFST is closed"))
    value.handle
end

"""Obtain a correctly initialized, writable revision-3 options record."""
function default_options()
    result = Ref(DuallityWfstOptionsV1())
    checked(ccall(native(:duallity_wfst_options_default), UInt32,
        (Ref{DuallityWfstOptionsV1},), result), :duallity_wfst_options_default)
    result[]
end

"""
    configured_wfst(dictionary, query, options; keepalive=nothing)

Construct a lazy WFST from a versioned raw options record. `keepalive` must
hold all Julia arrays and byte buffers referenced by nested non-null pointers.
Native construction deep-copies these inputs before this call returns. The
result owns both a graph retain and a native handle; use `result.graph` for
composition and close the result after use.
"""
function configured_wfst(dictionary::Union{VTI.Resource,VTI.Dictionary},
    query::AbstractString, options::DuallityWfstOptionsV1; keepalive=nothing)
    if (options.limits != C_NULL || options.operations != C_NULL) &&
            keepalive === nothing
        throw(ArgumentError("nested raw pointers require keepalive buffers"))
    end
    bytes = Vector{UInt8}(codeunits(String(query)))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    record = Ref(options)
    raw = raw_resource(dictionary)
    status = GC.@preserve dictionary bytes keepalive record begin
        ccall(native(:duallity_wfst_new_configured_ref), UInt32,
            (Ref{VTI.VtResourceRaw}, Ptr{UInt8}, Csize_t,
                Ref{DuallityWfstOptionsV1}, Ref{Ptr{Cvoid}}),
            Ref(raw), isempty(bytes) ? C_NULL : pointer(bytes), length(bytes),
            record, output)
    end
    checked(status, :duallity_wfst_new_configured_ref)
    handle = output[]
    handle != C_NULL || throw(NativeError(STATUS_PANIC,
        :duallity_wfst_new_configured_ref, "native returned a null handle"))
    try
        result = ConfiguredWfst(adopted_wfst(handle), handle)
        finalizer(close, result)
        result
    catch
        ccall(native(:duallity_wfst_free), Cvoid, (Ptr{Cvoid},), handle)
        rethrow()
    end
end

function copied_text(data::Ptr{UInt8}, length::UInt64)
    length == 0 && return ""
    data != C_NULL || throw(ArgumentError("native returned null text with positive length"))
    length <= CONFIG_MAX_CUSTOM_TEXT_BYTES ||
        throw(ArgumentError("native text exceeds ABI maximum"))
    String(copy(unsafe_wrap(Vector{UInt8}, data, Int(length))))
end

function copied_restrictions(operation::DuallityOperationV1)
    count = Int(operation.restriction_count)
    count <= CONFIG_MAX_RESTRICTION_PAIRS ||
        throw(ArgumentError("native restriction count exceeds ABI maximum"))
    count == 0 && return NamedTuple[]
    operation.restrictions != C_NULL ||
        throw(ArgumentError("native returned null restrictions with positive count"))
    stride = Int(operation.restriction_stride)
    stride >= sizeof(DuallityRestrictionV1) ||
        throw(ArgumentError("native restriction stride is too small"))
    [begin
        item = unsafe_load(Ptr{DuallityRestrictionV1}(
            Ptr{UInt8}(operation.restrictions) + index * stride))
        (source=copied_text(item.source_data, item.source_len),
            target=copied_text(item.target_data, item.target_len))
    end for index in 0:count-1]
end

"""Deep-copy effective configuration before the native handle is released."""
function effective_options(value::ConfiguredWfst)
    result = Ref(DuallityWfstOptionsV1())
    checked(ccall(native(:duallity_wfst_options_get), UInt32,
        (Ptr{Cvoid}, Ref{DuallityWfstOptionsV1}), live_handle(value), result),
        :duallity_wfst_options_get)
    raw = result[]
    count = Int(raw.operation_count)
    count <= CONFIG_MAX_OPERATIONS ||
        throw(ArgumentError("native operation count exceeds ABI maximum"))
    count == 0 || raw.operations != C_NULL ||
        throw(ArgumentError("native returned null operations with positive count"))
    stride = Int(raw.operation_stride)
    count == 0 || stride >= sizeof(DuallityOperationV1) ||
        throw(ArgumentError("native operation stride is too small"))
    operations = [begin
        item = unsafe_load(Ptr{DuallityOperationV1}(
            Ptr{UInt8}(raw.operations) + index * stride))
        (consume_x=item.consume_x, consume_y=item.consume_y,
            weight=item.weight, applicability=OperationApplicability(item.applicability),
            name=copied_text(item.name_data, item.name_len),
            restrictions=copied_restrictions(item))
    end for index in 0:count-1]
    limits = raw.limits == C_NULL ? nothing : begin
        item = unsafe_load(raw.limits)
        (max_query_bytes=item.max_query_bytes,
            max_query_scalars=item.max_query_scalars,
            max_operation_source_scalars=item.max_operation_source_scalars,
            max_operation_query_scalars=item.max_operation_query_scalars,
            max_retained_dictionary_nodes=item.max_retained_dictionary_nodes,
            max_retained_wfst_states=item.max_retained_wfst_states,
            max_paths_per_expansion=item.max_paths_per_expansion,
            max_work_units_per_expansion=item.max_work_units_per_expansion)
    end
    (kind=WfstKind(raw.kind), algorithm=Algorithm(raw.algorithm),
        maximum_distance=raw.maximum_distance,
        cache_policy=CachePolicy(raw.cache_policy), cache_capacity=raw.cache_capacity,
        limits=limits, operations=operations)
end

"""Copy cumulative native cache counters and current residency."""
function cache_statistics(value::ConfiguredWfst)
    result = Ref(DuallityCacheStatisticsV1())
    checked(ccall(native(:duallity_wfst_cache_statistics), UInt32,
        (Ptr{Cvoid}, Ref{DuallityCacheStatisticsV1}), live_handle(value), result),
        :duallity_wfst_cache_statistics)
    result[]
end

"""Clear native cached payloads while preserving state identities."""
function clear_cache!(value::ConfiguredWfst)
    checked(ccall(native(:duallity_wfst_cache_clear), UInt32,
        (Ptr{Cvoid},), live_handle(value)), :duallity_wfst_cache_clear)
    value
end

"""Atomically change native cache policy, clearing previous residency."""
function set_cache_policy!(value::ConfiguredWfst, policy::CachePolicy;
    capacity::Integer=0)
    0 <= capacity <= typemax(UInt64) ||
        throw(ArgumentError("cache capacity must fit UInt64"))
    checked(ccall(native(:duallity_wfst_cache_set_policy), UInt32,
        (Ptr{Cvoid}, UInt32, UInt64), live_handle(value), UInt32(policy),
        UInt64(capacity)), :duallity_wfst_cache_set_policy)
    value
end
