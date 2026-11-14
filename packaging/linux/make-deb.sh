#!/bin/sh
# Builds a minimal .deb (binary, desktop entry, icon, MIME associations).
set -e
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
ROOT=target/deb/inkdown_${VERSION}_amd64
rm -rf "$ROOT" && mkdir -p "$ROOT/DEBIAN" "$ROOT/usr/bin" "$ROOT/usr/share/applications" "$ROOT/usr/share/icons/hicolor/256x256/apps"
cp target/release/inkdown "$ROOT/usr/bin/inkdown"
cp assets/icon.png "$ROOT/usr/share/icons/hicolor/256x256/apps/inkdown.png"
cat > "$ROOT/usr/share/applications/inkdown.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Inkdown
Comment=Markdown viewer and editor
Exec=inkdown %F
Icon=inkdown
Terminal=false
Categories=Office;TextEditor;Utility;
MimeType=text/markdown;text/x-markdown;text/plain;application/json;application/yaml;application/toml;text/csv;image/svg+xml;
EOF
cat > "$ROOT/DEBIAN/control" <<EOF
Package: inkdown
Version: ${VERSION}
Section: editors
Priority: optional
Architecture: amd64
Depends: libwebkit2gtk-4.1-0, libgtk-3-0
Maintainer: Inkdown <support@example.com>
Description: Markdown viewer and editor
 Word-style and source editing, diagrams, math, data viewers and export.
EOF
dpkg-deb --build --root-owner-group "$ROOT" "target/inkdown_${VERSION}_amd64.deb"