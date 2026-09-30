"""An unconditional Unicode-scalar rewrite with nonnegative cost and stable priority."""
struct PhoneticRewriteRule
    input::String
    output::String
    cost::Float64
    priority::Int32
    function PhoneticRewriteRule(input::AbstractString, output::AbstractString;
        cost::Real=0.0, priority::Integer=0)
        value = Float64(cost)
        isfinite(value) && value >= 0 ||
            throw(ArgumentError("rewrite cost must be finite and nonnegative"))
        typemin(Int32) <= priority <= typemax(Int32) ||
            throw(ArgumentError("rewrite priority must fit Int32"))
        new(String(input), String(output), value, Int32(priority))
    end
end

struct PhoneticRuleRaw
    struct_size::UInt32
    record_version::UInt32
    header_reserved::UInt64
    input_data::Ptr{UInt8}
    input_len::Csize_t
    output_data::Ptr{UInt8}
    output_len::Csize_t
    cost::Float64
    priority::Int32
    reserved::UInt32
end

"""A term and the least cost observed within a bounded phonetic traversal."""
struct PhoneticMatch
    term::String
    cost::Float64
end

function phonetic_cache(cache::Symbol, capacity::Integer)
    capacity >= 0 && capacity <= typemax(UInt64) ||
        throw(ArgumentError("cache capacity must fit UInt64 and be nonnegative"))
    policy = cache === :all ? UInt32(0) :
        cache === :none ? UInt32(1) :
        cache === :lru ? UInt32(2) :
        throw(ArgumentError("cache must be :all, :none, or :lru"))
    cache !== :lru && capacity != 0 &&
        throw(ArgumentError("capacity applies only to :lru"))
    policy, UInt64(capacity)
end

function nonnegative_weight(value::Real, name::AbstractString)
    weight = Float64(value)
    isfinite(weight) && weight >= 0 ||
        throw(ArgumentError("$name must be finite and nonnegative"))
    weight
end

function owned_phonetic_wfst(raw::VTI.VtResourceRaw)
    VTI.wfstransducer(VTI.adopt_resource(raw); take=true)
end

"""
    phonetic_nfa(pattern; alphabet=nothing, phonetic_weight=0, cache=:all, capacity=0)

Compile a phonetic regular expression to a standalone, lazy tropical WFST.
`alphabet` is the finite set of Unicode scalars used to expand wide labels;
`nothing` selects the native printable-ASCII default. Literal labels remain
exact even if absent from this set. Own and `close` the returned WFST.
"""
function phonetic_nfa(pattern::AbstractString;
    alphabet::Union{Nothing,AbstractString}=nothing,
    phonetic_weight::Real=0.0, cache::Symbol=:all, capacity::Integer=0)
    bytes = Vector{UInt8}(codeunits(String(pattern)))
    symbols = isnothing(alphabet) ? UInt8[] : Vector{UInt8}(codeunits(String(alphabet)))
    sentinel = UInt8[0x00]
    policy, limit = phonetic_cache(cache, capacity)
    weight = nonnegative_weight(phonetic_weight, "phonetic_weight")
    output = Ref(VTI.VtResourceRaw(C_NULL, Ptr{VTI.VtResourceVTable}(C_NULL)))
    GC.@preserve bytes symbols sentinel begin
        pattern_pointer = isempty(bytes) ? C_NULL : pointer(bytes)
        alphabet_pointer = isnothing(alphabet) ? C_NULL :
            isempty(symbols) ? pointer(sentinel) : pointer(symbols)
        checked(ccall(native(:duallity_phonetic_nfa_new), UInt32,
            (Ptr{UInt8}, Csize_t, Ptr{UInt8}, Csize_t, Float64,
                UInt32, UInt64, Ref{VTI.VtResourceRaw}),
            pattern_pointer, length(bytes), alphabet_pointer, length(symbols),
            weight, policy, limit, output), :duallity_phonetic_nfa_new)
    end
    owned_phonetic_wfst(output[])
end

