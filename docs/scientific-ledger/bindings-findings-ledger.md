# Bindings findings ledger

This ledger records defects, inconsistencies, and coverage gaps found while
scrutinizing duallity's language-binding stack (the `duallity_*` C ABI, the
C/C++ headers, and the npm facade package). Every entry follows the uniform
family schema

```text
Finding DUAL-B<N> | date | component | class | severity | evidence | analysis
                  | fix (commit or ledger-only) | verification | status
```

and states exactly what was observed, how it was measured, what (if anything)
changed, and how the claim can be re-checked or falsified. Entries are
append-only; a resolved entry keeps its evidence and gains a fix commit and a
verification record. The machine gate that guards the resolved entries is
`scripts/check-bindings.py` (39 checks; exit 1 on any failure).

## Finding DUAL-B1 — README MSRV badge contradicted Cargo.toml

| Field | Value |
|---|---|
| Finding | DUAL-B1 |
| Date | 2026-08-08 |
| Component | `README.md` (rustc badge, license-section prose) |
| Class | documentation drift |
| Severity | low |
| Fix | commit `c1ac823` |
| Status | resolved |

**Evidence.** At commit `7b3a693`, `README.md:8` rendered the shields.io badge
`rustc-1.70%2B` and `README.md:262` stated "Minimum supported Rust version:
**1.70**", while `Cargo.toml:7` declares `rust-version = "1.95"`. The MSRV was
raised to 1.95 by commit `71f9b32` (the `--all-features` floor set by sysinfo
0.39 across the sibling path dependencies); both README mentions were left
behind.

**Analysis.** A consumer trusting the badge would attempt a build on rustc
1.70–1.94 and fail at dependency resolution. The two README mentions and the
manifest were three unlinked statements of one fact; nothing failed when they
diverged.

**Fix.** Commit `c1ac823` updates both mentions to 1.95. Commit `d29a907` adds
the guard: `scripts/check-bindings.py` checks `MSRV-1-badge` and `MSRV-2-prose`
compare the badge and the prose against `Cargo.toml`'s `rust-version` on every
run.

**Verification.** With the fix applied, the gate passes 39/39. A negative test
on 2026-08-08 reverted the badge to `rustc-1.70%2B` in the working tree: the
gate reported `FAIL MSRV-1-badge` and exited 1, then passed again after the
badge was restored. Falsification: edit either mention away from
`rust-version` and run `python3 scripts/check-bindings.py`; exit 0 would
invalidate the guard.

## Finding DUAL-B2 — v0.3.0 is pinned by the family but not tagged or published

| Field | Value |
|---|---|
| Finding | DUAL-B2 |
| Date | 2026-08-08 |
| Component | release versioning (git tags, crates.io, family pins) |
| Class | version-pin inconsistency |
| Severity | medium |
| Fix | ledger-only (release action; user decision in the approved plan) |
| Status | open — awaiting the v0.3.0 release event |

**Evidence** (gathered 2026-08-08):

- `Cargo.toml` declares `version = "0.3.0"` (since commit `010fbad`,
  "consolidate WFST optimization campaign + release 0.3.0"), and
  `bindings/javascript/package.json` plus `deps.cljs` pin
  `@vinary-tree/duallity` 0.3.0.
- `git tag` lists only `v0.1.0` and `v0.2.0`; there is no `v0.3.0` tag.
- The crates.io API (`/api/v1/crates/duallity`) reports `max_version` 0.2.0
  with published versions 0.2.0 and 0.1.0.
- `liblevenshtein-rust/bindings/related-projects.json` pins duallity at
  `{"version": "0.3.0", "ref": "v0.3.0"}` — a tag that does not exist.
- `liblevenshtein-rust/scripts/check-bindings.py` requires the duallity npm
  package version to equal 0.3.0.
- duallity's `.github/workflows/release-bindings.yml:63` stages release
  artifacts under `dist/duallity-0.3.0-<target>`.

