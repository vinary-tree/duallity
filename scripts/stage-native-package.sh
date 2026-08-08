#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 3 ]; then
  echo "usage: $0 <rust-target> <cargo-release-directory> <output-directory>" >&2
  exit 2
fi

target=$1
release_dir=$2
output_dir=$3
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)
package_name="duallity-${version}-${target}"
prefix="${output_dir}/${package_name}"

mkdir -p "${prefix}/bin" "${prefix}/include" \
  "${prefix}/lib/cmake/duallity" \
  "${prefix}/lib/cmake/vinary-tree-interop" "${prefix}/lib/pkgconfig"
cp include/duallity.h include/duallity.hpp "${prefix}/include/"
cp ../liblevenshtein-rust/vinary-tree-interop/include/vinary_tree_interop.h "${prefix}/include/"
cp cmake/duallityConfig.cmake cmake/duallityConfigVersion.cmake \
  "${prefix}/lib/cmake/duallity/"
cp ../liblevenshtein-rust/cmake/vinary-tree-interopConfig.cmake \
  ../liblevenshtein-rust/cmake/vinary-tree-interopConfigVersion.cmake \
  "${prefix}/lib/cmake/vinary-tree-interop/"
cp pkgconfig/duallity.pc ../liblevenshtein-rust/pkgconfig/vinary-tree-interop.pc \
  "${prefix}/lib/pkgconfig/"
cp LICENSE README.md "${prefix}/"

case "$target" in
  *-pc-windows-msvc)
    shared=$(find "$release_dir" -maxdepth 2 -type f -name 'duallity.dll' -print -quit)
    import_library=$(find "$release_dir" -maxdepth 2 -type f -name 'duallity.dll.lib' -print -quit)
    static_library=$(find "$release_dir" -maxdepth 2 -type f -name 'duallity.lib' -print -quit)
    test -n "$shared" && test -n "$import_library" && test -n "$static_library"
    cp "$shared" "${prefix}/bin/duallity.dll"
    cp "$import_library" "${prefix}/lib/duallity.dll.lib"
    cp "$static_library" "${prefix}/lib/duallity.lib"
    private_libs='-lbcrypt -luserenv -lws2_32 -lntdll -lsynchronization -ladvapi32'
    ;;
  *-apple-darwin)
    shared=$(find "$release_dir" -maxdepth 2 -type f -name 'libduallity.dylib' -print -quit)
    static_library=$(find "$release_dir" -maxdepth 2 -type f -name 'libduallity.a' -print -quit)
    test -n "$shared" && test -n "$static_library"
    cp "$shared" "${prefix}/lib/libduallity.dylib"
    cp "$static_library" "${prefix}/lib/libduallity.a"
    private_libs='-ldl -lpthread -lm -liconv -framework CoreFoundation -framework Security'
    ;;
  *-linux-gnu)
    shared=$(find "$release_dir" -maxdepth 2 -type f -name 'libduallity.so' -print -quit)
    static_library=$(find "$release_dir" -maxdepth 2 -type f -name 'libduallity.a' -print -quit)
    test -n "$shared" && test -n "$static_library"
    cp "$shared" "${prefix}/lib/libduallity.so"
    cp "$static_library" "${prefix}/lib/libduallity.a"
    private_libs='-ldl -lpthread -lm'
    ;;
  *)
    echo "unsupported release target: $target" >&2
    exit 1
    ;;
esac

sed -i.bak "s|^Libs.private:.*|Libs.private: ${private_libs}|" \
  "${prefix}/lib/pkgconfig/duallity.pc"
rm -f "${prefix}/lib/pkgconfig/duallity.pc.bak"
tar -czf "${output_dir}/${package_name}.tar.gz" -C "$output_dir" "$package_name"
printf '%s\n' "$prefix"
