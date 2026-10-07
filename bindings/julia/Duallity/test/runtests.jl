using Test
using Duallity
import Libdictenstein
import LlingLlang
import Liblevenshtein
import VinaryTreeInterop

const LD = Libdictenstein
const LL = LlingLlang
const LLEV = Liblevenshtein
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

@testset "WallBreaker finite graph matches native results" begin
    terms = ["cat", "cot", "dog", "café"]
    dictionary = LD.Scdawg()
    LD.insert_batch!(dictionary, [term => nothing for term in terms])
    try
        for (algorithm, native_algorithm) in (
            (ALGORITHM_STANDARD, LLEV.ALGORITHM_STANDARD),
            (ALGORITHM_TRANSPOSITION, LLEV.ALGORITHM_TRANSPOSITION),
            (ALGORITHM_MERGE_AND_SPLIT, LLEV.ALGORITHM_MERGE_AND_SPLIT),
        )
            matcher = LLEV.WallBreakerMatcher(terms;
                max_distance=1, algorithm=native_algorithm)
            expected = try
                cursor = LLEV.query(matcher, "cat")
                try
                    Dict(match.term => Float64(match.distance) for match in cursor)
                finally
                    close(cursor)
                end
            finally
                close(matcher)
            end
            result = wallbreaker_wfst(dictionary, "cat";
                maximum_distance=1, algorithm)
            try
                @test language(result.graph) == expected
                stats = wallbreaker_statistics(result)
                @test stats.results == length(expected)
                @test stats.states == stats.arcs + 1
                @test VTI.state_count(result.graph) == stats.states
                @test VTI.weight_domain(result.graph) == VTI.WEIGHT_TROPICAL_F64
                @test cache_statistics(result).misses > 0
                clear_cache!(result)
                @test cache_statistics(result).resident_states == 0
                @test cache_statistics(result).clears >= 1
                set_cache_policy!(result, NO_CACHE)
                @test language(result.graph) == expected
                @test cache_statistics(result).resident_states == 0
                set_cache_policy!(result, LRU; capacity=1)
                @test language(result.graph) == expected
                @test cache_statistics(result).resident_states <= 1
                set_cache_policy!(result, CACHE_ALL)
                mapper = case_mapper(['c', 'a', 't', 'o', 'f', 'é'])
                try
                    product = LL.compose(result.graph, mapper)
                    try
                        @test language(product) ==
                            Dict(uppercase(term) => distance for (term, distance) in expected)
                    finally
                        close(product)
                    end
                finally
                    close(mapper)
                end
            finally
                close(result)
            end
            @test !isopen(result)
            @test_throws ArgumentError cache_statistics(result)
        end
        unicode = wallbreaker_wfst(dictionary, "café";
            maximum_distance=0)
        @test language(unicode.graph) == Dict("café" => 0.0)
        @test_throws ArgumentError wallbreaker_wfst(dictionary, "cat";
            limits=LLEV.WallBreakerLimits(max_terms=1))
        close(dictionary)
        @test language(unicode.graph) == Dict("café" => 0.0)
        close(unicode)
    finally
        isopen(dictionary) && close(dictionary)
    end
    invalid = LD.Scdawg()
    try
        @test_throws ArgumentError wallbreaker_wfst(invalid, "a";
            maximum_distance=9)
        @test_throws ArgumentError wallbreaker_wfst(invalid, "a";
            algorithm=ALGORITHM_DAMERAU_LEVENSHTEIN)
    finally
        close(invalid)
    end
    empty_dictionary = LD.Scdawg()
    try
        empty_result = wallbreaker_wfst(empty_dictionary, "cat")
        @test isempty(language(empty_result.graph))
        @test wallbreaker_statistics(empty_result) == WallBreakerStatistics(0, 1, 0)
        close(empty_result)
    finally
        close(empty_dictionary)
    end
    byte_dictionary = LD.Scdawg(VTI.UNIT_BYTE)
    try
        @test_throws ArgumentError wallbreaker_wfst(byte_dictionary, "cat")
    finally
        close(byte_dictionary)
    end
end