**Analysis.** Inside this repository the 0.3.0 statement is complete and
self-consistent (manifest, npm facade, ClojureScript pin, release workflow,
`bindings/api.json`). The inconsistency is external: the family pins name a
git ref and registry version that no release event has produced, so any
consumer resolving `ref v0.3.0` or `duallity = "0.3"` from crates.io fails
today. No in-repo edit can close this gap; only tagging `v0.3.0` at a suitable
commit and publishing can.

**Fix.** Ledger-only, per the approved plan: tagging and publishing are
release actions outside this documentation-and-testing campaign, and the
in-repo tree is already coherent at 0.3.0.

**Verification.** After the release event, re-run `git tag` (expect `v0.3.0`),
re-query the crates.io API (expect `max_version` 0.3.0), and re-run the
sibling `liblevenshtein-rust/scripts/check-bindings.py` (its duallity pin
checks then resolve against published artifacts). Until then this entry is the
canonical record that the pins are intentional and forward-dated.

## Finding DUAL-B3 — the 7-function C ABI has one test and the repo has two property blocks

| Field | Value |
|---|---|
| Finding | DUAL-B3 |
| Date | 2026-08-08 |
| Component | `src/ffi.rs`, `src/bindings.rs` test coverage |
| Class | test-coverage gap |
| Severity | medium |
| Fix | none here — assigned to wave W5 of the approved plan |
| Status | OPEN |

**Evidence** (measured 2026-08-08 at commit `d29a907`):

- `src/ffi.rs` exports 7 `pub extern "C" fn duallity_*` symbols but contains
  exactly 1 test, `c_constructor_retains_query_start_dictionary_revision`,
  which exercises only kind 0 (Levenshtein) with algorithm 0 (Standard) on the
  happy path plus the free/resource/release lifecycle.
- Repo-wide there are exactly 2 `proptest!` blocks — `src/fzf_scorer.rs:253`
  and `src/fzf_support.rs:532` — both about fzf scoring. None exercises the C
  boundary, the constructor matrix, or the status-code surface.
- `src/bindings.rs` has 2 in-file tests (revision-survival and cross-crate
  composition); both enter through `create_wfst`, not through the exported C
  symbols.

**Analysis.** The implemented-but-untested surface includes: the full
9-kind × 4-algorithm constructor matrix (`DuallityWfstKind` × 
`DuallityAlgorithm`, where the universal and generalized kinds additionally
cap `maximum_distance` to `u8`); every non-`OK` `DuallityStatus` path
(`INVALID_ARGUMENT` for out-of-range kind/algorithm values,
`INVALID_UTF8` for malformed query bytes, `NULL_POINTER` for null output or
query pointers, `INCOMPATIBLE_RESOURCE` / `PROVIDER_ERROR` for defective
foreign dictionaries, `PANIC` containment at the `catch_unwind` boundary,
`LIMIT_EXCEEDED`); and `duallity_last_error_message` thread-locality. The
static parity gate added in `d29a907` pins the *shape* of the ABI (symbols,
enums, constants) but cannot observe its *behavior*.

**Fix.** None in this wave. Wave W5 of the approved plan owns the closure:
the FFI constructor matrix test, status-totality tests over every
`DuallityStatus` variant, and semantics proptests driving arbitrary
queries/distances through the boundary against an in-process oracle.

**Verification (for the eventual fix).** W5 is complete when every
`DuallityStatus` variant is produced by at least one test through the C
symbols, every `DuallityWfstKind` value is constructed through
`duallity_wfst_new`, and a property block relates boundary results to direct
`create_wfst` results. Falsification of this entry's counts: `grep -c
'pub \(unsafe \)\?extern "C" fn duallity_' src/ffi.rs` (expect 7) and
`grep -rn 'proptest!' src tests benches examples` (expect exactly the two
listed sites at the recorded commit).
