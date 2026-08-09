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
| Fix | commits `df69019` (ffi constructor matrix + WFST semantics + snapshot-capture-once), `1e10ac5` (in-process family E2E), `b53aa2f` (paging-acceptance F3) |
| Status | FIXED |

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

## Finding DUAL-B4 — edge paging used a weaker acceptance predicate than the proven law (F3)

| Field | Value |
|---|---|
| Finding | DUAL-B4 (family finding F3, duallity side) |
| Date | 2026-08-09 |
| Component | `src/bindings.rs` foreign-dictionary edge paging |
| Class | correctness |
| Severity | medium |
| Fix | commit `b53aa2f` |
| Status | FIXED |

**Evidence.** duallity's `ResourceDictionary` edge-paging loop accepted a
provider page with a predicate weaker than the one liblevenshtein and
lling-llang converged on, mirroring family finding F3 (the three consumers each
carried a subtly different acceptance predicate for one interop paging law).

**Analysis.** The proven arbiter is `ConsumerAcceptance.accepts_dec`
(liblevenshtein-rust `docs/verification/abi/theories/ConsumerAcceptance.v`): a
page is honest iff `written <= capacity`, `written <= total - start`, no
overshoot (`offset + written <= total`), and progress unless exhausted.

**Fix.** Commit `b53aa2f` harmonized the duallity paging loop to that predicate;
`tests/ffi_paging_acceptance.rs` pins the rejection of each adversarial page
shape without aborting.

**Verification.** `cargo test --features ffi --test ffi_paging_acceptance` green;
the predicate is now identical across all three consumers.

## Finding DUAL-B5 — DUALLITY_STATUS_LIMIT_EXCEEDED is unreachable through the public ABI

| Field | Value |
|---|---|
| Finding | DUAL-B5 |
| Date | 2026-08-09 |
| Component | `src/ffi.rs`, `include/duallity.h` (`DuallityStatus`) |
| Class | status-surface consistency |
| Severity | low |
| Fix | ledger + docs (reserved by design) |
| Status | RECORDED |

**Evidence.** `DUALLITY_STATUS_LIMIT_EXCEEDED` (7) is defined in `src/ffi.rs`
and `include/duallity.h`, but no `BindingError` variant maps to it and no
`duallity_*` function returns it. A `maximum_distance` exceeding the `u8` bound
of the universal/generalized kinds returns `INVALID_ARGUMENT` (via
`BindingError::InvalidArgument`), not `LimitExceeded`.

**Analysis.** An out-of-`u8`-range distance is a caller argument error, and
`INVALID_ARGUMENT` is a defensible classification for it; `LimitExceeded` is
reserved for a future numeric-representation bound that the current surface does
not hit. This is a status-surface nicety, not a defect: no correct caller can
depend on receiving `LimitExceeded` today.

**Fix.** Recorded as reserved and documented as such in
`docs/architecture/06-resource-abi-and-bindings.md` §3 and
`docs/guides/07-language-bindings.md` §5. If a future kind introduces a genuine
representation-limit path, map it to `LimitExceeded` then.

**Verification.** `grep -n LIMIT_EXCEEDED src/ffi.rs` shows the definition with
no producing `map_error` arm; the docs mark it reserved.

## Finding DUAL-B6 — the root README's blanket "zero unsafe" claim is stale

| Field | Value |
|---|---|
| Finding | DUAL-B6 |
| Date | 2026-08-09 |
| Component | `README.md` (feature summary) |
| Class | documentation accuracy |
| Severity | low |
| Fix | commit noted in the verification row |
| Status | FIXED |

**Evidence.** `README.md` asserted a blanket "Zero `unsafe`, panic-free public
surface", but `src/ffi.rs` + `src/bindings.rs` contain contained `unsafe` at the
C-ABI boundary (foreign-pointer dereferences, resource retain/release) — 25
occurrences.

**Analysis.** The compute core is `unsafe`-free, but the C ABI boundary
necessarily uses contained, `catch_unwind`-guarded `unsafe`. The blanket claim
overstates the guarantee; the docs the W5 docs wave owns already scoped it
(`docs/engineering/safety-and-panics.md`, `docs/security/`).

**Fix.** README scoped to distinguish the `unsafe`-free compute core from the
contained C-ABI boundary `unsafe` (see the verification row's commit).

**Verification.** `grep -rn 'unsafe' src/ffi.rs src/bindings.rs | wc -l` = 25;
the README no longer claims a crate-wide absence of `unsafe`.
