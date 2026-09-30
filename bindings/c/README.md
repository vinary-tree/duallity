# duallity C binding

The C17/C23 facade adapts an immutable Unicode-scalar `vt.dictionary.v1`
snapshot into a lazy edit/phonetic weighted finite-state transducer (WFST),
or constructs a standalone phonetic pattern/rewrite graph. Every constructor
exports an owned `vt.scalar-wfst.1` resource. The original nine edit/phonetic
selectors and the revision-4 native phonetic family are declared in
[`duallity.h`](../../include/duallity.h); the normative semantics are in the
[resource ABI reference](../../docs/architecture/06-resource-abi-and-bindings.md).

## Installation and loading

Install the staged native CMake/pkg-config package, or build the FFI library in
the repository's sibling-family layout. The executable evidence runs the full
four-library path:

```sh
bindings/c/tests/build-and-run.sh
```

At dynamic-load time, require exact `duallity_abi_version()` equality and an
`duallity_api_revision()` at least as new as the header. `duallity.h` includes
`vinary_tree_interop.h`; override `VT_INTEROP_HEADER` only when the build system
deliberately relocates that canonical family header.

## Executable conformance evidence

[`family_pipeline.c`](tests/family_pipeline.c) is compiled and executed by CI
against four separately built shared libraries. It constructs a Dynamic DAWG
in libdictenstein, captures it in duallity, composes the lazy WFST with an
lling-llang case map, compares its complete result set with liblevenshtein, then
repeats teardown in both ownership orders. The fixture verifies exact counts,
distances, terms, post-capture mutation isolation, and a zero retain ledger.

![Dictionary producer, duallity adapter, scalar-WFST resource, foreign consumer, and trust boundaries.](../../docs/diagrams/duallity-resource-abi-dataflow.svg)

## API, selectors, and domains

| Element | Contract |
|---|---|
| `duallity_wfst_new` | Borrows a dictionary resource for the call, captures one immutable snapshot, validates the query/selectors, and returns an owned WFST handle. |
| `duallity_wfst_new_ref` | Provides identical semantics through a pointer to the borrowed dictionary aggregate for foreign-function interfaces that cannot pass `VtResource` by value. |
| `DuallityAlgorithm` | Standard, optimal-string-alignment transposition, merge-and-split, or unrestricted Damerau-Levenshtein; consumed only by the Levenshtein kind. |
| `DuallityWfstKind` | Levenshtein; three universal; four generalized including phonetic; or FZF. Each advertises its exact weight domain. |
| `duallity_wfst_resource` | Returns one independently retained `vt.scalar-wfst.1` resource. |
| `duallity_wfst_free` | Frees the project handle; previously exported resources remain valid. |
| `duallity_resource_release` | Releases exactly one resource retain. |
| `duallity_phonetic_nfa_new` | Compiles a Unicode phonetic pattern into a standalone lazy WFST; optional alphabet controls expansion of wide labels. |
| `duallity_phonetic_product_new_ref` | Captures one dictionary revision and constructs the lazy phonetic-NFA × bounded edit × dictionary product. |
| `duallity_phonetic_rewrite_new` | Copies versioned, priority-ordered Unicode rewrite rules into a standalone graph. |
| `duallity_phonetic_rewrite_builtin_new` | Constructs the native English, German, or French rewrite rule set. |

The dictionary must advertise Unicode-scalar units. Query input is
pointer-plus-byte-length UTF-8, so it may be non-NUL-terminated but must be
valid. Maximum-distance limits are kind-specific; universal/generalized state
encodings reject values beyond their represented range instead of truncating.

The four phonetic constructors require a library built with `phonetic-rules`
and `duallity_api_revision() >= 4`. Pattern, alphabet, and rule text are
borrowed UTF-8 byte spans during the call. Null alphabet plus zero length
selects the native printable-ASCII default; an explicitly non-null, empty
alphabet means an empty expansion alphabet. Literal symbols still match
exactly. The product's `maximum_distance` is an unweighted edit limit in
`0..=255`; nonnegative finite `phonetic_weight`, `edit_weight`, and rule costs
govern tropical ranking, not that limit. Built-in locales use `0` for English,
`1` for German, and `2` for French. Neither the C boundary nor the native rule
sets implicitly case-fold or Unicode-normalize input.

