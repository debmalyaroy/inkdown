#!/bin/sh
# Assembles the Android package: the native libraries built by cargo, the Java activity
# (packaging/android/java) and the manifest. Run inside the build image (see Dockerfile).
#
#   sh packaging/android/package.sh <cargo-built.apk> <output.apk>
#
# Needs ANDROID_HOME with platform 34 and build-tools 34.0.0, a JDK, zip and unzip.
# Signs with $CARGO_APK_RELEASE_KEYSTORE / $CARGO_APK_RELEASE_KEYSTORE_PASSWORD.
set -eu
# absolute paths: the script changes directory while it works
IN=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
mkdir -p "$(dirname "$2")"
OUT=$(cd "$(dirname "$2")" && pwd)/$(basename "$2")
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
TOOLS="$ANDROID_HOME/build-tools/34.0.0"
JAR="$ANDROID_HOME/platforms/android-34/android.jar"
WORK=$(mktemp -d)

VERSION=$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$ROOT/native/Cargo.toml" | head -1)
MAJOR=$(echo "$VERSION" | cut -d. -f1)
MINOR=$(echo "$VERSION" | cut -d. -f2)
PATCH=$(echo "$VERSION" | cut -d. -f3 | sed 's/[^0-9].*//')
CODE=$((MAJOR * 10000 + MINOR * 100 + PATCH))
echo "==> Packaging Inkdown $VERSION (version code $CODE)"

# 1. the Java activity -> classes.dex
mkdir -p "$WORK/classes" "$WORK/dex"
javac -source 8 -target 8 -Xlint:-options -classpath "$JAR" -d "$WORK/classes" $(find "$HERE/java" -name '*.java')
"$TOOLS/d8" --lib "$JAR" --min-api 26 --output "$WORK/dex" $(find "$WORK/classes" -name '*.class')

# 2. resources and manifest -> base package
sed -e "s/@VERSION@/$VERSION/g" -e "s/@VERSION_CODE@/$CODE/g" "$HERE/AndroidManifest.xml" > "$WORK/AndroidManifest.xml"
"$TOOLS/aapt2" compile --dir "$HERE/res" -o "$WORK/res.zip"
"$TOOLS/aapt2" link -o "$WORK/base.apk" -I "$JAR" --manifest "$WORK/AndroidManifest.xml" "$WORK/res.zip"

# 3. add the code: classes.dex and the native libraries cargo built
mkdir -p "$WORK/stage"
cd "$WORK/stage"
unzip -q "$IN" 'lib/*'
cp "$WORK/dex/classes.dex" .
zip -q -r "$WORK/base.apk" classes.dex lib
cd "$ROOT"

# 4. align and sign
"$TOOLS/zipalign" -f 4 "$WORK/base.apk" "$WORK/aligned.apk"
#    ANDROID_KEYSTORE + ANDROID_KEYSTORE_PASSWORD_FILE name the release key; without them
#    the throwaway test key of the build image is used.
if [ -n "${ANDROID_KEYSTORE:-}" ]; then
    echo "==> Signing with the release key"
    "$TOOLS/apksigner" sign --ks "$ANDROID_KEYSTORE" --ks-pass "file:$ANDROID_KEYSTORE_PASSWORD_FILE" --out "$OUT" "$WORK/aligned.apk"
else
    echo "==> Signing with the test key (not for distribution)"
    "$TOOLS/apksigner" sign --ks "$CARGO_APK_RELEASE_KEYSTORE" --ks-pass "pass:$CARGO_APK_RELEASE_KEYSTORE_PASSWORD" --out "$OUT" "$WORK/aligned.apk"
fi
"$TOOLS/apksigner" verify --print-certs "$OUT" | grep -E "SHA-256|DN:"
rm -rf "$WORK"
ls -la "$OUT"
