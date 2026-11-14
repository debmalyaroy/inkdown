# Inkdown for macOS

The macOS app is the same Rust code as the Windows app (`native/`). There is no
platform-specific UI code: windows, menus, text and graphics are all drawn by Inkdown.

## Build and install

On a Mac with the Xcode command line tools and Rust installed:

```sh
make macos          # or: sh packaging/macos/build.sh
open dist/macos/Inkdown-*.dmg
```

Drag **Inkdown** onto **Applications** in the window that opens.

`ARCH=native make macos` builds only for the Mac you are on, which halves the build time.
The default is a universal app (Apple silicon and Intel), macOS 11 or later.

## First launch of a downloaded build

Builds from GitHub Releases are ad-hoc signed, not notarised by Apple. macOS blocks them
the first time. Either right-click the app and choose **Open**, or run:

```sh
xattr -dr com.apple.quarantine /Applications/Inkdown.app
```

A build you make yourself with the script above opens without this step.

## Signing and notarising for distribution

Needs a paid Apple Developer account.

```sh
SIGN_IDENTITY="Developer ID Application: Your Name (TEAMID)" sh packaging/macos/build.sh
xcrun notarytool submit dist/macos/Inkdown-*.dmg --apple-id you@example.com --team-id TEAMID --wait
xcrun stapler staple dist/macos/Inkdown-*.dmg
```

## Status

The app is compiled and unit-tested on macOS by CI on every change, and CI builds the
`.dmg` for each release. It has not been used interactively on a Mac by the author, so
treat the macOS build as a preview. Known gaps:

- Opening a file by double-clicking it in Finder starts Inkdown but does not load the
  file yet (drag the file onto the window, or use File > Open).
- Shortcuts use `Ctrl` as on Windows; `Cmd` variants are not mapped.
- No Mac App Store package.
