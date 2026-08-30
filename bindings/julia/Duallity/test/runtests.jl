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

@testset "ABI and all public selectors" begin
    @test abi_version() == ABI_VERSION == 1
    @test api_revision() >= API_REVISION == 2

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
    product = product_automaton(graph, mapper)
    snapshot = VTI.snapshot(product)
    close(product)
    close(graph)
    close(mapper)
    @test language(snapshot) == Dict("CAT" => 0.0, "COT" => 1.0)
    close(snapshot)
    close(dictionary)
end

@testset "multi-input product preserves caller ownership" begin
    first = case_mapper(['a', 'b'])
    second = case_mapper(['A', 'B'])
    third = case_mapper(['A', 'B'])
    product = product_automaton(first, second, third)
    try
        @test length(VTI.arcs(product, VTI.start(product))) == 2
        @test VTI.start(first) >= 0
        @test VTI.start(second) >= 0
        @test VTI.start(third) >= 0
    finally
        close(product)
        close(third)
        close(second)
        close(first)
    end
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
