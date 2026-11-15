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
and emoji, touch scrolling, all four modes, every menu and submenu, typing with the
on-screen keyboard, the file pickers, opening a file while running, the print dialog,
exports to Downloads, and Save writing back to the original file. Memory was about 54 MB. It has not been run on a physical phone or on arm64.

## How the package is put together

`cargo apk` builds the native libraries. `packaging/android/package.sh` then assembles the
APK from them, the manifest (`AndroidManifest.xml`) and a small Java activity
(`java/app/inkdown/editor/MainActivity.java`). The activity adds what Android only offers
to Java: the system file pickers, documents that arrive while the app is running, and
printing. Everything else is the same Rust code as on the desktop.

## What works on Android

- Reading and editing in all four modes, themes (following the system light or dark
  setting), the command palette, the Edit menu with the system clipboard.
- **Open** and **Save As** through the system file pickers; **Open with Inkdown** from a
  file manager or another app, also while Inkdown is running. Saving writes back to the
  original document.
- **Print or Save as PDF** through the system print dialog.
- Export to HTML, Word and RTF, and diagrams to PNG or SVG, saved to `Downloads/Inkdown`;
  **Save a Copy to Downloads** for the document itself.
- The app's own documents folder, shown in the drawer.

## Not available on Android

- Two-pane layout, a second window, build systems and "Show in File Manager".
- Google Docs upload and the in-place updater (the update check opens the releases page).
- The on-screen keyboard uses `NativeActivity` key events: plain typing works, but
  predictive text and non-Latin input methods need the `GameActivity` text input path.

## Signing

Each image build generates a new throwaway test key, so an APK from one build cannot be
installed over an APK from another: Android asks to uninstall first. For real releases
keep one keystore, and pass it to the build as `CARGO_APK_RELEASE_KEYSTORE` and
`CARGO_APK_RELEASE_KEYSTORE_PASSWORD` (in CI: as repository secrets).

## Publishing to Google Play

1. Create an upload key: `keytool -genkeypair -v -keystore upload.keystore -alias inkdown -keyalg RSA -keysize 2048 -validity 10000`.
2. Build with `CARGO_APK_RELEASE_KEYSTORE=/path/upload.keystore` and
   `CARGO_APK_RELEASE_KEYSTORE_PASSWORD=...` set (replace the test key lines in the Dockerfile).
3. Google Play requires an `.aab` bundle for new apps: convert with `bundletool`, or build
   with `xbuild` (`x build --platform android --format aab`), then upload it in Play Console.
