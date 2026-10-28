#!/bin/sh
# Builds the xara-ffi static library for iOS devices and the simulator and
# bundles it with its header as ios/build/XaraFFI.xcframework.
#   scripts/build_ios.sh [debug|release]   (default release)
#   scripts/build_ios.sh --target TRIPLE   one release library, for Xcode's build phase
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
export PATH="$HOME/.cargo/bin:$PATH"
export IPHONEOS_DEPLOYMENT_TARGET=${IPHONEOS_DEPLOYMENT_TARGET:-17.0}
if [ "${1:-}" = --target ]; then
	exec cargo build --manifest-path "$root/Cargo.toml" --locked --release -p xara-ffi --features registered --target "$2"
fi
profile=${1:-release}
flag=
[ "$profile" = release ] && flag=--release
out="$root/ios/build"
for t in aarch64-apple-ios aarch64-apple-ios-sim; do
	cargo build --manifest-path "$root/Cargo.toml" --locked $flag -p xara-ffi --features registered --target "$t"
done
lib() { echo "$root/target/$1/$profile/libxara_ffi.a"; }
# Skip repackaging when neither library changed, so Xcode builds stay quick.
stamp="$out/XaraFFI.xcframework/.stamp"
want=$(stat -f %m "$(lib aarch64-apple-ios)" "$(lib aarch64-apple-ios-sim)" | tr '\n' ' ')
if [ -f "$stamp" ] && [ "$(cat "$stamp")" = "$want" ]; then
	exit 0
fi
rm -rf "$out/XaraFFI.xcframework"
mkdir -p "$out"
xcodebuild -create-xcframework \
	-library "$(lib aarch64-apple-ios)" -headers "$root/crates/xara-ffi/include" \
	-library "$(lib aarch64-apple-ios-sim)" -headers "$root/crates/xara-ffi/include" \
	-output "$out/XaraFFI.xcframework" >/dev/null
echo "$want" >"$stamp"
echo "built $out/XaraFFI.xcframework ($profile)"
