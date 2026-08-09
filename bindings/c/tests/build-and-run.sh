#!/usr/bin/env bash
#
# Build the four family cdylibs (each with ONLY its own `ffi` feature, so the
# exported C ABIs are disjoint), then compile and run bindings/c/tests/
# family_pipeline.c against all four in one process.
#
# Layout assumption (the standard sibling checkout, mirrored by the
# `checkout-dev-siblings` CI action): the related crates live next to duallity
#
#     <parent>/duallity            (this repo)
#     <parent>/libdictenstein
#     <parent>/lling-llang
#     <parent>/liblevenshtein-rust (also provides vinary-tree-interop)
#     <parent>/llattice            (transitive dependency)
#
# Environment overrides:
#   CC        C compiler                       (default: cc)
#   PROFILE   cargo profile: release|debug     (default: release)
#   SKIP_BUILD=1  reuse already-built cdylibs  (default: unset)
#
# gxhash (via a persistent-artrie dependency of libdictenstein) requires AES-NI
# and SSE2 at compile time, so RUSTFLAGS pins those features (matching CI).

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
duallity_root="$(cd "${script_dir}/../../.." && pwd)"
parent="$(dirname "${duallity_root}")"

libdictenstein_dir="${parent}/libdictenstein"
lling_dir="${parent}/lling-llang"
liblevenshtein_dir="${parent}/liblevenshtein-rust"
interop_include="${liblevenshtein_dir}/vinary-tree-interop/include"

CC="${CC:-cc}"
PROFILE="${PROFILE:-release}"
cargo_profile_flag="--release"
if [ "${PROFILE}" = "debug" ]; then
  cargo_profile_flag=""
fi

for dir in "${libdictenstein_dir}" "${lling_dir}" "${liblevenshtein_dir}"; do
  if [ ! -d "${dir}" ]; then
    echo "error: expected sibling crate at ${dir} (see checkout-dev-siblings)" >&2
    exit 1
  fi
done

export RUSTFLAGS="${RUSTFLAGS:--C target-feature=+aes,+sse2}"

build_cdylib() {
  local name="$1" manifest="$2"
  echo "::group::cargo build ${name} (--no-default-features --features ffi)"
  cargo build ${cargo_profile_flag} --no-default-features --features ffi \
    --manifest-path "${manifest}/Cargo.toml"
  echo "::endgroup::"
}

if [ "${SKIP_BUILD:-}" != "1" ]; then
  build_cdylib libdictenstein "${libdictenstein_dir}"
  build_cdylib lling-llang "${lling_dir}"
  build_cdylib liblevenshtein "${liblevenshtein_dir}"
  build_cdylib duallity "${duallity_root}"
fi

libdictenstein_lib="${libdictenstein_dir}/target/${PROFILE}"
lling_lib="${lling_dir}/target/${PROFILE}"
liblevenshtein_lib="${liblevenshtein_dir}/target/${PROFILE}"
duallity_lib="${duallity_root}/target/${PROFILE}"

for so in \
  "${libdictenstein_lib}/liblibdictenstein.so" \
  "${lling_lib}/liblling_llang.so" \
  "${liblevenshtein_lib}/libliblevenshtein.so" \
  "${duallity_lib}/libduallity.so"; do
  if [ ! -f "${so}" ]; then
    echo "error: missing cdylib ${so}" >&2
    exit 1
  fi
done

binary="${duallity_root}/target/family_pipeline_c"
mkdir -p "${duallity_root}/target"

echo "::group::compile family_pipeline.c (-std=c17 -Wall -Wextra -Werror)"
"${CC}" -std=c17 -Wall -Wextra -Werror \
  -I "${interop_include}" \
  -I "${libdictenstein_dir}/include" \
  -I "${liblevenshtein_dir}/include" \
  -I "${lling_dir}/include" \
  -I "${duallity_root}/include" \
  "${script_dir}/family_pipeline.c" \
  -Wl,--no-as-needed \
  -L "${libdictenstein_lib}" \
  -L "${liblevenshtein_lib}" \
  -L "${lling_lib}" \
  -L "${duallity_lib}" \
  -llibdictenstein -lliblevenshtein -llling_llang -lduallity \
  -Wl,-rpath,"${libdictenstein_lib}" \
  -Wl,-rpath,"${liblevenshtein_lib}" \
  -Wl,-rpath,"${lling_lib}" \
  -Wl,-rpath,"${duallity_lib}" \
  -lpthread -ldl -lm \
  -o "${binary}"
echo "::endgroup::"

echo "::group::run ${binary}"
LD_LIBRARY_PATH="${libdictenstein_lib}:${lling_lib}:${liblevenshtein_lib}:${duallity_lib}:${LD_LIBRARY_PATH:-}" \
  "${binary}"
echo "::endgroup::"
