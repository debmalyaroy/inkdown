//! An open document: buffer, file identity, encoding, and the state of each of its views.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use encoding_rs::Encoding;

use crate::buffer::Buffer;
use crate::edit::{detect_indent, EdCfg};
use crate::editor::EdView;
use crate::settings::Settings;
use crate::syntax::{self, Highlighter};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Markdown,
    /// json / yaml / toml
    Data(&'static str),
    /// delimiter
    Table(char),
    Mermaid,
    Dot,
    Svg,
    Asciidoc,
    Image,
    Code,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    Read,
    Write,
    Split,
    Code,
}

impl Mode {
    pub fn id(self) -> &'static str {
        match self {
            Mode::Read => "read",
            Mode::Write => "write",
            Mode::Split => "split",
            Mode::Code => "code",
        }
    }
    pub fn from_id(s: &str) -> Option<Self> {
        Some(match s {
            "read" => Mode::Read,
            "write" => Mode::Write,
            "split" => Mode::Split,
            "code" | "edit" => Mode::Code,
            _ => return None,
        })
    }
}

impl Kind {
    pub fn of(name: &str, first_line: &str) -> Self {
        let lower = name.to_lowercase();
        let ext = Path::new(&lower).extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
        match ext.as_str() {
            "md" | "markdown" | "mdown" | "mkd" | "mkdn" | "mdwn" | "mdx" => Kind::Markdown,
            // .avsc is an Avro schema (JSON); .avro data files are decoded to JSON when opened
            "json" | "geojson" | "jsonc" | "webmanifest" | "avsc" | "avro" => Kind::Data("json"),
            "yaml" | "yml" => Kind::Data("yaml"),
            "toml" => Kind::Data("toml"),
            "csv" => Kind::Table(','),
            "tsv" | "tab" => Kind::Table('\t'),
            "mmd" | "mermaid" => Kind::Mermaid,
            "dot" | "gv" => Kind::Dot,
            "svg" => Kind::Svg,
            "adoc" | "asciidoc" | "asc" => Kind::Asciidoc,
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" => Kind::Image,
            "" if !lower.contains('.') && !first_line.starts_with("#!") && matches!(lower.as_str(), "readme" | "changelog" | "notes" | "todo") => Kind::Markdown,
            _ => Kind::Code,
        }
    }

    pub fn modes(self) -> &'static [Mode] {
        match self {
            Kind::Markdown => &[Mode::Read, Mode::Write, Mode::Split, Mode::Code],
            Kind::Svg => &[Mode::Read, Mode::Write, Mode::Split, Mode::Code],
            Kind::Data(_) | Kind::Table(_) | Kind::Mermaid | Kind::Dot | Kind::Asciidoc => &[Mode::Read, Mode::Split, Mode::Code],
            Kind::Image => &[Mode::Read],
            Kind::Code => &[Mode::Code],
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Markdown => "Markdown",
            Kind::Data("json") => "JSON",
            Kind::Data("yaml") => "YAML",
            Kind::Data(_) => "TOML",
            Kind::Table(',') => "CSV",
            Kind::Table(_) => "TSV",
            Kind::Mermaid => "Mermaid",
            Kind::Dot => "Graphviz",
            Kind::Svg => "SVG",
            Kind::Asciidoc => "AsciiDoc",
            Kind::Image => "Image",
            Kind::Code => "Code",
        }
    }

    pub fn prose(self) -> bool {
        matches!(self, Kind::Markdown | Kind::Asciidoc)
    }
}

pub struct Doc {
    pub id: u64,
    pub path: Option<PathBuf>,
    pub name: String,
    pub buf: Buffer,
    pub hl: Highlighter,
    pub cfg: EdCfg,
    pub ed: EdView,
    pub kind: Kind,
    pub mode: Mode,
    pub encoding: &'static Encoding,
    pub bom: bool,
    pub crlf: bool,
    pub wrap: Option<bool>,
    /// Time of the last unsaved change, for AutoSave.
    pub edited_at: Option<f64>,
    pub mtime: Option<SystemTime>,
    pub read_only: bool,
    /// Raw bytes of an image file.
    pub bytes: Option<std::sync::Arc<[u8]>>,
    pub view: crate::preview::ViewState,
    /// Scratch buffers (search results, build output) are never "dirty".
    pub scratch: bool,
    /// Name of this document's draft file (see drafts.rs).
    pub draft: String,
    /// Buffer version last written to the draft file; `None` when no draft file exists.
    pub draft_version: Option<u64>,
}

fn next_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(1);
    N.fetch_add(1, Ordering::Relaxed)
}

