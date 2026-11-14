# Inkdown

A fast universal text editor with first-class Markdown: a Word-style editor, live previews,
diagrams, data viewers and rich export. The host is Rust (`tao` + `wry`), one self-contained
executable per platform; everything renders offline.

## What it does

**Any text file.** Syntax highlighting for 120+ languages (loaded on demand), Sublime-style
keys (multi-cursor, `Ctrl+D`, line move/duplicate, comment toggle), find and replace with regex,
folding, tabs, folder browser, AutoSave, auto-reload when a file changes on disk.

**Markdown, three ways.** *Read* (clean reader), *Write* (Word-style WYSIWYG that saves plain
Markdown) and *Markdown* (source with a synchronised live preview). CommonMark + GitHub Flavored
Markdown, footnotes, callouts, front matter, math (KaTeX), task lists that tick in place.

**Diagrams.**
- Mermaid, Graphviz and chart (ECharts JSON) code blocks render live; `.mmd`, `.dot` and `.svg`
  files open as code + live preview.
- A visual designer (Insert > Drawing, or New > Drawing) with a block palette, connectors,
  colours, align/distribute, undo, zoom. Drawings are saved as standard SVG with the editable
  model embedded, so they display anywhere and reopen for editing in Inkdown. "Copy as Mermaid"
  converts the structure to flowchart code.

**Structured data.** JSON, YAML and TOML get a collapsible tree view, validation with the error
line highlighted, Format / Minify, and conversion between the three. CSV/TSV shows as a table.
AsciiDoc and HTML render with a live preview.

**Export.** Word (`.docx`), Rich Text (`.rtf`), HTML, PDF/print, Google Docs (formatted clipboard
hand-off or .docx for Drive) and "Copy with formatting". Diagrams, charts and formulas are
embedded as images.

**Tools.** Table of contents, document statistics, tidy Markdown, text to table, change case,
sort lines, symbols and emoji, date/time, document templates, nine themes, zoom, command palette
(`Ctrl+Shift+P`).

## Building

| Target | Command | Output |
| --- | --- | --- |
| Windows exe | `cargo build --release` | `target\release\inkdown.exe` |
| Windows installer | copy the exe as `Inkdown-Setup.exe` | double-click installs per-user and launches |
| Microsoft Store package | `pwsh packaging\windows\make-msix.ps1 -IdentityName … -Publisher … -PublisherDisplayName …` | `dist\Inkdown-<version>-x64.msix` |
| Linux x86-64 | `docker build -f packaging/linux/Dockerfile -t inkdown-linux .` then `docker run --rm -v "${PWD}/dist/linux:/out" inkdown-linux` | `dist/linux/inkdown`, `.deb` |
| macOS | `cargo build --release` on a Mac (code is portable; bundling not set up yet) | |

`inkdown.exe --install` / `--uninstall` (add `--quiet`) manage the per-user Windows install.
Windows protects default-app choices: confirm Inkdown for `.md` once under
Settings > Apps > Default apps > Inkdown.

Front-end libraries are vendored in `assets\vendor`; `vendor-src\` holds the npm manifest and the
esbuild entry files used to produce the Tiptap and AsciiDoc bundles.

## Layout

| Path | Purpose |
| --- | --- |
| `src\main.rs` | Window, web view host, IPC commands, file watching |
| `src\render.rs` | Markdown to HTML (comrak), incl. the variant for the rich editor |
| `src\export.rs` | DOCX and RTF writers |
| `src\data.rs` | JSON / YAML / TOML parsing and conversion |
| `src\install.rs`, `src\install_other.rs` | Windows install and file types; other-platform stand-in |
| `assets\js\core.js` | IPC bridge, widgets, path helpers |
| `assets\js\preview.js` | Rendering, diagrams, lightbox |
| `assets\js\editor.js` | Code editor (CodeMirror), language loading |
| `assets\js\rich.js` | Word-style editor (Tiptap) and its Markdown serializer |
| `assets\js\kinds.js` | File kinds, data tree, CSV table, AsciiDoc |
| `assets\js\designer.js` | Visual diagram designer |
| `assets\js\export.js` | Rasterising graphics for export, clipboard HTML |
| `assets\js\app.js` | Shell: tabs, modes, commands, tools, themes |
| `packaging\` | MSIX script, Linux Dockerfile and .deb builder |
| `tools\` | Test driver over the DevTools protocol (`cdp.mjs`) and self-test suites |

## Testing

`cargo test --release` covers rendering, export and data conversion. For the UI: `cargo build`,
start with `INKDOWN_DEBUG_PORT=9333` and `INKDOWN_DATA_DIR=<temp dir>` (an isolated profile), then
`node tools\cdp.mjs eval "$(Get-Content tools\selftest.js -Raw)"` and likewise for
`selftest2.js` (Write mode, tools, export), `selftest3.js` (file formats) and `selftest4a/b.js`
(designer).
