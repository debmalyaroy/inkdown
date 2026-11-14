#!/bin/sh
# Packs the release binary of the native app with a desktop entry and icon.
#   make linux      (or: cd native && cargo build --release, then this script)
# Output: dist/linux/inkdown-linux-x86_64.tar.gz
set -eu
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
cd "$ROOT"
VERSION=$(sed -n 's/^version *= *"\(.*\)"/\1/p' native/Cargo.toml | head -1)
ARCH=$(uname -m)
STAGE=$(mktemp -d)/inkdown-$VERSION
mkdir -p "$STAGE" dist/linux
cp native/target/release/inkdown "$STAGE/inkdown"
cp assets/icon.png "$STAGE/inkdown.png"
cat > "$STAGE/inkdown.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Inkdown
Comment=Universal editor for Markdown, code, data and diagrams
Exec=inkdown %F
Icon=inkdown
Terminal=false
Categories=Office;TextEditor;Utility;
MimeType=text/markdown;text/x-markdown;text/plain;application/json;application/yaml;application/toml;text/csv;image/svg+xml;
EOF
cat > "$STAGE/install.sh" <<'EOF'
#!/bin/sh
# Installs Inkdown for the current user (no root needed).
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$HOME/.local/bin" "$HOME/.local/share/applications" "$HOME/.local/share/icons/hicolor/256x256/apps"
install -m 755 "$HERE/inkdown" "$HOME/.local/bin/inkdown"
install -m 644 "$HERE/inkdown.png" "$HOME/.local/share/icons/hicolor/256x256/apps/inkdown.png"
install -m 644 "$HERE/inkdown.desktop" "$HOME/.local/share/applications/inkdown.desktop"
echo "Installed to $HOME/.local/bin/inkdown"
EOF
chmod +x "$STAGE/install.sh"
tar -C "$(dirname "$STAGE")" -czf "dist/linux/inkdown-linux-$ARCH.tar.gz" "inkdown-$VERSION"
ls -la dist/linux
