# C family pipeline test

`family_pipeline.c` is the C-ABI counterpart of the in-process Rust integration
test [`tests/family_pipeline.rs`](../../../tests/family_pipeline.rs). It drives
the whole vinary-tree family across **four separately-built shared libraries in
one process**, communicating only through the stable
[`vinary_tree_interop.h`](../../../include/vinary_tree_interop.h) resource ABI.

## Why four cdylibs

Each crate is built with only its own `--features ffi`, so the exported C symbol
sets are disjoint and never collide when all four are loaded together:

| Shared library          | Exported prefix | Role in the pipeline                                   |
| ----------------------- | --------------- | ------------------------------------------------------ |
| `liblibdictenstein.so`  | `ldict_*`       | Producer of a `vt.dictionary.v1` resource (DynamicDawg)|
| `libduallity.so`        | `duallity_*`    | Adapter: `vt.dictionary.v1` → `vt.scalar-wfst.1`        |
| `liblling_llang.so`     | `lling_*`       | Composer of two `vt.scalar-wfst.1` resources           |
| `libliblevenshtein.so`  | `llev_*`        | Independent consumer: `vt.dictionary.v1` → query cursor |

A `VtResource` is a two-word handle `{ void* context; const VtResourceVTable* }`
passed **by value** across every boundary. Because each resource carries its own
vtable, a consumer retains, releases, and negotiates interfaces on a producer's
resource without linking the producer — every callback dispatches back into the
library that created the resource, so allocation and freeing stay within the
owning library.

## What it asserts

```text
  libdictenstein DynamicDawg            (producer: vt.dictionary.v1)
        │  ldict_dictionary_resource    (borrowed — NOT released)
        ▼
  duallity_wfst_new  ── Levenshtein WFST ──▶  vt.scalar-wfst.1   (capture-once)
        │  duallity_wfst_resource        (owned retain — released)
        ▼  ∘ (lling_wfst_compose with a case-mapping WFST  term → UPPER(term))
  composed WFST  ── traverse ──▶  { UPPER(term) : lev(query, term) ≤ d }
```

- **DUAL-FAM-1** — the composed traversal (walked over the real
  `state_info`/`state_arcs` surface) equals the golden set derived from
  [`tests/fixtures/family_pipeline_golden.tsv`](../../../tests/fixtures/family_pipeline_golden.tsv)
  *and* equals a `llev_*` cursor query over the same dictionary resource.
- **DUAL-FAM-2** — ~5 dictionary mutations performed *after* capture do not
  change the captured language; a fresh cursor over the mutated dictionary does
  drift, proving the captures were genuinely isolated.
- **DUAL-FAM-3** — running the whole chain over an instrumented C
  `vt.dictionary.v1` provider (a counting dictionary, the C port of
  [`tests/support/counting_dictionary.rs`](../../../tests/support/counting_dictionary.rs))
  and tearing it down in **both orders** leaves the retain/release ledger
  balanced at zero (no leaked snapshot retain, no double free).
- **DUAL-CONFIG-C-1** — the same linked binary calls both the historical
  constructor and the revision-3 sized-record constructor. It checks defaults,
  effective option readback, caller-buffer independence, cache statistics and
  policy control, resource ownership after handle destruction, and rejection
  of unknown record versions and nonzero reserved fields.
- **DUAL-PHONETIC-C-1** — revision-4 constructors create an ambiguous native
  phonetic NFA, a dictionary-backed edit product, a custom rewrite graph, and
  a built-in German rewrite graph. The C walker verifies both spellings and
  snapshot survival after source release; malformed records and locales fail
  without transferring ownership.

## Resource-ownership contract exercised

| Call                        | Ownership of the `VtResource`                                    |
| --------------------------- | --------------------------------------------------------------- |
| `ldict_dictionary_resource` | **Borrow** — valid while the dictionary handle lives; do NOT release. A retaining consumer takes its own retain (duallity snapshots it; liblevenshtein retains it). |
| `duallity_wfst_resource`    | **Owned** — one retain; release with `duallity_resource_release`. |
| `duallity_phonetic_*_new`   | **Owned** — one direct `vt.scalar-wfst.1` retain; release with `duallity_resource_release`. |
| `lling_wfst_resource`       | **Owned** — one retain; release with `lling_resource_release`.    |
| `lling_wfst_compose`        | Lazily **retains** both inputs; freeing the composition releases them. |

## Build and run

```sh
# From the duallity checkout, with the sibling crates checked out next to it
# (llattice, libdictenstein, lling-llang, liblevenshtein-rust):
bindings/c/tests/build-and-run.sh
```

The script builds the three sibling cdylibs with `ffi` and duallity with
`c-bindings` (including the native phonetic family), then
compiles `family_pipeline.c` with `-std=c17 -Wall -Wextra -Werror`, wiring the
include paths (each crate's `include/` plus
`../vinary-tree-interop/include`), the library search paths,
and the runtime rpaths. `SKIP_BUILD=1` reuses already-built cdylibs; `CC` and
`PROFILE` (`release`|`debug`) are overridable. `LLING_DIR` selects a separate
lling-llang development checkout, and `CARGO_TARGET_DIR` places all four
builds and the C executable in one disk-backed build directory.

The `c-family-pipeline` job in
[`.github/workflows/ci.yml`](../../../.github/workflows/ci.yml) runs exactly this
script after checking out the dev siblings.
