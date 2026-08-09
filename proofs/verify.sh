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