@testset "WallBreaker Unicode and boundary equivalence" begin
    terms = ["a", "aa", "😀", "😀a", "é", "e\u0301", "café", "zzzz"]
    dictionary = LD.Scdawg()
    LD.insert_batch!(dictionary, [term => nothing for term in terms])
    try
        for (algorithm, native_algorithm) in (
            (ALGORITHM_STANDARD, LLEV.ALGORITHM_STANDARD),
            (ALGORITHM_TRANSPOSITION, LLEV.ALGORITHM_TRANSPOSITION),
            (ALGORITHM_MERGE_AND_SPLIT, LLEV.ALGORITHM_MERGE_AND_SPLIT),
        ), (query, maximum_distance) in (("", 0), ("😀", 1),
            ("e\u0301", 1), ("cafe", 2), ("zzzz", 8))
            matcher = LLEV.WallBreakerMatcher(terms;
                max_distance=maximum_distance, algorithm=native_algorithm)
            expected = try
                cursor = LLEV.query(matcher, query)
                try
                    Dict(match.term => Float64(match.distance) for match in cursor)
                finally
                    close(cursor)
                end
            finally
                close(matcher)
            end
            result = wallbreaker_wfst(dictionary, query;
                maximum_distance, algorithm, cache_policy=LRU, cache_capacity=2)
            try
                @test language(result.graph) == expected
                @test cache_statistics(result).resident_states <= 2
            finally
                close(result)
            end
        end
        captured = wallbreaker_wfst(dictionary, "😀"; maximum_distance=1)
        prior = language(captured.graph)
        LD.insert_batch!(dictionary, ["😀x" => nothing])
        @test language(captured.graph) == prior
        close(dictionary)
        @test language(captured.graph) == prior
        close(captured)
    finally
        isopen(dictionary) && close(dictionary)
    end
end

@testset "ABI and all public selectors" begin
    @test abi_version() == ABI_VERSION == 1
    @test api_revision() >= API_REVISION == 6

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

@testset "FZF native scoring, bounded ranking, and cache lifecycle" begin
    @test api_revision() >= 6
    terms = ["foo/bar", "FooBar", "far", "fóó", "zoo", ""]
    dictionary = LD.DynamicDawg()
    LD.insert_batch!(dictionary, [term => nothing for term in terms])
    try
        for scheme in (FZF_SCHEME_DEFAULT, FZF_SCHEME_PATH,
            FZF_SCHEME_HISTORY), case_sensitive in (false, true)
            options = FzfOptions(; scheme, case_sensitive, top_k=3,
                max_query_chars=8, max_candidate_chars=32,
                max_work_units=1_000, cache_policy=LRU, cache_capacity=2)
            expected = [(term, score.score) for term in terms
                for score in (fzf_score("fb", term; options),)
                if score.score !== nothing]
            sort!(expected; by=entry -> (-entry[2], entry[1]))
            result = fzf_rank(dictionary, "fb"; options)
            @test [(hit.term, hit.score) for hit in result.hits] ==
                expected[1:min(3, length(expected))]
            @test result.statistics.result_count == length(result.hits)
            @test result.statistics.columns_computed > 0
            @test result.statistics.work_units > 0
            graph = fzf_wfst(dictionary, "fb"; options)
            try
                @test VTI.weight_domain(graph.graph) == VTI.WEIGHT_ARCTIC_F64
                @test effective_fzf_options(graph) == options
                @test language(graph.graph) ==
                    Dict(term => Float64(score) for (term, score) in expected)
                @test cache_statistics(graph).resident_states <= 2
                clear_cache!(graph)
                @test cache_statistics(graph).resident_states == 0
                set_cache_policy!(graph, NO_CACHE)
                @test effective_fzf_options(graph).cache_policy == NO_CACHE
                @test language(graph.graph) ==
                    Dict(term => Float64(score) for (term, score) in expected)
            finally
                close(graph)
            end
            @test !isopen(graph)
            @test_throws ArgumentError effective_fzf_options(graph)
        end
        @test fzf_score("", "").score == 0
        @test fzf_score("f", "zoo").score === nothing
        empty_ranking = fzf_rank(dictionary, "qqq")
        @test isempty(empty_ranking.hits)
        @test empty_ranking.statistics.result_count == 0
        @test_throws ArgumentError fzf_rank(dictionary, "f";
            options=FzfOptions(top_k=0))
        @test_throws ArgumentError fzf_score("f", "foo";
            options=FzfOptions(max_candidate_chars=-1))
        @test_throws NativeError fzf_rank(dictionary, "f";
            options=FzfOptions(top_k=2, max_work_units=1))
        captured = fzf_wfst(dictionary, "fb";
            options=FzfOptions(top_k=2))
        prior = language(captured.graph)
        LD.insert_batch!(dictionary, ["fbzz" => nothing])
        @test language(captured.graph) == prior
        close(dictionary)
        @test language(captured.graph) == prior
        close(captured)
    finally
        isopen(dictionary) && close(dictionary)
    end