All four functions write one independently owned `VtResource` on success.
Release it once with `duallity_resource_release`; no `DuallityWfst*` handle is
involved. The product borrows the source dictionary only during construction
and retains its captured revision, so later source mutation or release cannot
change the graph. Failed constructors set a valid output slot to null and
transfer no retain. A caller that passes rewrite records must populate
`DuallityPhoneticRuleV1.header` with `struct_size = sizeof(DuallityPhoneticRuleV1)`,
`record_version = 1`, and zero reserved fields; the strings are copied before
return. A finite LRU cache uses policy `2` with an explicit positive capacity,
or the library default when capacity is zero; policies `0` and `1` select
all-state and no-state caching respectively.

Traversal is performed through the standard `vt.scalar-wfst.1` interface, so
callers can compose a rewrite graph with a phonetic product in lling-llang or
use a custom consumer. Bound traversal by states, output length, cost, and
result count when the pattern or rewrite graph admits cycles; a finite bound
may omit a globally better candidate that was not yet visited. The
[Julia facade](../julia/Duallity/README.md) gives a concrete bounded-search
example over the same native resources.

## Ownership and capture-once semantics

The dictionary argument is borrowed only until `duallity_wfst_new` returns.
Successful construction owns a snapshot retain; later dictionary mutation,
compaction, close, or checkpoint cannot alter the WFST's revision. A project
WFST and each exported resource are independent owners, so either can be
released first. Failed construction transfers no ownership.

Capture and resource handoff are $`\mathcal{O}(1)`$; reachable WFST product
states expand lazily. The complete double-adapter sequence appears in
[`wfst-new-capture-compose-sequence.svg`](../../docs/diagrams/wfst-new-capture-compose-sequence.svg).

## Errors and failure containment

Every fallible call returns `DuallityStatus`. Branch on the enum, then copy
`duallity_last_error_message()` before another native call on that thread.
Invalid arguments/UTF-8, null pointers, contained panics, incompatible
resources, provider faults, and representation limits are distinct. No Rust
panic or foreign-provider fault is permitted to unwind across the C ABI.

## Concurrency and reentrancy

Distinct handles and immutable exported resources are reentrant. Do not race
free/release with an operation on the same owner. A foreign provider is
serialized by default unless it explicitly opts into parallel reentrancy;
duallity's internal lazy registries publish shared states without a
resource-wide traversal lock.

## Performance and marshalling

Pass retained resources rather than serializing dictionaries or WFST graphs.
State expansion is batched per state, labels remain `uint64_t` wire values,
and the adapter retains the dictionary snapshot rather than copying terms.
Measure end-to-end composition/search separately from constructor capture: a
cheap constructor intentionally defers product work to traversal.

## Security and provider trust

Treat the source vtable, counts, node IDs, labels, values, page offsets, query
length, and selectors as untrusted. The boundary validates interface identity,
version, domains, reserved fields, bounds, provider statuses, and resource
budgets before publication. See the
[threat model](../../docs/security/threat-model.md) and its
[foreign-provider boundary diagram](../../docs/diagrams/foreign-provider-trust-boundary.svg).

## Compatibility and troubleshooting

Project ABI/API versions, the family ABI/interface versions, package versions,
and persistent producer formats are independent. `INCOMPATIBLE_RESOURCE`
usually means a missing/wrong-version dictionary interface or a non-Unicode
domain. Loader failures usually mean the OS/CPU artifact, interop header, or
runtime search path is mismatched. Record exact selectors and the copied native
diagnostic before reducing a failure.

## Maintainer workflow

1. Update [`bindings/api.json`](../api.json) before changing selectors, symbols, or pins.
2. Extend the ABI architecture reference, threat model, and this facade guide.
3. Add positive, negative, fault-injection, mutation-isolation, and retain-ledger cases.
4. Run both binding gates and the four-cdylib family pipeline.
5. Stage native packages and validate the C and C++ consumers from installed metadata.
