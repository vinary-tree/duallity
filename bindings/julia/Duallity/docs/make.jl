using Documenter
using Duallity

const DOCS_ROOT = @__DIR__
const BUILD_TARGET = joinpath("target", "build")

makedocs(
    root=DOCS_ROOT,
    sitename="Duallity.jl",
    modules=[Duallity],
    format=Documenter.HTML(
        prettyurls=false,
        repolink="https://github.com/vinary-tree/duallity",
    ),
    pages=["Guide and API" => "index.md"],
    build=BUILD_TARGET,
    checkdocs=:exports,
    repo="https://github.com/vinary-tree/duallity/blob/{commit}{path}#{line}",
    warnonly=false,
)

if get(ENV, "DUALLITY_DOCS_DEPLOY", "0") == "1"
    isfile(joinpath(DOCS_ROOT, BUILD_TARGET, "index.html")) ||
        error("Duallity.jl documentation build is missing its index page")
    deploydocs(
        root=DOCS_ROOT,
        target=BUILD_TARGET,
        repo="github.com/vinary-tree/duallity.git",
        devbranch="master",
    )
end
