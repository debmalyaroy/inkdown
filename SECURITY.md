# Security Policy

## Supported versions

Only the latest release receives fixes.

## Reporting a vulnerability

Please do not open a public issue for a security problem.

Report it privately with GitHub's
[private vulnerability reporting](https://github.com/debmalyaroy/inkdown/security/advisories/new)
(*Security* tab > *Report a vulnerability*). Include what you found, how to reproduce it,
and the version and platform.

You should get a first answer within a week. Once a fix is released the report is
credited in the release notes, unless you prefer otherwise.

## What to know about the builds

- The Windows installer and the macOS app are **not code-signed** yet, and the Android APK
  is signed with a test key. Check downloads against `SHA256SUMS.txt` on the release page.
- The updater downloads the installer over HTTPS from this repository's releases and runs
  it only if its SHA-256 matches the published checksum. That protects against a damaged
  download, not against a compromised release.
- Inkdown opens files from disk and renders Markdown, SVG, diagrams and Avro data. A crash
  or hang caused by a crafted file is a bug worth reporting.
- Remote images in a document are fetched when the document is shown.
- The update check is one request a day to GitHub with no identifying data. Turn it off
  with `"check_for_updates": false` in the settings.
