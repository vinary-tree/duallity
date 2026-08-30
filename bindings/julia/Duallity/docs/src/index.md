# Duallity.jl

Duallity.jl captures a Vinary Tree dictionary revision as a lazy fuzzy-query
weighted finite-state transducer and hands it to Julia or lling-llang without
materializing the accepted language. The package
[README](https://github.com/vinary-tree/duallity/tree/master/bindings/julia/Duallity#readme)
defines the nine adapter kinds, four edit algorithms, ownership, composition,
concurrency, security, and executable examples.

Use `product_automaton(first, second, rest...)` to join multiple query,
normalization, grammar, or language-model WFSTs through lling-llang while
keeping caller-owned operands open.

## Public API

```@autodocs
Modules = [Duallity]
Private = false
```
