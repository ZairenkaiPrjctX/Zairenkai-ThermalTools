#!/usr/bin/env bash
# Cross-compile zthermal (the CLI) for Android and write checksummed binaries to dist/.
#
# Requires: Rust with the Android targets below, and ANDROID_NDK_HOME pointing at an Android
# NDK (CI uses r27d, checksum-verified). Usage: scripts/build-android.sh [TARGET...]
# Output: dist/zthermal-<version>-<abi> and dist/SHA256SUMS. Nothing is installed on a device.
set -euo pipefail

cd "$(dirname "$0")/.."
: "${ANDROID_NDK_HOME:?set ANDROID_NDK_HOME to an Android NDK directory}"
API=29
BIN="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"
[ -d "$BIN" ] || { echo "NDK toolchain not found: $BIN" >&2; exit 1; }

if [ "$#" -eq 0 ]; then
  set -- aarch64-linux-android x86_64-linux-android armv7-linux-androideabi
fi

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
mkdir -p dist
for target in "$@"; do
  case "$target" in
    aarch64-linux-android) abi=arm64-v8a; clang="aarch64-linux-android${API}-clang" ;;
    x86_64-linux-android) abi=x86_64; clang="x86_64-linux-android${API}-clang" ;;
    armv7-linux-androideabi) abi=armeabi-v7a; clang="armv7a-linux-androideabi${API}-clang" ;;
    *) echo "unsupported target: $target" >&2; exit 2 ;;
  esac
  [ -x "$BIN/$clang" ] || { echo "linker not found: $BIN/$clang" >&2; exit 1; }
  var="CARGO_TARGET_$(echo "$target" | tr 'a-z-' 'A-Z_')_LINKER"
  env "$var=$BIN/$clang" cargo build --locked --release --target "$target" -p zthermal
  cp "target/$target/release/zthermal" "dist/zthermal-$VERSION-$abi"
  if [ "$abi" != armeabi-v7a ]; then # 64-bit targets may run on 16 KB page-size devices
    for align in $("$BIN/llvm-readelf" -lW "dist/zthermal-$VERSION-$abi" | awk '$1 == "LOAD" { print $NF }'); do
      [ $((align)) -ge 16384 ] || { echo "dist/zthermal-$VERSION-$abi: LOAD segment aligned to $align, need 16 KB" >&2; exit 1; }
    done
  fi
done
(cd dist && sha256sum zthermal-"$VERSION"-* | tee SHA256SUMS)
