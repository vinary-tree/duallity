using Test
using Duallity
import Libdictenstein
import LlingLlang
import VinaryTreeInterop

const LD = Libdictenstein
const LL = LlingLlang
const VTI = VinaryTreeInterop

function language(graph)
    accepted = Dict{String,Float64}()
    stack = [(VTI.start(graph), "", 0.0)]
    visited = 0
    while !isempty(stack)
        state, output, weight = pop!(stack)
        visited += 1
        visited <= 100_000 || error("WFST traversal did not converge")
        info = VTI.state_info(graph, state)
        info === nothing && continue
        if info.final
            candidate = weight + info.final_weight
            accepted[output] = min(get(accepted, output, Inf), candidate)
        end
        for arc in VTI.arcs(graph, state)
            suffix = isnothing(arc.output) ? "" : string(Char(UInt32(arc.output)))
            push!(stack, (arc.target, output * suffix, weight + arc.weight))
        end
    end
    accepted
end

function case_mapper(alphabet)
    builder = LL.WfstBuilder(size_hint=1)
    state = LL.add_state!(builder)
    LL.set_start!(builder, state)
    LL.set_final!(builder, state)
    for character in alphabet
        LL.add_arc!(builder, state, character, uppercase(character), state)
    end
    LL.build!(builder)
end

function raw_view(graph::LL.Wfst)
    VTI.wfstransducer(LL.resource(graph); take=true)
end

@testset "ABI and all public selectors" begin
    @test abi_version() == ABI_VERSION == 1
    @test api_revision() >= API_REVISION == 4

    dictionary = LD.DynamicDawg()
    try
        LD.insert_batch!(dictionary,
            ["cat" => nothing, "cot" => nothing, "dog" => nothing])
        view = LD.snapshot(dictionary)
        try
            for kind in instances(WfstKind)
                graph = wfst(view, "cat"; maximum_distance=1, kind)
                try
                    @test VTI.unit_domain(graph) == VTI.UNIT_UNICODE_SCALAR
                    expected_domain = kind == WFST_FZF ?
                        VTI.WEIGHT_ARCTIC_F64 : VTI.WEIGHT_TROPICAL_F64
                    @test VTI.weight_domain(graph) == expected_domain
                finally
                    close(graph)
                end
            end
            for algorithm in instances(Algorithm)
                graph = wfst(view, "cat"; maximum_distance=1, algorithm)
                @test VTI.start(graph) >= 0
                close(graph)
            end
        finally
            close(view)
        end
    finally
        close(dictionary)
    end
end

@testset "capture-once and lling-llang composition" begin
    dictionary = LD.DynamicDawg()
    LD.insert_batch!(dictionary,
        ["cat" => nothing, "cot" => nothing, "dog" => nothing])
    view = LD.snapshot(dictionary)
    graph = wfst(view, "cat"; maximum_distance=1)
    close(view)
    delete!(dictionary, "cot")
    dictionary["cab"] = nothing

    @test language(graph) == Dict("cat" => 0.0, "cot" => 1.0)

    mapper = case_mapper(['a', 'c', 'o', 't'])
    product = LL.compose(graph, mapper)
    snapshot = VTI.snapshot(product)
    close(product)
    close(graph)
    close(mapper)
    @test language(snapshot) == Dict("CAT" => 0.0, "COT" => 1.0)
    close(snapshot)
    close(dictionary)
end

@testset "argument and ownership failures" begin
    dictionary = LD.DynamicDawg()
    dictionary["cat"] = nothing
    view = LD.snapshot(dictionary)
    @test_throws ArgumentError wfst(view, "cat"; maximum_distance=-1)
    close(view)
    @test_throws VTI.InteropError wfst(view, "cat")
    close(dictionary)
end

