# Configurable WFST constructor-core qualification — 29 September 2026

## Question and boundary

Can duallity validate sized revision-3 option records, own custom grammar text,
and construct the corresponding native WFST without changing the shipped C API
revision or the behavior of existing constructors? The tested source is the
`codex/binding-integration` completion tree containing this note. This is an
**internal Rust constructor-core** result, not evidence that a revision-3 C
function or foreign-language mirror is available.

The [ABI contract](../architecture/07-versioned-configurable-wfst-abi.md)
defines record fields and ownership. The parser is in `src/ffi/config.rs`;
`src/bindings.rs` routes old and configured native construction through one
builder so default operation presets and exact custom limits have one
implementation. The public `duallity_api_revision()` remains `2` and the
shipped symbol set remains eight functions.

## Method

The parser and builder follow this order:

1. Read the 16-byte options header, reject unsupported size/version/reserved
   bytes, and interpret kind, algorithm, distance, and cache selector. This
   prevents a newer caller's nonzero unknown fields from silently changing
   meaning in an older library.
2. Validate optional limits and each counted array's nullability, stride,
   alignment, count, and checked byte extent. An element's own declared size
   must fit its stride before any unknown tail is read.
3. Bound and validate UTF-8 names and listed source/target strings, then copy
   them into owned native operation structures. Validate the complete
   operation grammar before any dictionary callback is reachable.
4. Capture one immutable dictionary revision and pass either the validated
   custom catalog or the named preset, plus optional exact limits, into the
   same generalized builder used by the legacy constructor.

This ordering matters: an invalid foreign record cannot cause a provider
snapshot, and caller-owned byte buffers may change after parsing without
changing the resulting grammar. An arbitrary non-null C pointer still has to
designate mapped, stable storage for its advertised extent; no library can
establish that from its numerical address alone.

## Reproduction and observations

The primary `../lling-llang` checkout was at version `0.2.0`, while this
duallity branch requires `4.0.0-rc.6`. For the test run only, its relative
path dependency was pointed at the clean
`../lling-llang-vco-integration` worktree, which reports `4.0.0-rc.6`.
The manifest path was restored before the source commit; this is a
test-graph prerequisite, not a committed dependency relocation. No primary
lling-llang worktree edits were made.

The following commands were run from this duallity worktree under a user
systemd scope with `MemoryMax=4G`, `MemorySwapMax=0`, `CPUQuota=100%`,
`TasksMax=64`, and `IOWeight=30`. `CARGO_TARGET_DIR` was a disk-backed target
under the liblevenshtein-rust workspace, never `/tmp`.

```sh
cargo test --locked --features ffi --lib --quiet
cargo clippy --locked --features ffi --lib --tests -- -D warnings
python3 scripts/check-bindings.py
python3 scripts/check-binding-docs.py
git diff --check
```

The final Rust run passed **178/178** library tests with no failures. Strict
Clippy passed. The binding checker passed **74/74** model/header/symbol checks
and the documentation checker passed for **six** adapter facades. The focused
tests cover null pointers, invalid UTF-8, unsupported selectors and option
combinations, reserved/version/tail fields, count and stride bounds, grammar
and query-limit failures, and deep ownership of both operation names and
listed restriction strings. A wire-defined standard operation catalog
produced the same complete lazy state/arc graph as the legacy generalized
constructor for a three-term dictionary after the source dictionary and its
resource handle were dropped. This exercises dictionary snapshot retention
and semantic parity, not just constructor success.

## Limits of the evidence

The internal parser is not yet called by any exported revision-3 C symbol.
The record-layout compile gate proves the C declarations compile, but these
Rust tests do not qualify foreign callers, host-language idioms, cache
policy controls, inspection buffers, old/new C binaries, or adversarial
concurrency. Those are separate tasks before the API revision can be raised.
No RC6 package was published by this qualification.