"""
    phonetic_product(dictionary, pattern; maximum_distance=2,
        phonetic_weight=0, edit_weight=1, cache=:all, capacity=0)

Capture the current Unicode dictionary revision and lazily compose its
phonetic pattern NFA, edit automaton, and dictionary trie. The graph survives
closing or mutating the source; distance is measured in unweighted edits and
must fit `UInt8`. Weights affect ranking, not the edit threshold.
"""
function phonetic_product(dictionary::Union{VTI.Resource,VTI.Dictionary},
    pattern::AbstractString; maximum_distance::Integer=2,
    phonetic_weight::Real=0.0, edit_weight::Real=1.0,
    cache::Symbol=:all, capacity::Integer=0)
    0 <= maximum_distance <= typemax(UInt8) ||
        throw(ArgumentError("maximum_distance must fit UInt8"))
    bytes = Vector{UInt8}(codeunits(String(pattern)))
    policy, limit = phonetic_cache(cache, capacity)
    phonetic = nonnegative_weight(phonetic_weight, "phonetic_weight")
    edit = nonnegative_weight(edit_weight, "edit_weight")
    raw = Ref(raw_resource(dictionary))
    output = Ref(VTI.VtResourceRaw(C_NULL, Ptr{VTI.VtResourceVTable}(C_NULL)))
    GC.@preserve bytes raw begin
        checked(ccall(native(:duallity_phonetic_product_new_ref), UInt32,
            (Ref{VTI.VtResourceRaw}, Ptr{UInt8}, Csize_t, UInt32, Float64,
                Float64, UInt32, UInt64, Ref{VTI.VtResourceRaw}),
            raw, isempty(bytes) ? C_NULL : pointer(bytes), length(bytes),
            UInt32(maximum_distance), phonetic, edit, policy, limit, output),
            :duallity_phonetic_product_new_ref)
    end
    owned_phonetic_wfst(output[])
end

function phonetic_product(dictionary::LD.Dictionary{String}, pattern::AbstractString; kwargs...)
    view = LD.snapshot(dictionary)
    try
        phonetic_product(view, pattern; kwargs...)
    finally
        close(view)
    end
end

"""
    rewrite_wfst(rules; allow_identity=true, cache=:all, capacity=0)

Create a standalone priority-ordered Unicode rewrite WFST. Rules are copied
at construction; an empty rule list with `allow_identity=true` is an identity
transducer. Contextual/locale rules must be expanded explicitly by the caller.
"""
function rewrite_wfst(rules::AbstractVector{PhoneticRewriteRule};
    allow_identity::Bool=true, cache::Symbol=:all, capacity::Integer=0)
    length(rules) <= 4_096 || throw(ArgumentError("at most 4096 rules are supported"))
    policy, limit = phonetic_cache(cache, capacity)
    inputs = [Vector{UInt8}(codeunits(rule.input)) for rule in rules]
    outputs = [Vector{UInt8}(codeunits(rule.output)) for rule in rules]
    records = Vector{PhoneticRuleRaw}(undef, length(rules))
    GC.@preserve inputs outputs begin
        for i in eachindex(rules)
            records[i] = PhoneticRuleRaw(UInt32(sizeof(PhoneticRuleRaw)), 1, 0,
                isempty(inputs[i]) ? C_NULL : pointer(inputs[i]), length(inputs[i]),
                isempty(outputs[i]) ? C_NULL : pointer(outputs[i]), length(outputs[i]),
                rules[i].cost, rules[i].priority, 0)
        end
        output = Ref(VTI.VtResourceRaw(C_NULL, Ptr{VTI.VtResourceVTable}(C_NULL)))
        GC.@preserve records begin
            checked(ccall(native(:duallity_phonetic_rewrite_new), UInt32,
                (Ptr{PhoneticRuleRaw}, Csize_t, UInt8, UInt32, UInt64,
                    Ref{VTI.VtResourceRaw}),
                isempty(records) ? C_NULL : pointer(records), length(records),
                UInt8(allow_identity), policy, limit, output),
                :duallity_phonetic_rewrite_new)
        end
        return owned_phonetic_wfst(output[])
    end
end

"""
    rewrite_wfst(locale::Union{Symbol,AbstractString}; allow_identity=true,
        cache=:all, capacity=0)

Construct the native English (`:en`), German (`:de`), or French (`:fr`)
unconditional rule set. Locale names are case-insensitive; input strings are
*not* case-folded or Unicode-normalized, preserving exact native semantics.
"""
function rewrite_wfst(locale::Union{Symbol,AbstractString};
    allow_identity::Bool=true, cache::Symbol=:all, capacity::Integer=0)
    tag = lowercase(String(locale))
    index = tag in ("en", "english") ? UInt32(0) :
        tag in ("de", "german") ? UInt32(1) :
        tag in ("fr", "french") ? UInt32(2) :
        throw(ArgumentError("locale must be English/en, German/de, or French/fr"))
    policy, limit = phonetic_cache(cache, capacity)
    output = Ref(VTI.VtResourceRaw(C_NULL, Ptr{VTI.VtResourceVTable}(C_NULL)))
    checked(ccall(native(:duallity_phonetic_rewrite_builtin_new), UInt32,
        (UInt32, UInt8, UInt32, UInt64, Ref{VTI.VtResourceRaw}),
        index, UInt8(allow_identity), policy, limit, output),
        :duallity_phonetic_rewrite_builtin_new)
    owned_phonetic_wfst(output[])
