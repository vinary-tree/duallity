# Configurable WFST ABI contract qualification — 28 September 2026

## Question and source boundary

Can the additive revision-3 configuration *record contract* be staged without
changing the shipped ABI version 1/API revision 2, breaking legacy symbols,
or allowing the header to drift from the authoritative binding model?

The tested source is commit `9d5736a` on the duallity
`codex/binding-integration` branch. The model and header SHA-256 values are
`cd1237539c5498ef70f55c6ba39892d1f0e7e2c3a64aa23edcd0ba3a8245a6d3`
and `0e6d1d282693ef83fa828be8b592f109e31cd8588f80ab9b27ad709805401a0e`,
respectively. The [architecture contract](../architecture/07-versioned-configurable-wfst-abi.md)
states the ownership, parsing, compatibility, and cache invariants. The
record declarations are available for compilation but their callable
revision-3 entry points are **not implemented or advertised as shipped** by
this contract step.

## Reproduction and observed results

From the duallity repository root, with the sibling
`../vinary-tree-interop/include` checkout present:

```sh
python3 scripts/check-bindings.py
python3 scripts/check-binding-docs.py
cc -std=c11 -Wall -Wextra -Werror -fsyntax-only \
  -Iinclude -I../vinary-tree-interop/include \
  bindings/c/tests/config_record_layout.c
ruff format --check scripts/check-bindings.py
git diff --check
```

The binding-surface checker passed 74 of 74 checks, including exact ordered
field, selector, revision, and bound comparisons for every new record. The
binding-documentation checker reported `ok (6 adapter facades)`. The C11
layout translation unit compiled with all warnings treated as errors; Ruff
and the whitespace check passed. The unchanged model, Rust, and header
continue to agree on ABI version 1, API revision 2, and exactly eight exported
C symbols. The CI build-and-test job now repeats the C record compile gate.

The rendered ownership diagram was visually inspected against its PlantUML
source. It makes the caller-borrowed input, duallity-owned copied grammar,
captured dictionary revision, and single lling-llang cache owner distinct.
The preview raster was local-only; the committed source and SVG are the
reproducible diagram evidence.

## Boundaries and next experiments

This result proves declaration/model consistency and C compilation on the
tested host, not runtime parsing safety, foreign-language ABI layout,
constructor semantics, cache statistics, or release readiness. The
subsequent constructor and cache-control tasks must test null and malformed
records, old/new C consumers, unsupported combinations, forced-failure
cleanup, provider faults, concurrent calls, and the final foreign mirrors.
The primary lling-llang worktree currently contains another agent's
uncommitted changes and does not yet contain the new shared-cache control
API. Its owner edits remain untouched; integration must use a separately
qualified source graph until a safe merge is possible. No RC6 package was
published by this contract qualification.