pub fn decode(bytes: &[u8]) -> Result<(String, &'static Encoding, bool), String> {
    if let Some((enc, bom_len)) = Encoding::for_bom(bytes) {
        let (text, _) = enc.decode_without_bom_handling(&bytes[bom_len..]);
        return Ok((text.into_owned(), enc, true));
    }
    if bytes.iter().take(8192).any(|b| *b == 0) {
        // UTF-16 without a BOM: mostly-zero high bytes
        let n = bytes.len().min(4096) / 2;
        let zeros_odd = (0..n).filter(|i| bytes[i * 2 + 1] == 0).count();
        let zeros_even = (0..n).filter(|i| bytes[i * 2] == 0).count();
        if n > 8 && zeros_odd > n * 7 / 10 {
            return Ok((encoding_rs::UTF_16LE.decode_without_bom_handling(bytes).0.into_owned(), encoding_rs::UTF_16LE, false));
        }
        if n > 8 && zeros_even > n * 7 / 10 {
            return Ok((encoding_rs::UTF_16BE.decode_without_bom_handling(bytes).0.into_owned(), encoding_rs::UTF_16BE, false));
        }
        return Err("binary".into());
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => Ok((s.to_string(), encoding_rs::UTF_8, false)),
        Err(_) => Ok((encoding_rs::WINDOWS_1252.decode_without_bom_handling(bytes).0.into_owned(), encoding_rs::WINDOWS_1252, false)),
    }
}

pub fn encode(text: &str, enc: &'static Encoding, bom: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + 3);
    if enc == encoding_rs::UTF_16LE || enc == encoding_rs::UTF_16BE {
        let le = enc == encoding_rs::UTF_16LE;
        if bom {
            out.extend_from_slice(if le { &[0xff, 0xfe] } else { &[0xfe, 0xff] });
        }
        for u in text.encode_utf16() {
            out.extend_from_slice(&if le { u.to_le_bytes() } else { u.to_be_bytes() });
        }
        return out;
    }
    if bom && enc == encoding_rs::UTF_8 {
        out.extend_from_slice(&[0xef, 0xbb, 0xbf]);
    }
    out.extend_from_slice(&enc.encode(text).0);
    out
}

pub const ENCODINGS: &[(&str, &str)] = &[
    ("UTF-8", "utf-8"),
    ("UTF-8 with BOM", "utf-8-bom"),
    ("UTF-16 LE", "utf-16le"),
    ("UTF-16 BE", "utf-16be"),
    ("Western (Windows 1252)", "windows-1252"),
    ("Western (ISO 8859-1)", "iso-8859-1"),
    ("Central European (Windows 1250)", "windows-1250"),
    ("Cyrillic (Windows 1251)", "windows-1251"),
    ("Greek (Windows 1253)", "windows-1253"),
    ("Turkish (Windows 1254)", "windows-1254"),
    ("Japanese (Shift JIS)", "shift_jis"),
    ("Japanese (EUC-JP)", "euc-jp"),
    ("Chinese (GBK)", "gbk"),
    ("Chinese (Big5)", "big5"),
    ("Korean (EUC-KR)", "euc-kr"),
];

/// Records decoded from an Avro data file; the rest are counted but not shown.
pub const AVRO_RECORD_LIMIT: usize = 20_000;

impl Doc {
    pub fn new(path: Option<PathBuf>, name: String, text: &str, set: &Settings) -> Self {
        let first = text.lines().next().unwrap_or("");
        let kind = Kind::of(&name, first);
        let syntax = syntax::find(&name, first);
        let mut d = Self {
            id: next_id(),
            path,
            name,
            buf: Buffer::new(text),
            hl: Highlighter::new(syntax),
            cfg: EdCfg::default(),
            ed: EdView::default(),
            kind,
            mode: Mode::Code,
            encoding: encoding_rs::UTF_8,
            bom: false,
            crlf: false, // new documents use LF on every platform
            wrap: None,
            edited_at: None,
            mtime: None,
            read_only: false,
            bytes: None,
            view: Default::default(),
            scratch: false,
            draft: crate::drafts::new_id(),
            draft_version: None,
        };
        d.mode = d.default_mode(set);
        d.apply_settings(set, text);
        d
    }

    pub fn default_mode(&self, set: &Settings) -> Mode {
        match self.kind {
            Kind::Markdown => {
                let m = Mode::from_id(&set.markdown_mode).unwrap_or(Mode::Read);
                if self.buf.len() == 0 && m == Mode::Read { Mode::Write } else { m }
            }
            Kind::Code => Mode::Code,
            Kind::Image => Mode::Read,
            _ => Mode::Split,
        }
    }

