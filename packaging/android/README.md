# Inkdown for Android

The Android app is the same pure-Rust code as the desktop app (`native/`), built as a
`NativeActivity` with `cargo-apk`. Phones get the narrow layout automatically (drawer
instead of a sidebar, icon-only mode switch).

## Build

Nothing needs to be installed on the host except Docker:

```sh
docker build -f packaging/android/Dockerfile -t inkdown-android .
docker run --rm -v "${PWD}/dist/android:/out" inkdown-android
```

The result is `dist/android/inkdown.apk` (arm64-v8a and x86_64), signed with a throwaway
test key that is generated inside the image. The image accepts the Android SDK licence
agreement when it installs the SDK (`sdkmanager --licenses`).

The first build takes about ten minutes. Later builds reuse a BuildKit cache and take
three to five. Add `--build-arg JOBS=4` to use less memory while compiling.

Install on a device with USB debugging enabled: `adb install dist/android/inkdown.apk`.
The app writes a few diagnostic lines to the device log: `adb logcat -s Inkdown`.

## Tested

On an Android 15 emulator (x86_64, Pixel 6 profile): launch, the tour page with diagrams
and emoji, touch scrolling, all four modes, typing with the on-screen keyboard in Code and
Write mode, "Open with Inkdown" for a Markdown file in Downloads, and Save writing back to
that file. Memory was about 54 MB. It has not been run on a physical phone or on arm64.

## What works on Android

- Reading and editing with all four modes, themes, the command palette and exports that
  need no desktop program (HTML, Word, RTF written to the app's documents folder).
- "Open with Inkdown" from a file manager or another app (text, Markdown, JSON, YAML,
  TOML, SVG). Saving writes back to the original document when the sending app allows it.
- The app's own documents folder, shown in the drawer.

## Not done yet

- A system file picker inside the app (needs a small Java activity for
  `ACTION_OPEN_DOCUMENT`; `NativeActivity` alone cannot receive the result).
- Opening a second document while the app is already running (`onNewIntent`).
- The on-screen keyboard uses `NativeActivity` key events: plain typing works, but
  predictive text and non-Latin input methods need the `GameActivity` text input path.
- PDF export, build systems, Find in Files across a folder picker and Google Docs upload
  are desktop features.

## Publishing to Google Play

1. Create an upload key: `keytool -genkeypair -v -keystore upload.keystore -alias inkdown -keyalg RSA -keysize 2048 -validity 10000`.
2. Build with `CARGO_APK_RELEASE_KEYSTORE=/path/upload.keystore` and
   `CARGO_APK_RELEASE_KEYSTORE_PASSWORD=...` set (replace the test key lines in the Dockerfile).
3. Google Play requires an `.aab` bundle for new apps: convert with `bundletool`, or build
   with `xbuild` (`x build --platform android --format aab`), then upload it in Play Console.