end

@testset "revision-3 raw configuration and cache ownership" begin
    @test api_revision() >= 3
    base = default_options()
    @test base.header.struct_size == sizeof(DuallityWfstOptionsV1)
    @test base.header.record_version == 1

    dictionary = LD.DynamicDawg()
    LD.insert_batch!(dictionary, ["cat" => nothing, "cot" => nothing])
    view = LD.snapshot(dictionary)
    name = Vector{UInt8}(codeunits("custom"))
    source = UInt8['c']
    target = UInt8['c']
    restriction = DuallityRestrictionV1(
        Duallity.record_header(DuallityRestrictionV1),
        pointer(source), length(source), pointer(target), length(target),
        (0, 0))
    restrictions = [restriction]
    operation = DuallityOperationV1(
        Duallity.record_header(DuallityOperationV1), 1, 1, 1.0,
        UInt32(APPLICABILITY_LISTED), 0, pointer(name), length(name),
        pointer(restrictions), 1, sizeof(DuallityRestrictionV1), (0, 0))
    operations = [operation]
    limits = Ref(DuallityGeneralizedLimitsV1(
        Duallity.record_header(DuallityGeneralizedLimitsV1),
        32, 32, 4, 4, 128, 256, 64, 128, (0, 0)))
    options = DuallityWfstOptionsV1(base.header,
        UInt32(WFST_GENERALIZED_STANDARD), base.algorithm, 1,
        UInt32(LRU), 0, 2, Base.unsafe_convert(
            Ptr{DuallityGeneralizedLimitsV1}, limits), pointer(operations),
        1, sizeof(DuallityOperationV1), (0, 0))
    @test_throws ArgumentError configured_wfst(view, "cat", options)
    configured = configured_wfst(view, "cat", options;
        keepalive=(name, source, target, restrictions, operations, limits))
    name[1] = UInt8('X')
    source[1] = UInt8('X')
    copied = effective_options(configured)
    @test copied.kind == WFST_GENERALIZED_STANDARD
    @test copied.cache_policy == LRU
    @test copied.cache_capacity == 2
    @test copied.limits.max_query_bytes == 32
    @test only(copied.operations).name == "custom"
    @test only(only(copied.operations).restrictions).source == "c"
    @test VTI.start(configured.graph) >= 0
    clear_cache!(configured)
    @test cache_statistics(configured).clears >= 1
    set_cache_policy!(configured, NO_CACHE)
    @test effective_options(configured).cache_policy == NO_CACHE
    close(view)
    close(dictionary)
    @test VTI.start(configured.graph) >= 0
    close(configured)
    @test !isopen(configured)
    @test only(copied.operations).name == "custom"
    @test_throws ArgumentError effective_options(configured)

    dictionary = LD.DynamicDawg()
    dictionary["cat"] = nothing
    view = LD.snapshot(dictionary)
    malformed = DuallityWfstOptionsV1(
        DuallityRecordHeaderV1(base.header.struct_size, 1, 1),
        base.kind, base.algorithm, base.maximum_distance,
        base.cache_policy, base.reserved_zero, base.cache_capacity,
        base.limits, base.operations, base.operation_count,
        base.operation_stride, base.reserved)
    try
        @test_throws NativeError configured_wfst(view, "cat", malformed)
    finally
        close(view)
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
