#!/usr/bin/env bash
# Dynamic-analysis harness for duallity's resource-ABI boundary (wave W8).
#
# Runs duallity's own FFI/ABI boundary tests under AddressSanitizer (+
# LeakSanitizer) and ThreadSanitizer to catch, at the C ABI gate, the classes
# of defect that safe Rust and `catch_unwind` cannot: use-after-free,
# out-of-bounds access, leaks of leased/retained `VtResource` handles, and data
# races across the call gate. These exercise, dynamically at machine level, the
# same invariants the correspondence tests pin statically: the constructor
# round-trip (DUAL-ENC-1), snapshot capture-once isolation, the edge-paging
# acceptance predicate (family finding F3 / `accepts_dec`), and the retain /
# release discipline of the shared `VtResource` type (DUAL-B7).
#
# The multi-cdylib C `family_pipeline` is already covered by valgrind (see
# `bindings/c/tests/` and finding DUAL-B8); THIS leg covers duallity's Rust FFI
# surface directly, under the sanitizer runtimes rather than valgrind.
#
# Requires a nightly toolchain with the `rust-src` component (for
# `-Zbuild-std`, which rebuilds std with the sanitizer runtime). duallity is a
# path-dependency consumer of all three siblings (libdictenstein, liblevenshtein,
# lling-llang), so `-Zbuild-std` rebuilds std plus the whole sibling stack under
# instrumentation and is SLOW (tens of minutes); scope with `--test <name>` and
# `SANITIZER_ONLY=address` when iterating locally.
#
# Usage:
#   scripts/run-sanitizers.sh                 # asan+lsan then tsan, whole ffi suite
#   SANITIZER_ONLY=address scripts/run-sanitizers.sh --test ffi_constructor_matrix
#   SANITIZER_NIGHTLY=nightly-2026-04-21 scripts/run-sanitizers.sh
set -euo pipefail

TARGET="${SANITIZER_TARGET:-x86_64-unknown-linux-gnu}"
NIGHTLY="${SANITIZER_NIGHTLY:-nightly}"
# duallity's boundary tests are gated behind `ffi`; `default = []`, so this is
# both the necessary and the minimal feature set for the FFI surface.
FEATURES="${SANITIZER_FEATURES:-ffi}"
ONLY="${SANITIZER_ONLY:-address thread}"

run_one() {
  local san="$1"; shift
  echo "== ${san}sanitizer =="
  RUSTFLAGS="-Zsanitizer=${san}" \
  RUSTDOCFLAGS="-Zsanitizer=${san}" \
  ASAN_OPTIONS="detect_leaks=1:detect_stack_use_after_return=1" \
    cargo +"$NIGHTLY" test -Zbuild-std \
      --target "$TARGET" --no-default-features --features "$FEATURES" "$@"
}

for san in $ONLY; do
  run_one "$san" "$@"
done

echo "sanitizers: all requested runs completed cleanly"
