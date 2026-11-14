# Inkdown (native)

The pure-Rust Inkdown: one code base for Windows, Linux, macOS, Android and iOS, with no
web view and no JavaScript. The UI is drawn with egui; documents are parsed, laid out and
painted by code in this crate.

```
cargo run --release -- path\to\file.md      # run
cargo test --lib                             # unit tests
pwsh tools\uitest.ps1 -Release               # UI regression suite (scripted, no real input)
```

## What it is

A universal text editor in the spirit of Sublime Text, with first-class documents:

| Area | What is there |
| --- | --- |
| Code editor | Rope buffer, virtualised painting, multiple cursors (`Ctrl+D`, `Ctrl+Alt+↑/↓`, `Ctrl+click`, column select with `Alt`+drag), Sublime key map with chords, undo groups, folding, bookmarks, minimap, bracket matching, indent guides, word wrap, auto-pairs, auto-indent, snippets with tab stops, word completion, macros |
| Languages | Sublime grammars through syntect (the `bat` set, 200+ languages), incremental and time-budgeted; drop extra `.sublime-syntax` files into `<config>/Packages` |
| Navigation | Goto Anything (`Ctrl+P`: files, `:line`, `@symbol`, `#word`, `>command`), command palette, Go to Definition (`F12`, search based), Find in Files with replace, outline panel, folder tree |
| Files | Tabs with drag reorder, two-column split, session restore with unsaved buffers, AutoSave, reload on external change, encodings (UTF-8/16, Windows-125x, Shift JIS, GBK …), LF/CRLF, indentation detection and conversion, hex view of binary files |
| Markdown | **Read** (typeset page), **Write** (Word-style: markup hidden except on the line being edited), **Split** (source + synchronised preview), **Code**; GFM tables, task lists, callouts, footnotes, definition lists, front matter, `<kbd>`, highlights, sub/superscript, collapsible sections |
| Rendering | Math (LaTeX via `latex-rust`), Mermaid (`mermaid-rs-renderer`), Graphviz (`layout-rs`), charts from ECharts-style JSON (`chart.rs`), SVG and raster images, all rasterised off the UI thread |
| Other formats | JSON / YAML / TOML tree with validation, formatting and conversion; CSV / TSV table with sorting; AsciiDoc (translated to Markdown); `.mmd`, `.dot`, `.svg` with a pan-and-zoom preview |
| Designer | Visual diagram editor for `.svg` (blocks, connectors, colours, align/distribute); saves standard SVG with the model embedded; copy as Mermaid |
| Export | HTML (self-contained), Word `.docx`, RTF, PDF (through Edge/Chrome headless), rich clipboard, diagram PNG/SVG, Google Docs upload with the user's own Google sign-in |
| Customising | `settings.json`, `keybindings.json` (Sublime format), `snippets.json`, user build systems; nine colour themes |

## Layout

```
src/app.rs        state, frame loop, command dispatcher, split panes
src/shell.rs      top bar, tabs, status bar, menus, dialogs, toasts
src/editor.rs     the code editor widget          src/buffer.rs  rope, selections, undo
src/edit.rs       editing commands                src/keymap.rs  key bindings
src/syntax.rs     incremental highlighting        src/theme.rs   palettes, fonts, style
src/md.rs         Markdown model and layout       src/preview.rs reader, tree, table, canvas
src/rich.rs       Write mode                      src/designer.rs diagram designer
src/assets.rs     async images/diagrams/math      src/chart.rs   charts as SVG
src/html.rs       HTML output                     src/export_ui.rs exports, integrations
src/picker.rs     palette / Goto Anything         src/sidebar.rs folder tree, outline, index
src/project.rs    find in files, builds, goto definition
src/automation.rs scripted UI driver for tests
```

`../src/export.rs` (Word/RTF writers), `../src/google.rs` and `../src/data.rs` are shared
with the older web-view build and included by path.

## Design notes

- **Speed.** Only what is on screen is laid out. The editor paints visible rows; the
  reader and Write mode cache the layout of each block or line by content hash, so a
  keystroke re-lays out one block. Syntax state is checkpointed every 32 lines.
- **Memory.** No browser engine. Textures for diagrams and images are dropped when they
  have not been drawn for a while. Fonts are bundled (Inter, JetBrains Mono, a Lucide
  icon subset, STIX Two Math inside `latex-rust`).
- **Testing.** `INKDOWN_AUTOMATION=<script>` feeds synthetic input through the normal
  event path and writes screenshots and JSON state dumps; `INKDOWN_DATA_DIR` isolates
  the profile; `INKDOWN_DIALOG_DIR` answers save dialogs. See `tools/uitest.ps1`.

## Known gaps

- No plugin API and no LSP client. Sublime's Python packages do not run here.
- A file can be shown in only one pane at a time; layouts other than one or two columns
  are not implemented.
- PDF export shells out to an installed Edge or Chrome; there is no built-in PDF writer.
- Find uses the `regex` crate: no look-behind or back-references.
- AsciiDoc and Mermaid cover the common subset, not every construct.
- Write mode is line based: a paragraph hard-wrapped over several source lines is shown
  as several lines.

## Platforms

- Windows: `cargo build --release`; installer and Store package in `../packaging/windows`.
- Linux: `../packaging/linux` (Docker).
- Android: `../packaging/android` (Docker, cargo-apk).
- iOS: `../packaging/ios` (needs a Mac; prepared but not built).
