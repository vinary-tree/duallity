using Documenter
using Duallity

makedocs(
    sitename="Duallity.jl",
    modules=[Duallity],
    format=Documenter.HTML(
        prettyurls=false,
        repolink="https://github.com/vinary-tree/duallity",
    ),
    pages=["Guide and API" => "index.md"],
    build=get(ENV, "DUALLITY_DOCS_BUILD", "build"),
    checkdocs=:exports,
    repo="https://github.com/vinary-tree/duallity/blob/{commit}{path}#{line}",
    warnonly=false,
)

if get(ENV, "DUALLITY_DOCS_DEPLOY", "0") == "1"
    deploydocs(repo="github.com/vinary-tree/duallity.git", devbranch="master")
end
