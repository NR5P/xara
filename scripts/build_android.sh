#!/bin/sh
# Builds libxara_ffi.so for Android with the NDK and copies it into the app's jniLibs.
#   scripts/build_android.sh [ABI...]   (default: arm64-v8a x86_64)
# The NDK is $ANDROID_NDK_HOME, else the newest one in the Android SDK.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
export PATH="$HOME/.cargo/bin:$PATH"
api=29
sdk=${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}}
ndk=${ANDROID_NDK_HOME:-$(ls -d "$sdk"/ndk/* 2>/dev/null | sort -V | tail -1)}
[ -d "$ndk" ] || { echo "no Android NDK found; install one with the SDK manager" >&2; exit 1; }
case $(uname -s) in
Darwin) host=darwin-x86_64 ;;
*) host=linux-x86_64 ;;
esac
bin="$ndk/toolchains/llvm/prebuilt/$host/bin"
[ $# -gt 0 ] || set -- arm64-v8a x86_64
for abi in "$@"; do
	case $abi in
	arm64-v8a) target=aarch64-linux-android clang=aarch64-linux-android ;;
	x86_64) target=x86_64-linux-android clang=x86_64-linux-android ;;
	armeabi-v7a) target=armv7-linux-androideabi clang=armv7a-linux-androideabi ;;
	*) echo "unknown ABI $abi" >&2; exit 2 ;;
	esac
	var=$(echo "$target" | tr a-z- A-Z_)
	export "CARGO_TARGET_${var}_LINKER=$bin/$clang$api-clang"
	cargo build --manifest-path "$root/Cargo.toml" --locked --release -p xara-ffi --target "$target"
	out="$root/android/app/src/main/jniLibs/$abi"
	mkdir -p "$out"
	cp "$root/target/$target/release/libxara_ffi.so" "$out/"
done
