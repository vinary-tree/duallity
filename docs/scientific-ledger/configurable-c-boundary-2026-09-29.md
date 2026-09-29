# Configurable WFST revision-3 C-boundary qualification (2026-09-29)

## Question and scope

Can duallity expose configured WFST construction, effective-option inspection,
and cache control as an additive C API while keeping the eight original ABI-1
functions callable and retaining exactly one dictionary snapshot per successful
construction? This experiment concerns the *development-branch* C boundary;
it is not a package-publication claim or a claim that all foreign facades are
complete.

The source of truth is [`bindings/api.json`](../../bindings/api.json), with
wire declarations in [`include/duallity.h`](../../include/duallity.h) and
implementations in [`src/ffi.rs`](../../src/ffi.rs). API revision 3 adds six
symbols and six version-1 records; ABI version 1 and the original eight
function signatures do not change. The explicit default is parameterized
Levenshtein, standard edit algorithm, maximum distance 2, and `CACHE_ALL`.

## Hypotheses and failure criteria

1. The model, Rust exports, C header, and raw binding mirrors agree on symbol
   names, record fields, enum values, and revision numbers. A mismatch fails
   the binding checker or C compilation/linkage.
2. An old and new C consumer can call into the *same* cdylib in one process.
   A missing symbol, altered call signature, or inconsistent ownership fails
   compilation, linkage, or the executable's assertions.
3. Malformed configuration fails before dictionary capture and leaves the
   constructor output null; provider faults release any temporary retains.
   A captured snapshot on parser failure or a positive outstanding-retain
   ledger fails the Rust boundary tests.
4. Cache controls operate on the exported lling-llang provider cache, not a
   second duallity cache. Inspection reports the normalized current policy;
   independently retained resources outlive the inspection handle.

## Method

The instrumented Rust provider in
[`tests/support/counting_dictionary.rs`](../../tests/support/counting_dictionary.rs)
counts snapshot callbacks, lazy edge reads, retain/release operations, and
programmable provider faults. The new
[`tests/ffi_configured_revision3.rs`](../../tests/ffi_configured_revision3.rs)
calls the exported Rust-defined C entry points with real wire records. The
existing C
[`family_pipeline.c`](../../bindings/c/tests/family_pipeline.c) links the four
independent family cdylibs and now calls both historical and revision-3
duallity constructors in the same executable. Its new phase exercises default
construction, caller-buffer independence, readback, statistics, policy
mutation, retained-resource lifetime, and malformed-version rejection.

All heavy Rust commands run under a user scope with 4 GiB RSS ceiling and
one-CPU quota. The build output uses a disk-backed shared `CARGO_TARGET_DIR`
under the liblevenshtein-rust workspace, not `/tmp`. The C integration run
used the RC6 lling-llang integration worktree via `LLING_DIR`; duallity's
committed `Cargo.toml` retains its ordinary sibling path. The current primary
lling-llang worktree is on an unrelated version/branch and was not modified.

## Results

| Check | Observed result |
| --- | --- |
| Binding model checker | 74 passed, 0 failed; exactly 14 modeled/exported/declared C symbols, API revision 3, ABI version 1 |
| Four-cdylib C compile/link/run | 359 assertions passed with `-std=c17 -Wall -Wextra -Werror`; original and configured constructors both called |
| Focused configured Rust boundary tests | 3 passed; successful capture once, zero edge reads before lazy expansion, malformed records before capture, and balanced retains |
| Full Rust FFI regression | 372 passed, 0 failed across unit and integration targets after updating the constructor-matrix revision pin |
| Strict clippy | `cargo clippy --locked --features ffi --all-targets -- -D warnings` passed |
| C11 record layout | `cc -std=c11 -Wall -Wextra -Werror -fsyntax-only` passed for `config_record_layout.c` |
| Python raw ABI lint and existing facade smoke | `ruff check` passed for `_abi.py`; four existing `test_api.py` cases passed against the newly built family cdylibs |
| Raku generation check | model, C header, and generated NativeCall declarations agreed |

The old C phase also composes a duallity WFST through lling-llang and checks
its accepted language against both a golden set and an independent
liblevenshtein cursor. The new phase is additive, so its pass does not replace
the earlier semantic and ownership checks.

## Interpretation and limits

The observations support source and binary compatibility of the C interface
within the tested Linux/x86-64 build and behavior of its normal/error paths.
They do not prove memory safety for arbitrary invalid C addresses: the caller
must provide live, correctly aligned storage for every advertised extent and
synchronize destruction. The Rust parser tests cover operation arrays,
restrictions, forward tails, UTF-8, and hard limits at finer granularity than
the C smoke phase. Foreign-language *idiomatic* wrappers, package metadata,
cross-platform binaries, and release publication remain later tasks. No RC6
package is published by this experiment.
