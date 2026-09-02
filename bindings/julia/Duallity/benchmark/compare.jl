using Duallity
import Libdictenstein
import VinaryTreeInterop

const LD = Libdictenstein
const VTI = VinaryTreeInterop

dictionary = LD.DynamicDawg()
LD.insert_batch!(dictionary,
    ["candidate-$index" => nothing for index in 1:10_000])
view = LD.snapshot(dictionary)

iterations = parse(Int, get(ENV, "DUALLITY_BENCH_ITERATIONS", "1000"))
elapsed = @elapsed for _ in 1:iterations
    graph = wfst(view, "candidate-5000"; maximum_distance=2)
    VTI.start(graph)
    close(graph)
end

println("iterations=$iterations total_seconds=$elapsed ns_per_adapter=",
    elapsed * 1e9 / iterations)
close(view)
close(dictionary)
