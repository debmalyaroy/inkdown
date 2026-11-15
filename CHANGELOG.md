# Changelog

Each release has full notes in `docs/releases/`.

## Unreleased

Not yet released; the Android changes are still being tested.

- Android: menu items act at once (a tapped item used to wait for the next touch).
- Android: the theme follows the system light or dark setting.
- Android: a finger drag always scrolls, a tap places the caret, the code editor keeps
  moving after a flick; the side drawer stays below the status bar; Back closes
  drawers, pickers and dialogs.
- Android: system file pickers for Open and Save As, files opened from another app while
  Inkdown is running, Print or Save as PDF, the system clipboard, and exports saved to
  the shared Downloads folder. The package now contains a small Java activity for these.
- All platforms: an Edit menu (Undo, Redo, Cut, Copy, Paste, Select All, Find, Replace);
  menus and the palette only list commands that work on the platform.

## [1.0.1] - 2026-11-14

See [docs/releases/v1.0.1.md](docs/releases/v1.0.1.md).

- Update check once a day and on demand; in-place update on Windows with a verified
  download.
- Help menu.
- Fixed: commands were ignored while a read-only document was active; the two Avro copy
  commands now work.
- Licensed under GPL-3.0-or-later; contributing guide, code of conduct, security policy and
  issue templates.

## [1.0.0] - 2026-11-14

First public release. See [docs/releases/v1.0.0.md](docs/releases/v1.0.0.md).

- Native, pure-Rust interface with no web view: about 70 ms to first frame, 74 MB of
  memory with a text file open.
- Read, Write, Split and Code modes for Markdown.
- Sublime-style code editor: multiple cursors, minimap, folding, snippets, build systems,
  Goto Anything, Find in Files, two panes.
- Native Mermaid, Graphviz, chart and LaTeX rendering; a visual diagram designer.
- JSON / YAML / TOML tree views, CSV tables, Avro data files (`.avro`) and schemas.
- Export to HTML, Word, RTF and PDF.
- Windows installer and portable exe; Android APK; macOS and Linux preview builds.

[1.0.1]: https://github.com/debmalyaroy/inkdown/releases/tag/v1.0.1
[1.0.0]: https://github.com/debmalyaroy/inkdown/releases/tag/v1.0.0