    pub fn apply_settings(&mut self, set: &Settings, sample: &str) {
        self.cfg.tab_width = set.tab_size.clamp(1, 16);
        self.cfg.soft_tabs = set.translate_tabs_to_spaces;
        self.cfg.auto_pair = set.auto_match_enabled;
        self.cfg.auto_indent = set.auto_indent;
        if matches!(self.hl.name(), "Makefile" | "Go") {
            self.cfg.soft_tabs = false;
        }
        if matches!(self.kind, Kind::Data("yaml")) {
            self.cfg.tab_width = 2;
            self.cfg.soft_tabs = true;
        }
        if set.detect_indentation {
            if let Some((soft, w)) = detect_indent(sample) {
                self.cfg.soft_tabs = soft;
                if soft {
                    self.cfg.tab_width = w;
                }
            }
        }
        self.sync_comment_tokens();
    }

    pub fn sync_comment_tokens(&mut self) {
        let (line, block) = match self.kind {
            Kind::Mermaid => (Some("%%"), None),
            Kind::Dot => (Some("//"), Some(("/*", "*/"))),
            Kind::Asciidoc => (Some("//"), None),
            _ => syntax::comment_tokens(self.hl.name()),
        };
        self.cfg.line_comment = line.map(String::from);
        self.cfg.block_comment = block.map(|(a, b)| (a.to_string(), b.to_string()));
    }

    pub fn open(path: &Path, set: &Settings) -> Result<Self, String> {
        let meta = std::fs::metadata(path).map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
        if meta.is_dir() {
            return Err("folder".into());
        }
        if meta.len() > 600 * 1024 * 1024 {
            return Err(format!("{} is too large to open ({} MB).", path.display(), meta.len() / 1024 / 1024));
        }
        let bytes = std::fs::read(path).map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let is_image = Kind::of(&name, "") == Kind::Image;
        // an Avro data file is binary: show its schema and records as JSON, read-only
        let avro = if crate::avro::is_container(&bytes) { Some(crate::avro::read(&bytes, AVRO_RECORD_LIMIT)?) } else { None };
        let decoded = match &avro {
            Some(f) => Ok((f.to_json_text(), encoding_rs::UTF_8, false)),
            None if is_image => Ok((String::new(), encoding_rs::UTF_8, false)),
            None => decode(&bytes),
        };
        let (text, enc, bom, binary) = match decoded {
            Ok((t, e, b)) => (t, e, b, false),
            Err(_) => (hex_dump(&bytes), encoding_rs::UTF_8, false, true),
        };
        let crlf = text.as_bytes().windows(2).take(200_000).any(|w| w == b"\r\n");
        let text = if crlf || text.contains('\r') { text.replace("\r\n", "\n").replace('\r', "\n") } else { text };
        let mut d = Self::new(Some(path.to_path_buf()), name, &text, set);
        d.encoding = enc;
        d.bom = bom;
        d.crlf = crlf;
        d.mtime = meta.modified().ok();
        if is_image {
            d.bytes = Some(bytes.into());
            d.read_only = true;
        }
        if avro.is_some() {
            d.read_only = true;
            d.crlf = false;
            if !matches!(d.kind, Kind::Data("json")) {
                // an Avro file under another extension
                d.kind = Kind::Data("json");
                d.mode = d.default_mode(set);
                d.hl = Highlighter::new(syntax::find("x.json", ""));
            }
        }
        if binary {
            d.read_only = true;
            d.kind = Kind::Code;
            d.mode = Mode::Code;
            d.hl = Highlighter::new(syntax::set().find_syntax_plain_text());
        }
        Ok(d)
    }

    pub fn dirty(&self) -> bool {
        !self.scratch && !self.read_only && self.buf.dirty()
    }

    pub fn text_for_disk(&mut self, set: &Settings) -> String {
        if set.trim_trailing_white_space_on_save && self.kind != Kind::Markdown {
            crate::edit::exec(&mut self.buf, crate::edit::Cmd::TrimTrailing, &self.cfg);
        }
        if set.ensure_newline_at_eof_on_save && self.buf.len() > 0 && self.buf.char_at(self.buf.len() - 1) != Some('\n') {
            let n = self.buf.len();
            let sels = self.buf.sels.clone();
            self.buf.edit(vec![crate::buffer::Edit::new(n, n, "\n")], crate::buffer::EditKind::Other);
            self.buf.sels = sels;
            self.buf.commit_sels();
        }
        let text = self.buf.text();
        if self.crlf { text.replace('\n', "\r\n") } else { text }
    }

