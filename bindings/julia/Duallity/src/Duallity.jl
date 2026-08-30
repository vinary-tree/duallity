module Duallity

using Libdl
import VinaryTreeInterop

const VTI = VinaryTreeInterop
include("GeneratedAbi.jl")

@doc "Native duallity ABI version required by this facade." ABI_VERSION
@doc "Minimum additive duallity API revision required by this facade." API_REVISION
@doc "Stable status returned by the duallity C ABI." Status
@doc "Edit-operation family used by the parameterized Levenshtein adapter." Algorithm
@doc "The nine public duallity weighted-transducer variants." WfstKind

export ABI_VERSION,
    API_REVISION,
    Status,
    Algorithm,
    WfstKind,
    NativeError,
    abi_version,
    api_revision,
    wfst,
    ALGORITHM_STANDARD,
    ALGORITHM_TRANSPOSITION,
    ALGORITHM_MERGE_AND_SPLIT,
    ALGORITHM_DAMERAU_LEVENSHTEIN,
    WFST_LEVENSHTEIN,
    WFST_UNIVERSAL_STANDARD,
    WFST_UNIVERSAL_TRANSPOSITION,
    WFST_UNIVERSAL_MERGE_AND_SPLIT,
    WFST_GENERALIZED_STANDARD,
    WFST_GENERALIZED_TRANSPOSITION,
    WFST_GENERALIZED_MERGE_AND_SPLIT,
    WFST_GENERALIZED_PHONETIC,
    WFST_FZF

"""A copied native failure with its stable status, operation, and diagnostic."""
struct NativeError <: Exception
    status::Status
    operation::Symbol
    message::String
end

function Base.showerror(io::IO, error::NativeError)
    print(io, error.operation, " failed with ", error.status)
    isempty(error.message) || print(io, ": ", error.message)
end

const LIBRARY_HANDLE = Ref{Ptr{Cvoid}}(C_NULL)

function library_candidates()
    names = Sys.iswindows() ? ["duallity.dll"] :
        Sys.isapple() ? ["libduallity.dylib"] : ["libduallity.so"]
    explicit = get(ENV, "DUALLITY_LIBRARY", "")
    isempty(explicit) ? names : vcat([explicit], names)
end

function library_handle()
    LIBRARY_HANDLE[] != C_NULL && return LIBRARY_HANDLE[]
    failures = String[]
    for candidate in library_candidates()
        try
            LIBRARY_HANDLE[] = Libdl.dlopen(candidate)
            return LIBRARY_HANDLE[]
        catch error
            push!(failures, "$candidate: $(sprint(showerror, error))")
        end
    end
    error("could not load duallity; set DUALLITY_LIBRARY\n" *
        join(failures, "\n"))
end

native(name::Symbol) = Libdl.dlsym(library_handle(), name)

"""Return the ABI version exported by the loaded native library."""
abi_version() = UInt32(ccall(native(:duallity_abi_version), UInt32, ()))

"""Return the additive API revision exported by the loaded native library."""
api_revision() = UInt32(ccall(native(:duallity_api_revision), UInt32, ()))

function last_error_message()
    pointer = ccall(native(:duallity_last_error_message), Cstring, ())
    pointer == C_NULL ? "" : unsafe_string(pointer)
end

function checked(code::Integer, operation::Symbol)
    status = Status(UInt32(code))
    status == STATUS_OK && return nothing
    throw(NativeError(status, operation, last_error_message()))
end

raw_resource(resource::VTI.Resource) = VTI.raw_resource(resource)
raw_resource(dictionary::VTI.Dictionary) = VTI.raw_resource(dictionary.resource)

function adopted_wfst(handle::Ptr{Cvoid})
    raw = Ref(VTI.VtResourceRaw(C_NULL, Ptr{VTI.VtResourceVTable}(C_NULL)))
    checked(ccall(native(:duallity_wfst_resource), UInt32,
        (Ptr{Cvoid}, Ref{VTI.VtResourceRaw}), handle, raw),
        :duallity_wfst_resource)
    VTI.wfstransducer(VTI.adopt_resource(raw[]); take=true)
end

"""
    wfst(dictionary, query; maximum_distance=1,
         algorithm=ALGORITHM_STANDARD, kind=WFST_LEVENSHTEIN)

Capture the current immutable revision of a Unicode-scalar dictionary and
return one owned lazy weighted finite-state transducer. The result implements
Vinary Tree's `vt.scalar-wfst.1` interface and composes directly with
`LlingLlang.compose`.

The nine `WfstKind` values select parameterized, universal, generalized,
phonetic, or FZF adapters. FZF uses Arctic (max-plus) weights; every other kind
uses tropical (min-plus) weights. Closing the input after this call does not
invalidate the returned graph.
"""
function wfst(dictionary::Union{VTI.Resource,VTI.Dictionary},
    query::AbstractString; maximum_distance::Integer=1,
    algorithm::Algorithm=ALGORITHM_STANDARD,
    kind::WfstKind=WFST_LEVENSHTEIN)
    maximum_distance >= 0 ||
        throw(ArgumentError("maximum_distance cannot be negative"))
    bytes = Vector{UInt8}(codeunits(String(query)))
    output = Ref{Ptr{Cvoid}}(C_NULL)
    raw = raw_resource(dictionary)
    status = GC.@preserve bytes begin
        ccall(native(:duallity_wfst_new), UInt32,
            (VTI.VtResourceRaw, Ptr{UInt8}, Csize_t, Csize_t, UInt32, UInt32,
                Ref{Ptr{Cvoid}}),
            raw, isempty(bytes) ? C_NULL : pointer(bytes), length(bytes),
            maximum_distance, UInt32(algorithm), UInt32(kind), output)
    end
    checked(status, :duallity_wfst_new)
    try
        adopted_wfst(output[])
    finally
        ccall(native(:duallity_wfst_free), Cvoid, (Ptr{Cvoid},), output[])
    end
end

function __init__()
    abi_version() == ABI_VERSION || error(
        "duallity ABI $(abi_version()) does not match Julia facade $ABI_VERSION")
    api_revision() >= API_REVISION || error(
        "duallity API revision $(api_revision()) is older than $API_REVISION")
end

end # module
