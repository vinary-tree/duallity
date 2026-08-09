#!/usr/bin/env bash
# duallity formal-verification driver: builds the Rocq proofs under resource
# caps (when a user systemd scope is available) and runs the proof-escape gate.
# Born integrated with the duallity ABI verification bootstrap (obligations
# #21-#25); TLC models and the invariant-registry checker are wired in as they
# land.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Run the Coq build under systemd resource caps when a user scope is actually
# available (local dev -- a heavy modular proof must not freeze the machine),
# and directly otherwise (CI runners have no user systemd session).
capped_make() {
  if command -v systemd-run >/dev/null 2>&1 \
     && systemd-run --user --scope -q true >/dev/null 2>&1; then
    systemd-run --user --scope -q \
      -p MemoryMax=8G -p CPUQuota=1800% -p TasksMax=200 \
      make "$@"
  else
    make "$@"
  fi
}

capped_make -C "$ROOT/proofs/coq" proof-check
capped_make -C "$ROOT/proofs/coq" -j1

# --- TLC model checking ---------------------------------------------------
tlc_cmd() {
  if command -v tlc >/dev/null 2>&1; then
    tlc "$@"
  elif [[ -n "${TLA2TOOLS_JAR:-}" ]]; then
    java -cp "$TLA2TOOLS_JAR" tlc2.TLC "$@"
  elif [[ -f "$HOME/.tla/tla2tools.jar" ]]; then
    java -cp "$HOME/.tla/tla2tools.jar" tlc2.TLC "$@"
  else
    echo "ERROR: TLC not found. Install tlc or set TLA2TOOLS_JAR=/path/to/tla2tools.jar." >&2
    return 127
  fi
}

run_tlc() {
  local name="$1" spec="$2" cfg="$3"
  local metadir="/tmp/duallity-tlc-${name}-$$"
  rm -rf "$metadir"
  tlc_cmd -metadir "$metadir" -config "$cfg" "$spec"
  rm -rf "$metadir"
}

run_tlc_expect_failure() {
  local name="$1" spec="$2" cfg="$3" expected="$4"
  local metadir="/tmp/duallity-tlc-${name}-$$"
  local output="/tmp/duallity-tlc-${name}-$$.out"
  rm -rf "$metadir"
  if tlc_cmd -metadir "$metadir" -config "$cfg" "$spec" >"$output" 2>&1; then
    echo "ERROR: expected TLC model '$name' to fail, but it passed." >&2
    rm -rf "$metadir" "$output"; return 1
  fi
  if ! grep -Fq "$expected" "$output"; then
    echo "ERROR: TLC model '$name' failed for an unexpected reason." >&2
    cat "$output" >&2; rm -rf "$metadir" "$output"; return 1
  fi
  cat "$output"; rm -rf "$metadir" "$output"
}

run_tlc snapshot-capture-once \
  "$ROOT/proofs/tla/SnapshotCaptureOnce.tla" \
  "$ROOT/proofs/tla/MC/SnapshotCaptureOnce.cfg"

# Mutant: a live-dictionary mutation also updates the captured revision (the
# WFST aliasing the live dict instead of its snapshot) -- violates isolation.
negative_cap_dir="/tmp/duallity-negative-cap-$$"
mkdir -p "$negative_cap_dir"
cp "$ROOT/proofs/tla/SnapshotCaptureOnce.tla" "$negative_cap_dir/SnapshotCaptureOnce.tla"
cp "$ROOT/proofs/tla/MC/SnapshotCaptureOnce.cfg" "$negative_cap_dir/SnapshotCaptureOnce.cfg"
perl -0pi -e 's/\/\\ UNCHANGED <<captured, constructed, sourceAlive>>/\/\\ captured'"'"' = liveRev + 1\n  \/\\ UNCHANGED <<constructed, sourceAlive>>/' \
  "$negative_cap_dir/SnapshotCaptureOnce.tla"
run_tlc_expect_failure snapshot-capture-once-mutant \
  "$negative_cap_dir/SnapshotCaptureOnce.tla" \
  "$negative_cap_dir/SnapshotCaptureOnce.cfg" \
  "Action property MutationIsolation is violated."
rm -rf "$negative_cap_dir"
