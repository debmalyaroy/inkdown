# Notices

Inkdown
Copyright 2026 the Inkdown authors

This program is free software: you can redistribute it and/or modify it under the terms
of the GNU General Public License as published by the Free Software Foundation, either
version 3 of the License, or (at your option) any later version.

This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY;
without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
See the GNU General Public License in `LICENSE` for more details.

## Third-party notices

Inkdown bundles the following fonts. Their licence texts are in `native/fonts/`.

| Font | Use | Licence |
| --- | --- | --- |
| Inter, Inter Italic | Interface and document text | SIL Open Font License 1.1 (`OFL-Inter.txt`) |
| JetBrains Mono | Code | SIL Open Font License 1.1 (`OFL-JetBrainsMono.txt`) |
| Lucide (subset, as `icons.ttf`) | Icons | ISC License (`LICENSE-Lucide.txt`) |

The application is built from open-source Rust crates listed in `native/Cargo.lock`, each
under its own licence (mostly MIT or Apache-2.0). The main ones: `egui` / `eframe` (user
interface), `syntect` and `two-face` (syntax definitions), `comrak` (Markdown parsing),
`resvg` (SVG rendering), `mermaid-rs-renderer`, `layout-rs` (Graphviz layout), `latex-rust`
(math), `ropey` (text buffer).

The earlier WebView build in the repository root bundles JavaScript libraries in
`assets/vendor/` (CodeMirror, Mermaid, KaTeX, ECharts, highlight.js, Tiptap, Asciidoctor,
Viz.js, DOMPurify), each under its own licence as stated in its files.