    pub fn save(&mut self, set: &Settings) -> Result<(), String> {
        let Some(path) = self.path.clone() else { return Err("No file name.".into()) };
        if self.read_only {
            return Err("This file is read-only in Inkdown.".into());
        }
        let text = self.text_for_disk(set);
        let bytes = encode(&text, self.encoding, self.bom);
        // write beside the target and rename, so a crash never leaves half a file
        let tmp = path.with_extension(format!("{}.inkdown-tmp", path.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default()));
        let result = std::fs::write(&tmp, &bytes).and_then(|_| std::fs::rename(&tmp, &path));
        if let Err(e) = result {
            let _ = std::fs::remove_file(&tmp);
            // fall back to a direct write (e.g. the folder is not writable but the file is)
            std::fs::write(&path, &bytes).map_err(|e2| format!("Cannot save {}: {e} / {e2}", path.display()))?;
        }
        self.buf.mark_saved();
        self.edited_at = None;
        self.mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        #[cfg(any(target_os = "android", target_os = "ios"))]
        crate::mobile::after_save(&path, &bytes)?;
        Ok(())
    }

    /// Re-detect kind and syntax after a rename / Save As.
    pub fn set_path(&mut self, path: PathBuf, set: &Settings) {
        self.name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        self.path = Some(path);
        let first = self.buf.line_text(0);
        let kind = Kind::of(&self.name, &first);
        if kind != self.kind {
            self.kind = kind;
            if !kind.modes().contains(&self.mode) {
                self.mode = self.default_mode(set);
            }
            self.view = Default::default();
        }
        self.hl = Highlighter::new(syntax::find(&self.name, &first));
        self.sync_comment_tokens();
    }

    pub fn set_syntax(&mut self, name: &str) {
        if let Some(s) = syntax::by_name(name) {
            self.hl = Highlighter::new(s);
            self.sync_comment_tokens();
        }
    }

    pub fn wrap(&self, set: &Settings) -> bool {
        self.wrap.unwrap_or(match set.word_wrap.as_str() {
            "true" => true,
            "false" => false,
            _ => self.kind.prose() || self.hl.name() == "Plain Text",
        })
    }

    pub fn dir(&self) -> Option<PathBuf> {
        self.path.as_ref().and_then(|p| p.parent()).map(Path::to_path_buf)
    }

    /// True when the file on disk changed since we loaded or saved it.
    pub fn changed_on_disk(&self) -> bool {
        match (&self.path, self.mtime) {
            (Some(p), Some(t)) => std::fs::metadata(p).and_then(|m| m.modified()).map(|m| m != t).unwrap_or(false),
            _ => false,
        }
    }

    pub fn reload(&mut self, set: &Settings) -> Result<(), String> {
        let Some(path) = self.path.clone() else { return Ok(()) };
        let fresh = Doc::open(&path, set)?;
        let caret = self.buf.primary().head;
        self.buf.set_text(&fresh.buf.text());
        self.buf.set_caret(caret.min(self.buf.len()));
        self.buf.mark_saved();
        self.mtime = fresh.mtime;
        self.bytes = fresh.bytes;
        self.edited_at = None;
        Ok(())
    }
}

fn hex_dump(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let shown = bytes.len().min(4 * 1024 * 1024);
    let mut out = String::with_capacity(shown * 4);
    for (i, chunk) in bytes[..shown].chunks(16).enumerate() {
        let _ = write!(out, "{:08x}  ", i * 16);
        for (j, b) in chunk.iter().enumerate() {
            let _ = write!(out, "{b:02x}{}", if j == 7 { "  " } else { " " });
        }
        for j in chunk.len()..16 {
            out.push_str(if j == 7 { "    " } else { "   " });
        }
        out.push(' ');
        for b in chunk {
            out.push(if b.is_ascii_graphic() || *b == b' ' { *b as char } else { '.' });
        }
        out.push('\n');
    }
    if shown < bytes.len() {
        let _ = writeln!(out, "… {} more bytes not shown", bytes.len() - shown);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodings_round_trip() {
        let (t, e, bom) = decode(&[0xef, 0xbb, 0xbf, b'h', b'i']).unwrap();
        assert_eq!((t.as_str(), e, bom), ("hi", encoding_rs::UTF_8, true));
        assert_eq!(encode("hi", e, bom), vec![0xef, 0xbb, 0xbf, b'h', b'i']);
        let utf16 = encode("héllo", encoding_rs::UTF_16LE, true);
        let (t, e, bom) = decode(&utf16).unwrap();
        assert_eq!((t.as_str(), e, bom), ("héllo", encoding_rs::UTF_16LE, true));
        let (t, e, _) = decode(&[b'c', b'a', b'f', 0xe9]).unwrap();
        assert_eq!((t.as_str(), e), ("café", encoding_rs::WINDOWS_1252));
        assert!(decode(&[0x89, b'P', b'N', b'G', 0, 0, 0, 1, 2, 3]).is_err());
        assert_eq!(Kind::of("a.YML", ""), Kind::Data("yaml"));
        assert_eq!(Kind::of("build.sh", "#!/bin/sh"), Kind::Code);
    }
}
