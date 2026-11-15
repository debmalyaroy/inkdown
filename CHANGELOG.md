# Changelog

Each release has full notes in `docs/releases/`.

## [1.2.0] - 2026-11-15

See [docs/releases/v1.2.0.md](docs/releases/v1.2.0.md).

- PDF files open as pages, drawn natively.
- Logs are coloured by severity, in any common format and at any size.
- Follow: new lines of a file appear as they are written.
- Open from a web address, refreshed while open; streams are shown as they arrive.
- Large files: read in the background with a third of the former peak memory; limit
  raised to 2 GB on desktop; word wrap and Find / Replace All now work on them.
- A new home page with a proper list of recent files and folders.
- Closing the window asks about unsaved work only when it could not be kept as a draft.

## [1.1.0] - 2026-11-15

See [docs/releases/v1.1.0.md](docs/releases/v1.1.0.md).

- Unsaved work is kept in draft files and restored after any kind of exit; a second
  launch hands its file to the running window.
- Notes: file drafts as Markdown into notebooks, file all drafts, browse notes.
- A phone interface: bottom mode switch, formatting strip, bottom sheets.
- Find, Replace and Find in Files understand look-around and back-references.
- Frame timings on the About screen.

## [1.0.3] - 2026-11-15

See [docs/releases/v1.0.3.md](docs/releases/v1.0.3.md).

- Android: the package is signed with a permanent key, so later versions install over
  it. No other changes.

## [1.0.2] - 2026-11-15

See [docs/releases/v1.0.2.md](docs/releases/v1.0.2.md).

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

[1.2.0]: https://github.com/debmalyaroy/inkdown/releases/tag/v1.2.0
[1.1.0]: https://github.com/debmalyaroy/inkdown/releases/tag/v1.1.0
[1.0.3]: https://github.com/debmalyaroy/inkdown/releases/tag/v1.0.3
[1.0.2]: https://github.com/debmalyaroy/inkdown/releases/tag/v1.0.2
[1.0.1]: https://github.com/debmalyaroy/inkdown/releases/tag/v1.0.1
[1.0.0]: https://github.com/debmalyaroy/inkdown/releases/tag/v1.0.0
