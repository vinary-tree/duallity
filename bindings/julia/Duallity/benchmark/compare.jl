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

left = wfst(view, "candidate-5000"; maximum_distance=2)
right = wfst(view, "candidate-5001"; maximum_distance=2)
product_elapsed = @elapsed for _ in 1:iterations
    product = product_automaton(left, right)
    VTI.start(product)
    close(product)
end
println("iterations=$iterations total_seconds=$product_elapsed ns_per_product=",
    product_elapsed * 1e9 / iterations)
close(right)
close(left)
close(view)
close(dictionary)
