# Configurable WFST cross-language qualification (2026-10-07)

## Question and boundary

Do the revision-3 configured weighted finite-state transducer (WFST)
constructor, option readback, and cache controls remain usable through the
RC.6 C, C++, Python, Julia, Raku, JavaScript, TypeScript, and ClojureScript
surfaces? The [versioned contract](../architecture/07-versioned-configurable-wfst-abi.md)
defines the records and ownership rules; the [C-boundary experiment](configurable-c-boundary-2026-09-29.md)
records the original revision-3 compatibility evidence. This ledger qualifies
the current source candidate. It does not record a package publication.

The complete native build reports ABI version 1 and API revision 6. Revision 3
introduced configuration; revisions 4 through 6 added other functions. The
minimal FFI build reports revision 3 and a phonetic-only build reports revision
4. Callers negotiate the returned revision before using an additive function.

## Method and observations

The duallity checkout started at `8b1ca8a5718d44706e1be0233c290d91bfaf7446`.
The local sibling checkouts used for the native family run were interop
`a54144a52553b79efacae15de1dfce9486b1e854`, libdictenstein
`01b81049f22ab0382d1bd3451daaa835ce454736`, lling-llang
`55e67fdb14273f92ba3e815155ee713069c25ac4`, and liblevenshtein
`15dae2fff8f533f8a8f6b920e5e8f798df9d2436`. Each shared library was
built independently with its own `ffi` feature, then linked into the C family
consumer. The local lling-llang and liblevenshtein checkouts were separate
development branches; the pull-request CI must repeat this gate against its
fresh coordinated sibling checkouts before this result is used for release.

| Surface | Executed gate and observation |
| --- | --- |
| Source, model, header, mirrors | `generate-config-abi.py --check`, `generate-raku-abi.py --check`, `check-bindings.py`, and `check-binding-docs.py` passed. The C11 record-layout compile passed with `-Wall -Wextra -Werror`. |
| Native Rust | `cargo test --all-features --no-fail-fast` passed 472 tests across 23 targets. These include malformed records, provider failures, retain balance, cache controls, and legacy entry points. |
| C and C++ | The four-library C family pipeline passed 480 assertions, calling original and configured constructors in one process. The C++17 configured consumer compiled with warnings as errors and passed. It checks copied option readback, cache policy changes, retained-resource lifetime, and invalid configuration. |
| Python | Seven source-facade tests and the executable family example passed against the native libraries. Four fresh local wheels were installed in a new virtual environment; the example passed there using bundled libraries. |
| Julia | Five local packages were developed into an isolated temporary project. `Pkg.test("Duallity")` passed, including revision-3 ownership/cache cases; the separate installed-family consumer passed. |
| Raku | A fresh `raku` process with the four source distributions on `RAKULIB` passed all 43 conformance tests, including malformed headers, snapshot survival, copied options, and cache controls. |
| JavaScript, TypeScript, ClojureScript | Locally packed RC.6 runtime, interop, and duallity archives were installed offline into a temporary consumer. Native ESM, CommonJS, browser WebAssembly, Node WASI, TypeScript type checking, and compiled ClojureScript probes all passed configured-WFST and cache cases. The duallity facade tests and `npm pack --dry-run` also passed. |

The JavaScript installed-consumer harness was copied from the sibling
`javascript-runtime` checkout at
`1792601c4c6dc781c025ce1a3fe33b6bbb5b42c3` into temporary storage;
its probes test both direct and package entry points without modifying that
repository. The local Raku executable named `zef` is the unrelated Meataxe
tool, so a Raku package-manager installation was unavailable. The source
distribution consumer is the Raku evidence here. The Python and JavaScript
archives were built and installed locally; no registry artifact was used.

## Interpretation and remaining release gate

The observed old/new C calls support additive ABI compatibility on this
Linux/x86-64 host. The negative paths and retain counters exercise failure
cleanup; readback after construction and cache mutation exercise the shared
provider cache rather than a second language-local cache. The language probes
also show that a dictionary snapshot survives its source handle and that
copied configuration survives the graph handle.

The pull-request matrix remains the authoritative fresh-checkout gate for
Linux C/C++, Python 3.10 and 3.14, Julia 1.10 and 1.12, Raku, sanitizers,
and package checks. The JavaScript runtime's integrated CI exercises its
installed-family consumer on the final coordinated sources. Neither the local
evidence nor this ledger is a substitute for those green hosted results or
for the separate RC.6 publication decision.
