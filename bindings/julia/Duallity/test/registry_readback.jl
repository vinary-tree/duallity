# Read-only release gate. Run only after the exact Julia General version exists.
# Usage: julia --startup-file=no registry_readback.jl <version> <source-sha> <scratch-root>
using Pkg
using UUIDs

length(ARGS) == 3 || error("expected version, full source SHA, and scratch root")
expected_version, source_sha, scratch_root = ARGS
occursin(r"^[0-9a-f]{40}$", source_sha) ||
    error("source SHA must be a full lowercase Git SHA")
version = VersionNumber(expected_version)
isdir(scratch_root) || error("scratch root does not exist")

for override in ("DUALLITY_LIBRARY", "LIBDICTENSTEIN_LIBRARY",
    "LLING_LLANG_LIBRARY")
    haskey(ENV, override) &&
        error("registered-package readback must not use $override")
end

repository = normpath(joinpath(@__DIR__, "..", "..", "..", ".."))
readchomp(`git -C $repository rev-parse HEAD`) == source_sha ||
    error("checkout is not the reviewed source SHA")
expected_tree = readchomp(`git -C $repository rev-parse $(source_sha * ":bindings/julia/Duallity")`)

mktempdir(abspath(scratch_root)) do consumer
    Pkg.activate(consumer)
    Pkg.Registry.add("General")
    Pkg.add(PackageSpec(name="Duallity", version=expected_version))

    installed = Pkg.dependencies()
    for (name, id) in (
        "Duallity" => UUID("9fdd8473-3d19-49db-b469-45003d1c6a84"),
        "Libdictenstein" => UUID("bafc558c-9074-40dc-b084-d73291979eb7"),
        "LlingLlang" => UUID("c0d6310b-ae37-4a72-96a5-37e0f0aef4be"),
        "VinaryTreeInterop" => UUID("8d6503e5-4d65-4bd8-a8ee-293a0149584e"),
    )
        info = get(installed, id, nothing)
        info === nothing && error("General did not install $name")
        info.version == version ||
            error("General resolved $name $(info.version), not $expected_version")
        if name == "Duallity"
            string(info.tree_hash) == expected_tree ||
                error("General Duallity tree $(info.tree_hash) differs from reviewed $expected_tree")
        end
    end

    run(`$(Base.julia_cmd()) --startup-file=no --project=$consumer
        $(joinpath(@__DIR__, "installed_consumer.jl"))`)
    println("General installed-consumer readback passed: ", expected_version,
        ", source ", source_sha, ", tree ", expected_tree)
end