@testset "native phonetic family: Unicode, ambiguity, empty and threshold" begin
    @test api_revision() >= 4
    nfa = phonetic_nfa("(ph|f)one"; phonetic_weight=0.25)
    try
        matches = phonetic_matches(nfa; max_visits=100, max_output_scalars=8)
        @test Set(match.term for match in matches) == Set(["phone", "fone"])
        @test all(match.cost >= 0 for match in matches)
    finally
        close(nfa)
    end

    unicode = phonetic_nfa("é"; alphabet="éö", cache=:lru, capacity=2)
    @test [match.term for match in phonetic_matches(unicode)] == ["é"]
    close(unicode)
    decomposed = phonetic_nfa("e\u0301"; alphabet="e\u0301")
    @test [match.term for match in phonetic_matches(decomposed)] == ["e\u0301"]
    close(decomposed)

    ambiguous = phonetic_nfa("(a|a)"; cache=:none)
    @test [match.term for match in phonetic_matches(ambiguous)] == ["a"]
    close(ambiguous)

    @test_throws NativeError phonetic_nfa("")

    dictionary = LD.DynamicDawg()
    LD.insert_batch!(dictionary,
        ["phone" => nothing, "fone" => nothing, "é" => nothing,
            "" => nothing, "bone" => nothing])
    view = LD.snapshot(dictionary)
    exact = phonetic_product(view, "(ph|f)one"; maximum_distance=0)
    close(view)
    delete!(dictionary, "fone")
    try
        matches = phonetic_matches(exact; max_visits=1000, max_output_scalars=8)
        @test Set(match.term for match in matches) == Set(["phone", "fone"])
    finally
        close(exact)
    end
    near = phonetic_product(dictionary, "fone"; maximum_distance=1,
        edit_weight=2.0, cache=:none)
    try
        matches = phonetic_matches(near; max_visits=1000, max_output_scalars=8)
        @test any(match.term == "bone" for match in matches)
        @test all(match.cost >= 0 for match in matches)
    finally
        close(near)
        close(dictionary)
    end

    cache_outputs = Dict{Symbol,Vector{Tuple{String,Float64}}}()
    for policy in (:all, :none, :lru)
        graph = phonetic_nfa("(ph|f)one"; cache=policy,
            capacity=policy == :lru ? 2 : 0)
        try
            cache_outputs[policy] = [(match.term, match.cost)
                for match in phonetic_matches(graph; max_visits=100)]
        finally
            close(graph)
        end
    end
    @test cache_outputs[:all] == cache_outputs[:none] == cache_outputs[:lru]
end

@testset "native rewrite graph and composed pipeline" begin
    rule = PhoneticRewriteRule("ph", "f"; cost=0.5, priority=2)
    @test rule.priority == 2
    rewrite = rewrite_wfst([rule]; allow_identity=false, cache=:lru, capacity=2)
    input = LL.WfstBuilder(size_hint=3)
    first = LL.add_state!(input)
    middle = LL.add_state!(input)
    last = LL.add_state!(input)
    LL.set_start!(input, first)
    LL.set_final!(input, last)
    LL.add_arc!(input, first, 'p', 'p', middle)
    LL.add_arc!(input, middle, 'h', 'h', last)
    source = LL.build!(input)
    source_view = raw_view(source)
    product = LL.compose(source_view, rewrite)
    try
        matches = phonetic_matches(product; max_visits=100, max_output_scalars=4)
        @test any(match.term == "f" && match.cost == 0.5 for match in matches)
    finally
        close(product)
        close(source_view)
        close(source)
        close(rewrite)
    end

    dictionary = LD.DynamicDawg()
    dictionary["fone"] = nothing
    pipeline = phonetic_pipeline(dictionary, "fone";
        rewrite_rules=[rule], maximum_distance=0)
    @test VTI.unit_domain(pipeline) == VTI.UNIT_UNICODE_SCALAR
    close(pipeline)
    localized = phonetic_pipeline(dictionary, "fone";
        locale=:en, maximum_distance=0,
        rewrite_cache=:lru, rewrite_capacity=2)
    @test VTI.weight_domain(localized) == VTI.WEIGHT_TROPICAL_F64
    close(localized)
    @test_throws ArgumentError phonetic_pipeline(dictionary, "fone";
        rewrite_rules=[rule], locale=:en)
    close(dictionary)

    for locale in (:en, "GERMAN", :fr)
        builtin = rewrite_wfst(locale; cache=:none)
        @test VTI.weight_domain(builtin) == VTI.WEIGHT_TROPICAL_F64
        @test !isempty(VTI.arcs(builtin, VTI.start(builtin)))
        close(builtin)
    end
end

@testset "phonetic argument and bounded-search contracts" begin
    @test_throws ArgumentError PhoneticRewriteRule("a", "b"; cost=-1)
    @test_throws ArgumentError PhoneticRewriteRule("a", "b"; cost=Inf)
    @test_throws ArgumentError PhoneticRewriteRule("a", "b"; priority=big(typemax(Int32)) + 1)
    @test_throws ArgumentError phonetic_nfa("a"; cache=:unknown)
    @test_throws ArgumentError phonetic_nfa("a"; phonetic_weight=NaN)
    @test_throws NativeError phonetic_nfa("(")
    @test_throws ArgumentError rewrite_wfst(PhoneticRewriteRule[]; cache=:all, capacity=1)
    @test_throws ArgumentError rewrite_wfst(:unknown)
    dictionary = LD.DynamicDawg()
    @test_throws ArgumentError phonetic_product(dictionary, "a"; maximum_distance=256)
    @test_throws ArgumentError phonetic_product(dictionary, "a"; edit_weight=-1)
    close(dictionary)
    graph = phonetic_nfa("a*")
    @test_throws ArgumentError phonetic_matches(graph; max_visits=0)
    @test length(phonetic_matches(graph; max_visits=5,
        max_output_scalars=2, max_results=2)) <= 2
    close(graph)
end
