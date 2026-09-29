# Handle-owned WFST configuration inspection — 29 September 2026

## Question and source boundary

Can a future revision-3 caller inspect effective WFST options through stable
borrowed pointers without retaining caller memory or making those pointers
outlive their duallity handle? This qualification covers the internal
`OwnedOptions` representation in the `codex/binding-integration` completion
tree containing this note. The public C API remains at revision 2 with its
eight existing symbols; no `options_get` function is advertised yet.

The [ABI contract](../architecture/07-versioned-configurable-wfst-abi.md)
defines the intended readback lifetime. The
[ownership diagram](../diagrams/duallity-config-ownership.svg) distinguishes
caller buffers, handle-owned inspection storage, and the separately retained
lling-llang WFST resource.

## Representation and method

The parser first validates and owns the native custom operation set. Before
that set moves into the WFST builder, `OwnedOptions::from_parsed` copies its
names and listed restriction text into private boxed byte arrays. It builds
boxed restriction and operation record arrays whose pointers target those
allocations. The optional generalized limits record is boxed separately.
Moving the opaque handle cannot move any pointee. Construction completes all
pointer wiring before sharing the immutable view across threads; the caller
must treat returned pointers as read-only and stop using them when the handle
is freed.

Readback copies the options record by value and reads policy/capacity from
the live exported provider cache. It therefore reports an effective policy
change without mutating the query, dictionary snapshot, operation catalog,
or limits. Listed restrictions are presented in the native substitution
set's canonical order: duplicate input pairs may coalesce because this is an
**effective semantic catalog**, not a byte-for-byte log of the caller's
original array. Operation order and names remain intact. Legacy handles
have no custom catalog or explicit limits. Their ignored algorithm selector
and FZF distance are normalized to the effective standard and zero values.

## Reproduction and observed results

The primary `../lling-llang` checkout was owned by another agent and reported
version `0.2.0`; this duallity branch requires `4.0.0-rc.6` and its shared
cache API. For the test run only, the manifest used the relative
`../lling-llang-vco-integration` worktree. The ordinary relative path was
restored before committing. No primary lling-llang edits were made.

Tests ran under a user systemd scope with `MemoryMax=4G`, `MemorySwapMax=0`,
`CPUQuota=100%`, `TasksMax=64`, and `IOWeight=30`; the target directory was
disk-backed under the liblevenshtein-rust workspace, not `/tmp`.

```sh
cargo test --locked --features ffi --lib --quiet
cargo clippy --locked --features ffi --lib --tests -- -D warnings
python3 scripts/check-bindings.py
python3 scripts/check-binding-docs.py
git diff --check
```

The FFI-enabled library suite passed **188/188** tests; the four new
inspection tests passed. Strict Clippy, the binding model's **74/74** checks,
and the binding-documentation checker for **six** facades passed. The custom
catalog test changed and dropped caller-owned name, restriction, and limits
buffers before dereferencing the handle's inspection pointers. It then
changed the resource's cache policy and observed only the policy/capacity
fields change. An ASCII-pair regression exercised the native byte fast path
and confirmed that duplicate listed pairs coalesce in semantic readback.
After dropping the handle, a separately retained WFST
resource still supported cache control. Legacy tests confirmed effective
readback for ignored selectors and an empty custom catalog. Repeated
null-output construction followed by a successful construction exercised
the early-drop path for temporary inspection storage.

## Limits of the evidence

The early-drop test exercises the control path but is not itself a leak
detector. A sanitizer-backed C consumer and output-size/failure-injection
tests remain part of the public C-boundary qualification. The future
`options_get` write rules, exported symbol/version negotiation, and every
foreign-language mirror remain unqualified. No RC6 package was published
or pushed by this internal step.