end

"""
    phonetic_pipeline(dictionary, pattern; rewrite_rules=[], locale=nothing, kwargs...)

Construct a dictionary-backed phonetic/edit product and optionally compose a
separate custom or built-in-locale rewrite stage before it. `rewrite_rules`
and `locale` are mutually exclusive. Composition is delegated to
LlingLlang.jl, so no phonetic rules or edit logic are reimplemented in Julia.
The returned graph owns its inputs independently; this helper closes
intermediate graphs. `rewrite_cache` and `rewrite_capacity` configure the
separate rewrite stage; other keywords configure the dictionary product.
"""
function phonetic_pipeline(dictionary::Union{VTI.Resource,VTI.Dictionary,LD.Dictionary{String}},
    pattern::AbstractString; rewrite_rules::AbstractVector{PhoneticRewriteRule}=PhoneticRewriteRule[],
    locale::Union{Nothing,Symbol,AbstractString}=nothing, allow_identity::Bool=true,
    rewrite_cache::Symbol=:all, rewrite_capacity::Integer=0, kwargs...)
    !isnothing(locale) && !isempty(rewrite_rules) &&
        throw(ArgumentError("provide either rewrite_rules or locale, not both"))
    product = phonetic_product(dictionary, pattern; kwargs...)
    isempty(rewrite_rules) && isnothing(locale) && return product
    rewrite = nothing
    try
        rewrite = isnothing(locale) ?
            rewrite_wfst(rewrite_rules; allow_identity,
                cache=rewrite_cache, capacity=rewrite_capacity) :
            rewrite_wfst(locale; allow_identity,
                cache=rewrite_cache, capacity=rewrite_capacity)
        LlingLlang.compose(rewrite, product)
    finally
        close(product)
        isnothing(rewrite) || close(rewrite)
    end
end

"""
    phonetic_matches(graph; max_visits=10000, max_output_scalars=128,
        max_results=100, max_cost=Inf)

Enumerate reachable output strings under explicit traversal bounds and return
observed candidates in ascending cost, then lexicographic order. This is a
bounded search, not a claim of globally optimal top-k ranking when a bound
truncates the graph. Repeated outputs keep the lowest observed cost.
"""
function phonetic_matches(graph; max_visits::Integer=10_000,
    max_output_scalars::Integer=128, max_results::Integer=100,
    max_cost::Real=Inf)
    max_visits > 0 && max_output_scalars >= 0 && max_results >= 0 ||
        throw(ArgumentError("traversal bounds must be nonnegative and max_visits positive"))
    bound = Float64(max_cost)
    !isnan(bound) && bound >= 0 ||
        throw(ArgumentError("max_cost must be nonnegative"))
    frontier = [(VTI.start(graph), "", 0.0)]
    best_path = Dict{Tuple{UInt64,String},Float64}()
    matches = Dict{String,Float64}()
    visits = 0
    while !isempty(frontier) && visits < max_visits
        state, term, cost = pop!(frontier)
        cost > bound && continue
        key = (UInt64(state), term)
        cost >= get(best_path, key, Inf) && continue
        best_path[key] = cost
        visits += 1
        info = VTI.state_info(graph, state)
        info === nothing && continue
        if info.final
            final_cost = cost + info.final_weight
            final_cost <= bound &&
                (matches[term] = min(get(matches, term, Inf), final_cost))
        end
        for arc in VTI.arcs(graph, state)
            next_term = isnothing(arc.output) ? term : term * string(Char(UInt32(arc.output)))
            length(next_term) <= max_output_scalars || continue
            next_cost = cost + arc.weight
            next_cost <= bound && push!(frontier, (arc.target, next_term, next_cost))
        end
    end
    ordered = [PhoneticMatch(term, cost) for (term, cost) in matches]
    sort!(ordered; by=match -> (match.cost, match.term))
    ordered[1:min(length(ordered), max_results)]
end
