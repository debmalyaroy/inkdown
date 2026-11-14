#!/bin/sh
# Builds Inkdown.app and Inkdown.dmg on a Mac.
#
#   sh packaging/macos/build.sh              # universal app (Apple silicon + Intel)
#   ARCH=native sh packaging/macos/build.sh  # only this Mac's architecture (faster)
#
# Output: dist/macos/Inkdown.app and dist/macos/Inkdown-<version>.dmg
# Open the .dmg and drag Inkdown to Applications to install.
#
# Needs: Xcode command line tools (xcode-select --install) and Rust (https://rustup.rs).
# The app is ad-hoc signed. To sign for distribution set
#   SIGN_IDENTITY="Developer ID Application: Your Name (TEAMID)"
# and notarise the .dmg afterwards (see packaging/macos/README.md).
set -eu

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
cd "$ROOT"
[ "$(uname -s)" = "Darwin" ] || { echo "This script must run on macOS." >&2; exit 1; }

VERSION=$(sed -n 's/^version *= *"\(.*\)"/\1/p' native/Cargo.toml | head -1)
ARCH=${ARCH:-universal}
SIGN_IDENTITY=${SIGN_IDENTITY:--}
OUT="$ROOT/dist/macos"
APP="$OUT/Inkdown.app"
DMG="$OUT/Inkdown-$VERSION.dmg"
export MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-11.0}

echo "==> Building Inkdown $VERSION ($ARCH)"
cd native
if [ "$ARCH" = "universal" ]; then
    rustup target add aarch64-apple-darwin x86_64-apple-darwin
    cargo build --release --locked --target aarch64-apple-darwin
    cargo build --release --locked --target x86_64-apple-darwin
    mkdir -p target/universal
    lipo -create -output target/universal/inkdown \
        target/aarch64-apple-darwin/release/inkdown \
        target/x86_64-apple-darwin/release/inkdown
    BIN="$ROOT/native/target/universal/inkdown"
else
    cargo build --release --locked
    BIN="$ROOT/native/target/release/inkdown"
fi
cd "$ROOT"

echo "==> Assembling $APP"
rm -rf "$APP" "$DMG"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/inkdown"
chmod +x "$APP/Contents/MacOS/inkdown"
sed "s/@VERSION@/$VERSION/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"

# icon: assets/icon.png -> Inkdown.icns
ICONSET=$(mktemp -d)/Inkdown.iconset
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
    sips -z $s $s assets/icon.png --out "$ICONSET/icon_${s}x${s}.png" > /dev/null
    d=$((s * 2))
    sips -z $d $d assets/icon.png --out "$ICONSET/icon_${s}x${s}@2x.png" > /dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/Inkdown.icns"

echo "==> Signing ($SIGN_IDENTITY)"
if [ "$SIGN_IDENTITY" = "-" ]; then
    codesign --force --deep --sign - "$APP"
else
    codesign --force --deep --options runtime --timestamp --sign "$SIGN_IDENTITY" "$APP"
fi
codesign --verify --deep --strict "$APP"

echo "==> Creating $DMG"
STAGE=$(mktemp -d)
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "Inkdown $VERSION" -srcfolder "$STAGE" -ov -format UDZO "$DMG" > /dev/null
rm -rf "$STAGE"

echo
echo "Done:"
echo "  $APP"
echo "  $DMG"
lipo -archs "$APP/Contents/MacOS/inkdown" | sed 's/^/  architectures: /'
