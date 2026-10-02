# The release gate runs this in a clean General project; source CI exercises the
# same consumer against the coordinated local family before publication.
using Duallity
import Libdictenstein as LD
import LlingLlang as LL
import VinaryTreeInterop as VTI

dictionary = LD.DynamicDawg()
try
    LD.insert_batch!(dictionary, ["cat" => nothing, "cot" => nothing])
    view = LD.snapshot(dictionary)
    graph = try
        wfst(view, "cat"; maximum_distance=1)
    finally
        close(view)
    end
    try
        VTI.weight_domain(graph) == VTI.WEIGHT_TROPICAL_F64 ||
            error("registered graph has the wrong weight domain")
        !isempty(VTI.arcs(graph, VTI.start(graph))) ||
            error("registered graph has no reachable fuzzy transitions")

        mapper = LL.WfstBuilder(size_hint=1)
        state = LL.add_state!(mapper)
        LL.set_start!(mapper, state)
        LL.set_final!(mapper, state)
        for character in ['a', 'c', 'o', 't']
            LL.add_arc!(mapper, state, character, uppercase(character), state)
        end
        uppercase_graph = LL.build!(mapper)
        try
            product = LL.compose(graph, uppercase_graph)
            try
                product_view = VTI.snapshot(product)
                try
                    !isempty(VTI.arcs(product_view, VTI.start(product_view))) ||
                        error("registered family composition has no reachable arcs")
                finally
                    close(product_view)
                end
            finally
                close(product)
            end
        finally
            close(uppercase_graph)
        end
    finally
        close(graph)
    end
finally
    close(dictionary)
end

println("General-installed Duallity Julia family consumer passed")
