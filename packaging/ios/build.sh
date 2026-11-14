#!/bin/sh
# iOS build of Inkdown (the pure-Rust app in native/). Run on a Mac with Xcode installed.
#
#   sh packaging/ios/build.sh sim       # build for the simulator, install and launch it
#   sh packaging/ios/build.sh device    # build Inkdown.app for a device (then sign it, see README)
#
# iOS has no OpenGL, so this build uses the wgpu (Metal) renderer instead of glow.
set -eu
MODE="${1:-sim}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT/native"

if [ "$MODE" = "sim" ]; then
    case "$(uname -m)" in
        arm64) TARGET=aarch64-apple-ios-sim ;;
        *) TARGET=x86_64-apple-ios ;;
    esac
else
    TARGET=aarch64-apple-ios
fi

rustup target add "$TARGET"
cargo build --release --bin inkdown --target "$TARGET" --no-default-features --features wgpu

APP="$ROOT/dist/ios/$MODE/Inkdown.app"
rm -rf "$APP"
mkdir -p "$APP"
cp "target/$TARGET/release/inkdown" "$APP/inkdown"
cp "$ROOT/packaging/ios/Info.plist" "$APP/Info.plist"
# a single 1024px icon is enough for development builds
sips -z 180 180 "$ROOT/assets/icon.png" --out "$APP/AppIcon60x60@3x.png" > /dev/null
echo "Built $APP"

if [ "$MODE" = "sim" ]; then
    xcrun simctl boot "iPhone 15" 2> /dev/null || true
    open -a Simulator
    xcrun simctl install booted "$APP"
    xcrun simctl launch booted app.inkdown.editor
else
    echo "Sign with: codesign -s \"Apple Development: YOUR NAME\" --entitlements packaging/ios/entitlements.plist \"$APP\""
    echo "and embed your provisioning profile as \"$APP/embedded.mobileprovision\" before installing."
fi
