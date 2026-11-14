# Inkdown for iOS and iPadOS

Status: **prepared, not built.** iOS apps can only be compiled and signed on a Mac with
Xcode, and this project has so far been built on Windows and Linux only. The files here
are the starting point; expect to fix small things on the first real build.

The app is the same pure-Rust code as the desktop app (`native/`). iOS has no OpenGL, so it
uses the `wgpu` renderer (Metal) instead of `glow`:

```sh
sh packaging/ios/build.sh sim       # simulator: builds, installs and launches
sh packaging/ios/build.sh device    # device build, then sign (below)
```

## What the build script does

1. `cargo build --release --bin inkdown --target aarch64-apple-ios --no-default-features --features wgpu`
2. Assembles `dist/ios/<mode>/Inkdown.app` from the binary and `Info.plist`.
3. Simulator: `xcrun simctl install` + `launch`.

`Info.plist` registers Inkdown as an editor for Markdown, plain text, source code, JSON,
YAML, CSV and SVG, enables "open in place", and exposes the app's Documents folder in the
Files app.

## Signing and the App Store

- Development device: `codesign -s "Apple Development: NAME" Inkdown.app` and embed the
  provisioning profile as `Inkdown.app/embedded.mobileprovision`.
- App Store: an Apple Developer Program membership is required. Create the App ID
  `app.inkdown.editor`, a distribution certificate and profile, wrap the `.app` in an Xcode
  archive (`xcodebuild -exportArchive`) or use Transporter to upload the `.ipa`.

## Known work left for iOS

- Receiving the document URL when the app is opened from Files (needs the
  `application:openURL:` hook; winit does not surface it yet).
- Safe-area insets (notch and home indicator) for the top bar and status bar.
- Hardware-keyboard shortcuts use Cmd in place of Ctrl; this is mapped but untested.
- Exports that rely on desktop programs (PDF through Edge/Chrome, build systems) are not
  available.
