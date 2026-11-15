//! The code editor widget: a virtualised, multi-cursor text view.
//!
//! Only the rows on screen are laid out and painted, so cost per frame is independent of
//! file size. Keyboard commands arrive through the app key map (`command`); this widget
//! handles text input, the mouse, scrolling and painting.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::text::{CCursor, LayoutJob};
use egui::{pos2, vec2, Color32, CornerRadius, CursorIcon, Event, FontId, Galley, Id, Pos2, Rect, Sense, Stroke, TextFormat, Ui, Vec2};

use crate::buffer::{is_word, Buffer, Edit, EditKind, Sel};
use crate::edit::{self, col_at_vcol, vcol, Cmd, EdCfg};
use crate::icons;
use crate::syntax::{Highlighter, Span};
use crate::theme::{self, Palette, TOK_BOLD, TOK_HEADING, TOK_ITALIC};

pub struct EdStyle {
    pub pal: Palette,
    pub font_size: f32,
    pub line_numbers: bool,
    pub minimap: bool,
    pub highlight_line: bool,
    pub indent_guides: bool,
    pub rulers: Vec<usize>,
    pub wrap: bool,
    pub auto_complete: bool,
    pub read_only: bool,
}

#[derive(Default)]
pub struct Find {
    pub open: bool,
    pub replace_open: bool,
    pub query: String,
    pub replacement: String,
    pub regex: bool,
    pub case: bool,
    pub word: bool,
    pub in_selection: bool,
    pub scope: Option<(usize, usize)>,
    pub matches: Vec<(usize, usize)>,
    pub error: Option<String>,
    key: (u64, String, bool, bool, bool, Option<(usize, usize)>),
    pub focus: bool,
}

impl Find {
    pub fn pattern(&self) -> Result<crate::rx::Pattern, String> {
        let mut p = if self.regex { self.query.clone() } else { crate::rx::escape(&self.query) };
        if self.word {
            p = format!(r"\b(?:{p})\b");
        }
        crate::rx::Pattern::new(&p, self.case, true)
    }

    pub fn update(&mut self, buf: &Buffer) {
        let key = (buf.version, self.query.clone(), self.regex, self.case, self.word, if self.in_selection { self.scope } else { None });
        if key == self.key {
            return;
        }
        self.key = key;
        self.matches.clear();
        self.error = None;
        if self.query.is_empty() {
            return;
        }
        let re = match self.pattern() {
            Ok(r) => r,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let text = buf.text();
        let (lo, hi) = match (self.in_selection, self.scope) {
            (true, Some((a, z))) => (buf.rope.char_to_byte(a.min(buf.len())), buf.rope.char_to_byte(z.min(buf.len()))),
            _ => (0, text.len()),
        };
        let started = Instant::now();
        let found = &mut self.matches;
        re.scan(&text[lo..hi], |start, end| {
            if start != end {
                found.push((buf.rope.byte_to_char(lo + start), buf.rope.byte_to_char(lo + end)));
            }
            found.len() < 100_000 && started.elapsed() <= Duration::from_millis(400)
        });
    }

    /// Index of the first match starting at or after `pos`.
    pub fn next_from(&self, pos: usize) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }
        let i = self.matches.partition_point(|m| m.0 < pos);
        Some(if i >= self.matches.len() { 0 } else { i })
    }
    pub fn prev_from(&self, pos: usize) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }
        let i = self.matches.partition_point(|m| m.0 < pos);
        Some(if i == 0 { self.matches.len() - 1 } else { i - 1 })
    }
}

pub struct Completion {
    pub items: Vec<(String, Option<String>)>, // (label, snippet body)
    pub selected: usize,
    pub prefix_len: usize,
}

pub struct SnippetSession {
    /// Remaining tab stops as char ranges, in visiting order; the first is the current one.
    pub stops: Vec<(usize, usize)>,
    pub len_at: usize,
}

#[derive(Clone, Copy, PartialEq)]
enum Drag {
    None,
    Char(usize),
    Word(usize, usize),
    Line(usize),
    Column(usize, usize), // (row, vcol)
    Scroll(f32),
    HScroll(f32),
    Minimap,
}

pub struct EdView {
    pub scroll: Vec2,
    pub folds: Vec<(usize, usize)>,
    pub bookmarks: BTreeSet<usize>,
    pub reveal: bool,
    pub center: bool,
    pub find: Find,
    pub completion: Option<Completion>,
    pub snippet: Option<SnippetSession>,
    pub error_line: Option<usize>,
    pub focus: bool,
    map: DisplayMap,
    drag: Drag,
    blink_from: f64,
    lines_seen: usize,
    version_seen: u64,
    max_cols: usize,
    bracket: (u64, usize, Option<(usize, usize)>),
    pub rows_visible: usize,
    pub has_focus: bool,
    velocity: f32,
    /// First visible line and row height, for scroll sync with the preview.
    pub top_line: usize,
    pub row_height: f32,
}

impl Default for EdView {
    fn default() -> Self {
        Self {
            scroll: Vec2::ZERO,
            folds: Vec::new(),
            bookmarks: BTreeSet::new(),
            reveal: false,
            center: false,
            find: Find::default(),
            completion: None,
            snippet: None,
            error_line: None,
            focus: true,
            map: DisplayMap::default(),
            drag: Drag::None,
            blink_from: 0.0,
            lines_seen: 0,
            version_seen: u64::MAX,
            max_cols: 0,
            bracket: (u64::MAX, 0, None),
            rows_visible: 30,
            has_focus: false,
            velocity: 0.0,
            top_line: 0,
            row_height: 20.0,
        }
    }
}

// ----------------------------------------------------------- display rows

#[derive(Default)]
struct DisplayMap {
    key: (u64, usize, u64, bool),
    /// Rows before each line (len = lines + 1). Empty means one row per line.
    starts: Vec<u32>,
    lines: usize,
}

fn char_cells(c: char) -> usize {
    let u = c as u32;
    if (0x1100..=0x115f).contains(&u) || (0x2e80..=0xa4cf).contains(&u) || (0xac00..=0xd7a3).contains(&u) || (0xf900..=0xfaff).contains(&u) || (0xfe30..=0xfe4f).contains(&u) || (0xff00..=0xff60).contains(&u) || (0x1f300..=0x1faff).contains(&u) {
        2
    } else {
        1
    }
}

/// Char offsets at which each visual row of a wrapped line starts.
pub fn wrap_breaks(text: &str, cols: usize, tab: usize) -> Vec<usize> {
    let mut out = vec![0usize];
    if cols < 8 {
        return out;
    }
    let mut v = 0usize;
    let mut last_space: Option<usize> = None;
    let mut row_start = 0usize;
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let w = if c == '\t' { tab - v % tab } else { char_cells(c) };
        if v + w > cols && i > row_start {
            let at = match last_space {
                Some(s) if s + 1 > row_start && s + 1 <= i => s + 1,
                _ => i,
            };
            out.push(at);
            row_start = at;
            last_space = None;
            v = 0;
            i = at;
            continue;
        }
        if c == ' ' || c == '\t' {
            last_space = Some(i);
        }
        v += w;
        i += 1;
    }
    out
}

impl DisplayMap {
    fn rebuild(&mut self, buf: &Buffer, cols: usize, wrap: bool, folds: &[(usize, usize)], tab: usize) {
        let epoch = folds.iter().fold(folds.len() as u64, |h, f| h.wrapping_mul(1_000_003).wrapping_add((f.0 as u64) << 20 ^ f.1 as u64));
        let key = (buf.version, if wrap { cols } else { 0 }, epoch, wrap);
        if key == self.key && self.lines == buf.lines() {
            return;
        }
        self.key = key;
        self.lines = buf.lines();
        self.starts.clear();
        if !wrap && folds.is_empty() {
            return;
        }
        self.starts.reserve(self.lines + 1);
        let mut total = 0u32;
        let mut hidden_until = 0usize;
        let mut fi = 0;
        let mut sorted: Vec<(usize, usize)> = folds.to_vec();
        sorted.sort();
        for (i, line) in buf.rope.lines().enumerate() {
            self.starts.push(total);
            while fi < sorted.len() && sorted[fi].0 < i {
                fi += 1;
            }
            if i < hidden_until {
                if fi < sorted.len() && sorted[fi].0 == i {
                    hidden_until = hidden_until.max(sorted[fi].1 + 1);
                }
                continue;
            }
            if fi < sorted.len() && sorted[fi].0 == i {
                hidden_until = sorted[fi].1 + 1;
            }
            total += if wrap {
                let n = line.len_chars();
                if n <= cols / 2 {
                    1
                } else {
                    let t: String = line.chars().filter(|c| *c != '\n' && *c != '\r').collect();
                    wrap_breaks(&t, cols, tab).len() as u32
                }
            } else {
                1
            };
        }
        self.starts.push(total);
    }

    fn total(&self) -> usize {
        if self.starts.is_empty() { self.lines } else { *self.starts.last().unwrap() as usize }
    }
    fn row_of_line(&self, line: usize) -> usize {
        if self.starts.is_empty() { line } else { self.starts[line.min(self.starts.len() - 1)] as usize }
    }
    fn rows_in(&self, line: usize) -> usize {
        if self.starts.is_empty() { 1 } else { (self.starts[(line + 1).min(self.starts.len() - 1)] - self.starts[line.min(self.starts.len() - 1)]) as usize }
    }
    /// The line shown at `row`, and which of its rows that is.
    fn line_at(&self, row: usize) -> (usize, usize) {
        if self.starts.is_empty() {
            return (row.min(self.lines.saturating_sub(1)), 0);
        }
        let row = row.min(self.total().saturating_sub(1)) as u32;
        // last line whose start <= row and which has rows
        let i = self.starts.partition_point(|s| *s <= row).saturating_sub(1);
        let i = i.min(self.lines.saturating_sub(1));
        (i, (row - self.starts[i]) as usize)
    }
}

/// Lines below `line` with deeper indentation: the range a fold would hide.
pub fn fold_range(buf: &Buffer, line: usize, tab: usize) -> Option<(usize, usize)> {
    let ind = |l: usize| -> Option<usize> {
        let t = buf.rope.line(l);
        let mut v = 0;
        for c in t.chars() {
            match c {
                ' ' => v += 1,
                '\t' => v = (v / tab + 1) * tab,
                '\n' | '\r' => return None,
                _ => return Some(v),
            }
        }
        None
    };
    let base = ind(line)?;
    let mut end = line;
    let mut l = line + 1;
    let limit = (line + 20_000).min(buf.lines());
    while l < limit {
        match ind(l) {
            Some(i) if i <= base => break,
            Some(_) => end = l,
            None => {}
        }
        l += 1;
    }
    (end > line).then_some((line, end))
}

// ------------------------------------------------------------ row layout

struct RowText {
    galley: Arc<Galley>,
    ascii: bool,
    /// display column of every char boundary is `vcol`; for non-ASCII we ask the galley
    text: String,
}

fn token_format(tok: u8, size: f32, pal: &Palette) -> TextFormat {
    let mut f = TextFormat { font_id: FontId::monospace(size), color: pal.token(tok), ..Default::default() };
    match tok {
        TOK_BOLD | TOK_HEADING => f.coords = theme::weight(700.0),
        TOK_ITALIC => f.italics = true,
        _ => {}
    }
    f
}

fn layout_row(ui: &Ui, text: &str, byte0: usize, spans: Option<&[Span]>, tab: usize, size: f32, pal: &Palette) -> RowText {
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    job.break_on_newline = false;
    let mut seg = String::new();
    let mut seg_tok = 0u8;
    let mut si = 0usize;
    let mut v = 0usize;
    let mut ascii = true;
    for (bi, c) in text.char_indices() {
        let abs = byte0 + bi;
        let tok = match spans {
            Some(sp) => {
                while si < sp.len() && (sp[si].0 as usize) <= abs {
                    si += 1;
                }
                sp.get(si).map(|s| s.1).unwrap_or(0)
            }
            None => 0,
        };
        if tok != seg_tok && !seg.is_empty() {
            job.append(&seg, 0.0, token_format(seg_tok, size, pal));
            seg.clear();
        }
        seg_tok = tok;
        if c == '\t' {
            let n = tab - v % tab;
            for _ in 0..n {
                seg.push(' ');
            }
            v += n;
        } else {
            if !c.is_ascii() {
                ascii = false;
            }
            seg.push(c);
            v += 1;
        }
    }
    if !seg.is_empty() {
        job.append(&seg, 0.0, token_format(seg_tok, size, pal));
    }
    let display = job.text.clone();
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    RowText { galley, ascii, text: display }
}

impl RowText {
    fn x_of(&self, vc: usize, cw: f32) -> f32 {
        if self.ascii {
            vc as f32 * cw
        } else {
            let n = self.text.chars().count();
            if vc >= n {
                self.galley.size().x + (vc - n) as f32 * cw
            } else {
                self.galley.pos_from_cursor(CCursor::new(vc)).min.x
            }
        }
    }
    fn vcol_at(&self, x: f32, cw: f32) -> usize {
        if self.ascii {
            (x / cw + 0.5).max(0.0) as usize
        } else {
            self.galley.cursor_from_pos(vec2(x.max(0.0), 1.0)).index.0
        }
    }
}

struct Geo {
    text: Rect,
    gutter: Rect,
    minimap: Option<Rect>,
    cw: f32,
    rh: f32,
    cols: usize,
}

// ------------------------------------------------------------------ show

pub struct EdOutput {
    pub changed: bool,
    pub focused: bool,
    /// A command chosen from the context menu, for the app to run.
    pub action: Option<&'static str>,
}

static PANE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Which split pane is being drawn (or has focus, outside drawing).
pub fn set_pane(p: u8) {
    PANE.store(p, std::sync::atomic::Ordering::Relaxed);
}
pub fn pane() -> u8 {
    PANE.load(std::sync::atomic::Ordering::Relaxed)
}
pub fn editor_id_of(p: u8) -> Id {
    Id::new(("ink-code-editor", p))
}
/// Keyboard-focus id of the editor in the current pane.
pub fn editor_id() -> Id {
    editor_id_of(pane())
}

fn snippet_stops(body: &str, indent: &str, unit: &str) -> (String, Vec<(usize, usize, usize)>) {
    // returns the expanded text and (index, start, end) of each tab stop, char offsets
    let body = body.replace('\t', unit).replace('\n', &format!("\n{indent}"));
    let chars: Vec<char> = body.chars().collect();
    let mut out = String::new();
    let mut stops = Vec::new();
    let mut n = 0usize;
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$' && i + 1 < chars.len() {
            if chars[i + 1].is_ascii_digit() {
                let idx = chars[i + 1].to_digit(10).unwrap() as usize;
                stops.push((idx, n, n));
                i += 2;
                continue;
            }
            if chars[i + 1] == '{' {
                if let Some(close) = chars[i..].iter().position(|c| *c == '}') {
                    let inner: String = chars[i + 2..i + close].iter().collect();
                    if let Some((idx, default)) = inner.split_once(':').and_then(|(a, d)| a.parse::<usize>().ok().map(|a| (a, d.to_string()))) {
                        let len = default.chars().count();
                        stops.push((idx, n, n + len));
                        out.push_str(&default);
                        n += len;
                        i += close + 1;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        n += 1;
        i += 1;
    }
    stops.sort_by_key(|s| if s.0 == 0 { usize::MAX } else { s.0 });
    (out, stops)
}

impl EdView {
    fn touch(&mut self, now: f64) {
        self.blink_from = now;
        self.reveal = true;
    }

    /// Keep folds, bookmarks and snippet stops aligned after the text changed.
    pub fn sync(&mut self, buf: &mut Buffer, hl: &mut Highlighter) {
        let dirty = buf.take_dirty_line();
        if let Some(line) = dirty {
            hl.invalidate(line);
        }
        if self.version_seen == buf.version {
            return;
        }
        let delta = buf.lines() as isize - self.lines_seen as isize;
        if self.version_seen != u64::MAX {
            if let Some(line) = dirty {
                if !self.folds.is_empty() {
                    self.folds.retain(|f| !(f.0 <= line && line <= f.1) || f.0 > line);
                    for f in &mut self.folds {
                        if f.0 > line {
                            f.0 = (f.0 as isize + delta).max(0) as usize;
                            f.1 = (f.1 as isize + delta).max(0) as usize;
                        }
                    }
                    let n = buf.lines();
                    self.folds.retain(|f| f.1 < n && f.0 < f.1);
                }
                if delta != 0 && !self.bookmarks.is_empty() {
                    self.bookmarks = self.bookmarks.iter().map(|b| if *b > line { (*b as isize + delta).max(0) as usize } else { *b }).collect();
                }
            }
            self.error_line = None;
        }
        if let Some(s) = &mut self.snippet {
            let d = buf.len() as isize - s.len_at as isize;
            s.len_at = buf.len();
            if let Some(cur) = s.stops.first().copied() {
                let head = buf.primary().head;
                if head + 1 < cur.0 || (head as isize) > cur.1 as isize + d.max(0) + 1 {
                    self.snippet = None;
                } else {
                    for (i, st) in s.stops.iter_mut().enumerate() {
                        if i == 0 {
                            st.1 = (st.1 as isize + d).max(st.0 as isize) as usize;
                        } else if st.0 >= cur.1 {
                            st.0 = (st.0 as isize + d).max(0) as usize;
                            st.1 = (st.1 as isize + d).max(0) as usize;
                        }
                    }
                }
            }
        }
        self.lines_seen = buf.lines();
        self.version_seen = buf.version;
    }

    pub fn scroll_to_line(&mut self, line: usize) {
        self.scroll.y = self.map.row_of_line(line.min(self.map.lines.saturating_sub(1))) as f32 * self.row_height;
    }

    pub fn unfold_at(&mut self, line: usize) {
        self.folds.retain(|f| !(f.0 < line && line <= f.1));
    }

    pub fn toggle_fold(&mut self, buf: &Buffer, line: usize, tab: usize) {
        if let Some(i) = self.folds.iter().position(|f| f.0 == line) {
            self.folds.remove(i);
        } else if let Some(r) = fold_range(buf, line, tab) {
            self.folds.push(r);
        }
    }

    /// Scroll so that `line` is centred (used by go-to-line and search results).
    pub fn goto(&mut self, buf: &mut Buffer, line: usize, col: usize) {
        let p = buf.pos_of(line, col);
        buf.set_caret(p);
        self.unfold_at(line);
        self.reveal = true;
        self.center = true;
    }

    pub fn start_snippet(&mut self, buf: &mut Buffer, body: &str, replace_back: usize, cfg: &EdCfg) {
        let s = buf.primary();
        let a = s.start().saturating_sub(replace_back);
        let line = buf.line_of(a);
        let lt = buf.line_text(line);
        let indent: String = lt.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        let (text, stops) = snippet_stops(body, &indent, &cfg.unit());
        buf.edit(vec![Edit::new(a, s.end(), text.clone())], EditKind::Other);
        let abs: Vec<(usize, usize)> = stops.iter().map(|(_, x, y)| (a + x, a + y)).collect();
        match abs.first() {
            Some(first) => {
                buf.set_sel(first.0, first.1);
                self.snippet = (abs.len() > 1).then(|| SnippetSession { stops: abs, len_at: buf.len() });
            }
            None => buf.set_caret(a + text.chars().count()),
        }
        buf.commit_sels();
        self.version_seen = buf.version; // stops are already in final coordinates
        self.lines_seen = buf.lines();
        if let Some(l) = buf.take_dirty_line() {
            // re-queue for the highlighter
            let _ = l;
        }
    }

    fn next_stop(&mut self, buf: &mut Buffer) -> bool {
        let Some(s) = &mut self.snippet else { return false };
        if s.stops.len() < 2 {
            self.snippet = None;
            return false;
        }
        s.stops.remove(0);
        let (a, z) = s.stops[0];
        buf.set_sel(a.min(buf.len()), z.min(buf.len()));
        if s.stops.len() == 1 {
            self.snippet = None;
        }
        true
    }

    fn update_completion(&mut self, buf: &Buffer, snippets: &[(String, String)], explicit: bool) {
        self.completion = None;
        if buf.sels.len() != 1 || !buf.primary().is_empty() {
            return;
        }
        let head = buf.primary().head;
        let mut a = head;
        while a > 0 && is_word(buf.rope.char(a - 1)) {
            a -= 1;
        }
        let prefix = buf.slice(a, head);
        let n = prefix.chars().count();
        if (n < 2 && !explicit) || prefix.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return;
        }
        if buf.char_at(head).is_some_and(is_word) {
            return;
        }
        let lower = prefix.to_lowercase();
        let mut items: Vec<(String, Option<String>)> = Vec::new();
        for (trigger, body) in snippets {
            if trigger.to_lowercase().starts_with(&lower) && (n > 0 || explicit) {
                items.push((trigger.clone(), Some(body.clone())));
            }
        }
        // words from the neighbourhood of the caret, nearest first
        let line = buf.line_of(head);
        let (l0, l1) = (line.saturating_sub(1500), (line + 1500).min(buf.lines()));
        let text = buf.slice(buf.line_start(l0), if l1 >= buf.lines() { buf.len() } else { buf.line_start(l1) });
        let mut seen = std::collections::HashSet::new();
        seen.insert(prefix.clone());
        let mut words: Vec<(usize, String)> = Vec::new();
        let caret_byte = text.char_indices().nth(head - buf.line_start(l0)).map(|x| x.0).unwrap_or(text.len());
        let mut start = None;
        for (i, c) in text.char_indices().chain(std::iter::once((text.len(), ' '))) {
            if is_word(c) {
                start.get_or_insert(i);
            } else if let Some(s) = start.take() {
                let w = &text[s..i];
                if w.len() > n && w.len() < 80 && fuzzy_prefix(w, &lower) && seen.insert(w.to_string()) {
                    words.push((s.abs_diff(caret_byte), w.to_string()));
                }
            }
        }
        words.sort_by(|x, y| {
            let (px, py) = (x.1.to_lowercase().starts_with(&lower), y.1.to_lowercase().starts_with(&lower));
            py.cmp(&px).then(x.0.cmp(&y.0))
        });
        items.extend(words.into_iter().take(40).map(|w| (w.1, None)));
        if !items.is_empty() {
            self.completion = Some(Completion { items, selected: 0, prefix_len: n });
        }
    }

    fn accept_completion(&mut self, buf: &mut Buffer, cfg: &EdCfg) -> bool {
        let Some(c) = self.completion.take() else { return false };
        let Some((label, body)) = c.items.get(c.selected).cloned() else { return false };
        match body {
            Some(b) => self.start_snippet(buf, &b, c.prefix_len, cfg),
            None => {
                let h = buf.primary().head;
                buf.edit(vec![Edit::new(h - c.prefix_len, h, label.clone())], EditKind::Other);
                buf.set_caret(h - c.prefix_len + label.chars().count());
                buf.commit_sels();
            }
        }
        true
    }

    /// Run a named editor command. Returns `None` if the name is not an editor command.
    pub fn command(&mut self, name: &str, buf: &mut Buffer, cfg: &EdCfg, snippets: &[(String, String)], now: f64) -> Option<bool> {
        buf.now = now;
        let page = self.rows_visible.saturating_sub(2).max(1);
        let cmd = match name {
            "move_left" => Cmd::Left(false),
            "move_right" => Cmd::Right(false),
            "move_up" | "move_down" | "select_up" | "select_down" => {
                if let (Some(c), false) = (&mut self.completion, name.starts_with("select")) {
                    let n = c.items.len();
                    c.selected = if name == "move_up" { (c.selected + n - 1) % n } else { (c.selected + 1) % n };
                    return Some(false);
                }
                match name {
                    "move_up" => Cmd::Up(false),
                    "move_down" => Cmd::Down(false),
                    "select_up" => Cmd::Up(true),
                    _ => Cmd::Down(true),
                }
            }
            "select_left" => Cmd::Left(true),
            "select_right" => Cmd::Right(true),
            "move_word_left" => Cmd::WordLeft(false),
            "move_word_right" => Cmd::WordRight(false),
            "select_word_left" => Cmd::WordLeft(true),
            "select_word_right" => Cmd::WordRight(true),
            "move_subword_left" => Cmd::SubwordLeft(false),
            "move_subword_right" => Cmd::SubwordRight(false),
            "select_subword_left" => Cmd::SubwordLeft(true),
            "select_subword_right" => Cmd::SubwordRight(true),
            "move_to_bol" => Cmd::Home(false),
            "move_to_eol" => Cmd::End(false),
            "select_to_bol" => Cmd::Home(true),
            "select_to_eol" => Cmd::End(true),
            "move_to_bof" => Cmd::DocStart(false),
            "move_to_eof" => Cmd::DocEnd(false),
            "select_to_bof" => Cmd::DocStart(true),
            "select_to_eof" => Cmd::DocEnd(true),
            "page_up" => Cmd::PageUp(false, page),
            "page_down" => Cmd::PageDown(false, page),
            "select_page_up" => Cmd::PageUp(true, page),
            "select_page_down" => Cmd::PageDown(true, page),
            "move_to_bracket" => Cmd::MatchBracket,
            "select_all" => Cmd::SelectAll,
            "expand_selection_to_line" => Cmd::SelectLine,
            "find_under_expand" => Cmd::SelectWordOrNext,
            "find_under_expand_skip" => Cmd::SkipNext,
            "find_all_under" => Cmd::SelectAllOccurrences,
            "split_selection_into_lines" => Cmd::SplitIntoLines,
            "add_cursor_above" => Cmd::CursorAbove,
            "add_cursor_below" => Cmd::CursorBelow,
            "expand_selection_to_brackets" => Cmd::ExpandToBrackets,
            "expand_selection_to_indentation" => Cmd::ExpandToIndent,
            "expand_selection" => Cmd::ExpandSelection,
            "insert_newline" => {
                if self.accept_completion(buf, cfg) {
                    self.touch(now);
                    return Some(true);
                }
                Cmd::Newline
            }
            "left_delete" => Cmd::Backspace,
            "right_delete" => Cmd::Delete,
            "delete_word_left" => Cmd::DeleteWordLeft,
            "delete_word_right" => Cmd::DeleteWordRight,
            "delete_to_bol" => Cmd::DeleteToLineStart,
            "delete_to_eol" => Cmd::DeleteToLineEnd,
            "delete_line" => Cmd::DeleteLine,
            "duplicate_line" => Cmd::Duplicate,
            "swap_line_up" => Cmd::MoveLinesUp,
            "swap_line_down" => Cmd::MoveLinesDown,
            "join_lines" => Cmd::JoinLines,
            "indent" => Cmd::Indent,
            "unindent" => Cmd::Unindent,
            "insert_tab" => {
                if self.accept_completion(buf, cfg) {
                    self.touch(now);
                    return Some(true);
                }
                if self.next_stop(buf) {
                    self.touch(now);
                    return Some(false);
                }
                // a snippet trigger before the caret expands on Tab
                if buf.sels.len() == 1 && buf.primary().is_empty() {
                    let h = buf.primary().head;
                    let mut a = h;
                    while a > 0 && is_word(buf.rope.char(a - 1)) {
                        a -= 1;
                    }
                    let word = buf.slice(a, h);
                    if let Some((_, body)) = snippets.iter().find(|s| s.0 == word && !word.is_empty()) {
                        let body = body.clone();
                        self.start_snippet(buf, &body, h - a, cfg);
                        self.touch(now);
                        return Some(true);
                    }
                }
                Cmd::Tab
            }
            "unindent_tab" => Cmd::BackTab,
            "toggle_comment" => Cmd::ToggleComment,
            "toggle_block_comment" => Cmd::ToggleBlockComment,
            "insert_line_after" => Cmd::LineAfter,
            "insert_line_before" => Cmd::LineBefore,
            "transpose" => Cmd::Transpose,
            "upper_case" => Cmd::Upper,
            "lower_case" => Cmd::Lower,
            "title_case" => Cmd::TitleCase,
            "sort_lines" => Cmd::SortLines,
            "sort_lines_case_insensitive" => Cmd::SortLinesInsensitive,
            "reverse_lines" => Cmd::ReverseLines,
            "unique_lines" => Cmd::UniqueLines,
            "trim_trailing_white_space" => Cmd::TrimTrailing,
            "expand_tabs" => Cmd::TabsToSpaces,
            "unexpand_tabs" => Cmd::SpacesToTabs,
            "undo" | "soft_undo" => Cmd::Undo,
            "redo" => Cmd::Redo,
            "single_selection" => {
                if self.completion.take().is_some() {
                    return Some(false);
                }
                if self.snippet.take().is_some() && buf.sels.len() == 1 && buf.primary().is_empty() {
                    return Some(false);
                }
                if self.find.open && buf.sels.len() == 1 {
                    self.find.open = false;
                    return Some(false);
                }
                Cmd::SingleSel
            }
            "auto_complete" => {
                self.update_completion(buf, snippets, true);
                return Some(false);
            }
            "fold" | "unfold" | "toggle_fold" => {
                let tab = cfg.tab_width.max(1);
                let mut line = buf.line_of(buf.primary().head);
                let folded = self.folds.iter().position(|f| f.0 == line || (f.0 < line && line <= f.1));
                match (name, folded) {
                    ("unfold", Some(i)) | ("toggle_fold", Some(i)) => {
                        self.folds.remove(i);
                    }
                    ("fold", None) | ("toggle_fold", None) => {
                        // fold the block that contains the caret
                        loop {
                            if let Some(r) = fold_range(buf, line, tab) {
                                let caret = buf.line_of(buf.primary().head);
                                if r.1 >= caret {
                                    self.folds.push(r);
                                    let p = buf.line_end(r.0);
                                    if caret > r.0 {
                                        buf.set_caret(p);
                                    }
                                    break;
                                }
                            }
                            if line == 0 {
                                break;
                            }
                            line -= 1;
                        }
                    }
                    _ => {}
                }
                return Some(false);
            }
            "unfold_all" => {
                self.folds.clear();
                return Some(false);
            }
            "fold_all" | "fold_level_1" | "fold_level_2" | "fold_level_3" => {
                let tab = cfg.tab_width.max(1);
                let level = match name {
                    "fold_level_2" => 1,
                    "fold_level_3" => 2,
                    _ => 0,
                };
                self.folds.clear();
                let mut l = 0;
                while l < buf.lines().min(200_000) {
                    let t = buf.line_text(l);
                    let ind = vcol(&t, t.chars().take_while(|c| *c == ' ' || *c == '\t').count(), tab) / tab;
                    if !t.trim().is_empty() && ind == level {
                        if let Some(r) = fold_range(buf, l, tab) {
                            self.folds.push(r);
                            l = r.1 + 1;
                            continue;
                        }
                    }
                    l += 1;
                }
                buf.set_caret(buf.line_start(buf.line_of(buf.primary().head).min(self.folds.first().map(|f| f.0).unwrap_or(0))));
                return Some(false);
            }
            "toggle_bookmark" => {
                let l = buf.line_of(buf.primary().head);
                if !self.bookmarks.remove(&l) {
                    self.bookmarks.insert(l);
                }
                return Some(false);
            }
            "next_bookmark" | "prev_bookmark" => {
                let l = buf.line_of(buf.primary().head);
                let target = if name == "next_bookmark" { self.bookmarks.range(l + 1..).next().or(self.bookmarks.iter().next()).copied() } else { self.bookmarks.range(..l).next_back().or(self.bookmarks.iter().next_back()).copied() };
                if let Some(t) = target {
                    self.goto(buf, t, 0);
                }
                return Some(false);
            }
            "clear_bookmarks" => {
                self.bookmarks.clear();
                return Some(false);
            }
            "scroll_lines_up" => {
                self.scroll.y -= 20.0;
                return Some(false);
            }
            "scroll_lines_down" => {
                self.scroll.y += 20.0;
                return Some(false);
            }
            "show_at_center" => {
                self.reveal = true;
                self.center = true;
                return Some(false);
            }
            "find_next" | "find_prev" => {
                self.find_step(buf, name == "find_next");
                return Some(false);
            }
            "slurp_find_string" => {
                let s = buf.primary();
                let (a, z) = if s.is_empty() { edit::word_at(buf, s.head) } else { (s.start(), s.end()) };
                self.find.query = buf.slice(a, z);
                self.find.regex = false;
                return Some(false);
            }
            "replace_next" => {
                let changed = self.replace_one(buf);
                return Some(changed);
            }
            "replace_all" => {
                let changed = self.replace_all(buf);
                return Some(changed);
            }
            _ => return None,
        };
        let moving_only = !matches!(cmd, Cmd::Up(_) | Cmd::Down(_));
        let _ = moving_only;
        let changed = edit::exec(buf, cmd, cfg);
        self.completion = None;
        for s in buf.sels.clone() {
            self.unfold_at(buf.line_of(s.head));
        }
        self.touch(now);
        Some(changed)
    }

    pub fn find_step(&mut self, buf: &mut Buffer, forward: bool) {
        if self.find.query.is_empty() {
            let s = buf.primary();
            if !s.is_empty() {
                self.find.query = buf.slice(s.start(), s.end());
            }
        }
        self.find.update(buf);
        let s = buf.primary();
        let i = if forward { self.find.next_from(if s.is_empty() { s.head } else { s.end() }) } else { self.find.prev_from(s.start()) };
        if let Some(i) = i {
            let (a, z) = self.find.matches[i];
            buf.set_sel(a, z);
            buf.occur_word = false;
            self.unfold_at(buf.line_of(a));
            self.reveal = true;
            self.center = true;
        }
    }

    fn expand_replacement(&self, buf: &Buffer, a: usize, z: usize) -> String {
        if !self.find.regex {
            return self.find.replacement.clone();
        }
        let Ok(re) = self.find.pattern() else { return self.find.replacement.clone() };
        let text = buf.slice(a, z);
        re.expand(&text, &self.find.replacement.replace("\\n", "\n").replace("\\t", "\t")).unwrap_or_else(|| self.find.replacement.clone())
    }

    pub fn replace_one(&mut self, buf: &mut Buffer) -> bool {
        self.find.update(buf);
        let s = buf.primary();
        if self.find.matches.binary_search(&(s.start(), s.end())).is_ok() {
            let text = self.expand_replacement(buf, s.start(), s.end());
            let n = text.chars().count();
            buf.edit(vec![Edit::new(s.start(), s.end(), text)], EditKind::Other);
            buf.set_caret(s.start() + n);
            buf.commit_sels();
            self.find_step(buf, true);
            true
        } else {
            self.find_step(buf, true);
            false
        }
    }

    pub fn replace_all(&mut self, buf: &mut Buffer) -> bool {
        self.find.update(buf);
        if self.find.matches.is_empty() {
            return false;
        }
        let edits: Vec<Edit> = self.find.matches.iter().map(|(a, z)| Edit::new(*a, *z, self.expand_replacement(buf, *a, *z))).collect();
        let caret = buf.primary().head;
        buf.edit(edits, EditKind::Other);
        buf.set_caret(caret.min(buf.len()));
        buf.commit_sels();
        true
    }
}

fn fuzzy_prefix(word: &str, lower_prefix: &str) -> bool {
    // first letter must match, the rest in order
    let mut it = word.chars().flat_map(|c| c.to_lowercase());
    let mut first = true;
    for p in lower_prefix.chars() {
        if first {
            if it.next() != Some(p) {
                return false;
            }
            first = false;
            continue;
        }
        if !it.any(|c| c == p) {
            return false;
        }
    }
    true
}

fn selection_text(buf: &Buffer) -> String {
    let mut sels = buf.sels.clone();
    sels.sort_by_key(|s| s.start());
    if sels.iter().all(|s| s.is_empty()) {
        // no selection copies the whole line, like Sublime
        let mut lines: Vec<usize> = sels.iter().map(|s| buf.line_of(s.head)).collect();
        lines.dedup();
        return lines.iter().map(|l| format!("{}\n", buf.line_text(*l))).collect();
    }
    sels.iter().filter(|s| !s.is_empty()).map(|s| buf.slice(s.start(), s.end())).collect::<Vec<_>>().join("\n")
}

#[allow(clippy::too_many_arguments)]
pub fn show(ui: &mut Ui, view: &mut EdView, buf: &mut Buffer, hl: &mut Highlighter, cfg: &EdCfg, st: &EdStyle, snippets: &[(String, String)]) -> EdOutput {
    let frame_start = Instant::now();
    let pal = &st.pal;
    let now = ui.input(|i| i.time);
    buf.now = now;
    let mut out = EdOutput { changed: false, focused: false, action: None };
    let full = ui.available_rect_before_wrap();
    let id = editor_id();
    let response = ui.interact(full, id, Sense::click_and_drag());
    ui.advance_cursor_after_rect(full);
    let v0 = buf.version;
    let tab = cfg.tab_width.max(1);

    // --- metrics
    let font = FontId::monospace(st.font_size);
    let cw = ui.fonts_mut(|f| f.glyph_width(&font, 'm'));
    let rh = (st.font_size * 1.5).round();
    let digits = buf.lines().to_string().len().max(3);
    let gutter_w = if st.line_numbers { (digits as f32 + 3.2) * cw } else { cw * 1.2 };
    let minimap_w = if st.minimap && full.width() > 620.0 { 96.0 } else { 0.0 };
    let geo = {
        let gutter = Rect::from_min_size(full.min, vec2(gutter_w, full.height()));
        let text = Rect::from_min_max(pos2(full.min.x + gutter_w, full.min.y), pos2(full.max.x - minimap_w, full.max.y));
        let minimap = (minimap_w > 0.0).then(|| Rect::from_min_max(pos2(full.max.x - minimap_w, full.min.y), full.max));
        let cols = (((text.width() - cw * 2.0) / cw).floor() as usize).max(10);
        Geo { text, gutter, minimap, cw, rh, cols }
    };
    view.rows_visible = (geo.text.height() / rh) as usize;
    view.row_height = rh;

    // --- focus
    if view.focus {
        ui.memory_mut(|m| m.request_focus(id));
        view.focus = false;
    }
    let focused = ui.memory(|m| m.has_focus(id));
    view.has_focus = focused;
    out.focused = focused;
    if focused {
        ui.memory_mut(|m| m.set_focus_lock_filter(id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true }));
    }

    view.sync(buf, hl);
    hl.begin_frame();
    view.map.rebuild(buf, geo.cols, st.wrap, &view.folds, tab);

    // --- text input
    if focused && !st.read_only {
        let events = ui.input(|i| i.events.clone());
        for ev in events {
            match ev {
                Event::Text(t) if !t.is_empty() && t != "\n" && t != "\r" => {
                    edit::exec(buf, Cmd::Insert(t.clone()), cfg);
                    view.touch(now);
                    if st.auto_complete && t.chars().all(is_word) {
                        view.update_completion(buf, snippets, false);
                    } else {
                        view.completion = None;
                    }
                }
                Event::Paste(t) if !t.is_empty() => {
                    edit::exec(buf, Cmd::Paste(t), cfg);
                    view.completion = None;
                    view.touch(now);
                }
                Event::Copy => ui.ctx().copy_text(selection_text(buf)),
                Event::Cut => {
                    ui.ctx().copy_text(selection_text(buf));
                    if buf.sels.iter().all(|s| s.is_empty()) {
                        edit::exec(buf, Cmd::DeleteLine, cfg);
                    } else {
                        edit::exec(buf, Cmd::Insert(String::new()), cfg);
                    }
                    view.touch(now);
                }
                Event::Ime(egui::ImeEvent::Commit(t)) if !t.is_empty() => {
                    edit::exec(buf, Cmd::Insert(t), cfg);
                    view.touch(now);
                }
                _ => {}
            }
        }
    } else if focused {
        for ev in ui.input(|i| i.events.clone()) {
            if matches!(ev, Event::Copy) {
                ui.ctx().copy_text(selection_text(buf));
            }
        }
    }
    if buf.version != v0 {
        view.sync(buf, hl);
        view.map.rebuild(buf, geo.cols, st.wrap, &view.folds, tab);
    }

    // helpers over the display map ------------------------------------
    let line_rows = |line: usize, buf: &Buffer| -> Vec<usize> {
        if st.wrap {
            wrap_breaks(&buf.line_text(line), geo.cols, tab)
        } else {
            vec![0]
        }
    };
    // (row, vcol within row) of a position
    let row_vcol = |p: usize, buf: &Buffer, map: &DisplayMap| -> (usize, usize) {
        let (line, col) = buf.line_col(p);
        let text = buf.line_text(line);
        if st.wrap {
            let br = wrap_breaks(&text, geo.cols, tab);
            let sub = br.partition_point(|b| *b <= col).saturating_sub(1);
            let rest: String = text.chars().skip(br[sub]).collect();
            (map.row_of_line(line) + sub.min(map.rows_in(line).saturating_sub(1)), vcol(&rest, col - br[sub], tab))
        } else {
            (map.row_of_line(line), vcol(&text, col, tab))
        }
    };
    let pos_at = |row: usize, vc: usize, buf: &Buffer, map: &DisplayMap| -> usize {
        let (line, sub) = map.line_at(row);
        let text = buf.line_text(line);
        let br = line_rows(line, buf);
        let sub = sub.min(br.len() - 1);
        let end = br.get(sub + 1).copied().unwrap_or(usize::MAX);
        let rest: String = text.chars().skip(br[sub]).take(end.saturating_sub(br[sub])).collect();
        let c = col_at_vcol(&rest, vc, tab);
        // do not land on the wrap boundary of the next row
        let c = if end != usize::MAX && br[sub] + c >= end { end - br[sub] - 1 } else { c };
        buf.line_start(line) + br[sub] + c.min(rest.chars().count())
    };

    // --- scrolling
    let total_rows = view.map.total();
    let content_h = total_rows as f32 * rh + (geo.text.height() - rh * 3.0).max(0.0);
    let max_y = (content_h - geo.text.height()).max(0.0);
    let max_x = if st.wrap { 0.0 } else { ((view.max_cols + 8) as f32 * cw - geo.text.width()).max(0.0) };
    let hovered = ui.rect_contains_pointer(full);
    if hovered {
        let (delta, touch) = ui.input(|i| (i.smooth_scroll_delta, i.any_touches()));
        let _ = touch;
        if delta != Vec2::ZERO {
            view.scroll.y -= delta.y;
            view.scroll.x -= delta.x;
            view.velocity = 0.0;
        }
    }

    // --- pointer
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let mods = ui.input(|i| i.modifiers);
    let (pressed, down, released, middle_pressed, middle_down) = ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.primary_released(), i.pointer.button_pressed(egui::PointerButton::Middle), i.pointer.button_down(egui::PointerButton::Middle)));
    let touch = crate::platform::touch(ui.ctx());
    if touch {
        // a finger drag scrolls, and the page keeps moving for a moment after the finger lifts
        let dt = ui.input(|i| i.stable_dt).clamp(0.001, 0.05);
        if response.dragged() {
            let d = response.drag_delta();
            view.scroll.y -= d.y;
            view.scroll.x -= d.x;
            view.velocity = 0.55 * view.velocity + 0.45 * (-d.y / dt);
        } else if down {
            view.velocity = 0.0;
        } else if view.velocity.abs() > 12.0 {
            view.scroll.y += view.velocity * dt;
            view.velocity *= (-dt * 3.2).exp();
            ui.ctx().request_repaint();
        } else {
            view.velocity = 0.0;
        }
    }
    let scrollbar_w = 12.0;
    let vbar = Rect::from_min_max(pos2(geo.text.max.x - scrollbar_w, geo.text.min.y), geo.text.max);
    let hbar = Rect::from_min_max(pos2(geo.text.min.x, geo.text.max.y - scrollbar_w), pos2(geo.text.max.x - scrollbar_w, geo.text.max.y));
    let thumb_h = if content_h > 0.0 { (geo.text.height() * geo.text.height() / content_h).clamp(28.0, geo.text.height()) } else { geo.text.height() };
    let hit = |p: Pos2, scroll: Vec2| -> (usize, usize) {
        let row = (((p.y - geo.text.min.y + scroll.y) / rh).floor().max(0.0) as usize).min(total_rows.saturating_sub(1));
        let x = p.x - geo.text.min.x - cw + scroll.x;
        (row, (x / cw + 0.5).max(0.0) as usize)
    };
    let popup_open = egui::Popup::is_any_open(ui.ctx());
    if touch && !popup_open {
        // a tap places the caret and a double tap selects the word; a press alone does
        // nothing, so that starting a scroll on the text does not move the caret
        if let Some(p) = pointer.filter(|p| geo.text.contains(*p) && !vbar.contains(*p)) {
            if response.double_clicked() || response.clicked() {
                ui.memory_mut(|m| m.request_focus(id));
                view.completion = None;
                view.velocity = 0.0;
                let (row, vc) = hit(p, view.scroll);
                let pos = pos_at(row, vc, buf, &view.map);
                if response.double_clicked() {
                    let (a, z) = edit::word_at(buf, pos);
                    buf.set_sel(a, z);
                } else {
                    buf.set_caret(pos);
                }
                buf.occur_word = false;
                view.blink_from = now;
                view.snippet = None;
            }
        }
    }
    if let Some(p) = pointer {
        if (pressed || middle_pressed) && response.hovered() && !popup_open && !(touch && geo.text.contains(p) && !vbar.contains(p)) {
            ui.memory_mut(|m| m.request_focus(id));
            view.completion = None;
            let (row, vc) = hit(p, view.scroll);
            if geo.minimap.is_some_and(|m| m.contains(p)) {
                view.drag = Drag::Minimap;
            } else if max_y > 0.0 && vbar.contains(p) {
                let thumb_top = vbar.min.y + view.scroll.y / max_y * (vbar.height() - thumb_h);
                if p.y < thumb_top || p.y > thumb_top + thumb_h {
                    view.scroll.y = ((p.y - vbar.min.y - thumb_h / 2.0) / (vbar.height() - thumb_h).max(1.0)).clamp(0.0, 1.0) * max_y;
                    view.drag = Drag::Scroll(thumb_h / 2.0);
                } else {
                    view.drag = Drag::Scroll(p.y - thumb_top);
                }
            } else if max_x > 0.0 && hbar.contains(p) {
                view.drag = Drag::HScroll(p.x - view.scroll.x / max_x.max(1.0));
            } else if geo.gutter.contains(p) {
                let (line, _) = view.map.line_at(row);
                let fold_zone = p.x > geo.gutter.max.x - cw * 1.8;
                if fold_zone && (view.folds.iter().any(|f| f.0 == line) || fold_range(buf, line, tab).is_some()) {
                    view.toggle_fold(buf, line, tab);
                    view.drag = Drag::None;
                } else {
                    let end = if line + 1 < buf.lines() { buf.line_start(line + 1) } else { buf.len() };
                    if mods.shift {
                        let a = buf.primary().anchor;
                        buf.set_sel(a, end);
                    } else {
                        buf.set_sel(buf.line_start(line), end);
                    }
                    view.drag = Drag::Line(line);
                }
            } else if geo.text.contains(p) {
                let pos = pos_at(row, vc, buf, &view.map);
                let clicks = if response.triple_clicked() { 3 } else if response.double_clicked() { 2 } else { 1 };
                if middle_pressed || (mods.alt && !mods.command) {
                    view.drag = Drag::Column(row, vc);
                    buf.set_caret(pos);
                } else if clicks == 3 {
                    let line = buf.line_of(pos);
                    let end = if line + 1 < buf.lines() { buf.line_start(line + 1) } else { buf.len() };
                    buf.set_sel(buf.line_start(line), end);
                    view.drag = Drag::Line(line);
                } else if clicks == 2 {
                    let (a, z) = edit::word_at(buf, pos);
                    if mods.command {
                        buf.sels.push(Sel::new(a, z));
                    } else {
                        buf.set_sel(a, z);
                    }
                    buf.occur_word = true;
                    view.drag = Drag::Word(a, z);
                } else if mods.command {
                    if let Some(i) = buf.sels.iter().position(|s| s.is_empty() && s.head == pos).filter(|_| buf.sels.len() > 1) {
                        buf.sels.remove(i);
                    } else {
                        buf.sels.push(Sel::caret(pos));
                    }
                    view.drag = Drag::Char(pos);
                } else if mods.shift {
                    let a = buf.primary().anchor;
                    buf.set_sel(a, pos);
                    view.drag = Drag::Char(a);
                } else {
                    buf.set_caret(pos);
                    buf.occur_word = false;
                    view.drag = Drag::Char(pos);
                }
                view.blink_from = now;
                view.snippet = None;
                if touch {
                    // on a touch screen a drag scrolls; selection is by double tap and the handles of the OS keyboard
                    view.drag = Drag::None;
                }
            }
        } else if (down || middle_down) && view.drag != Drag::None && !pressed && !middle_pressed {
            // auto-scroll while dragging past the edges
            if !matches!(view.drag, Drag::Scroll(_) | Drag::HScroll(_) | Drag::Minimap) {
                if p.y < geo.text.min.y {
                    view.scroll.y -= ((geo.text.min.y - p.y) * 0.4).min(60.0);
                } else if p.y > geo.text.max.y {
                    view.scroll.y += ((p.y - geo.text.max.y) * 0.4).min(60.0);
                }
                view.scroll.y = view.scroll.y.clamp(0.0, max_y);
            }
            let (row, vc) = hit(p, view.scroll);
            match view.drag {
                Drag::Char(anchor) => {
                    let pos = pos_at(row, vc, buf, &view.map);
                    if let Some(last) = buf.sels.last_mut() {
                        *last = Sel::new(anchor, pos);
                    }
                    if touch {
                        // on touch screens a drag scrolls instead of selecting
                    }
                }
                Drag::Word(a, z) => {
                    let pos = pos_at(row, vc, buf, &view.map);
                    let (wa, wz) = edit::word_at(buf, pos);
                    if let Some(last) = buf.sels.last_mut() {
                        *last = if pos < a { Sel::new(z, wa) } else { Sel::new(a, wz.max(z)) };
                    }
                }
                Drag::Line(line) => {
                    let (cur, _) = view.map.line_at(row);
                    let (l0, l1) = (line.min(cur), line.max(cur));
                    let end = if l1 + 1 < buf.lines() { buf.line_start(l1 + 1) } else { buf.len() };
                    if cur < line {
                        buf.set_sel(end, buf.line_start(l0));
                    } else {
                        buf.set_sel(buf.line_start(l0), end);
                    }
                }
                Drag::Column(r0, v0c) => {
                    let (ra, rb) = (r0.min(row), r0.max(row));
                    let mut sels = Vec::new();
                    for r in ra..=rb.min(ra + 5000) {
                        let a = pos_at(r, v0c, buf, &view.map);
                        let h = pos_at(r, vc, buf, &view.map);
                        if a != h || v0c == vc || rb == ra {
                            sels.push(Sel::new(a, h));
                        }
                    }
                    if !sels.is_empty() {
                        buf.sels = sels;
                    }
                }
                Drag::Scroll(grab) => {
                    view.scroll.y = ((p.y - grab - vbar.min.y) / (vbar.height() - thumb_h).max(1.0)).clamp(0.0, 1.0) * max_y;
                }
                Drag::HScroll(_) => {
                    view.scroll.x = ((p.x - hbar.min.x) / hbar.width().max(1.0)).clamp(0.0, 1.0) * max_x;
                }
                Drag::Minimap | Drag::None => {}
            }
            ui.ctx().request_repaint();
        }
    }
    if released || (!down && !middle_down) {
        if view.drag != Drag::None && !matches!(view.drag, Drag::Scroll(_) | Drag::HScroll(_) | Drag::Minimap) {
            buf.normalize();
        }
        if !matches!(view.drag, Drag::None) && !down && !middle_down {
            view.drag = Drag::None;
        }
    }

    // --- context menu
    response.context_menu(|ui| {
        ui.set_min_width(200.0);
        let mut item = |ui: &mut Ui, icon: &str, label: &str, cmd: &'static str, key: &str| {
            let b = egui::Button::new(format!("{icon}  {label}")).shortcut_text(key);
            if ui.add(b).clicked() {
                out.action = Some(cmd);
                ui.close();
            }
        };
        item(ui, icons::SCISSORS, "Cut", "cut", "Ctrl+X");
        item(ui, icons::COPY, "Copy", "copy", "Ctrl+C");
        item(ui, icons::CLIPBOARD, "Paste", "paste", "Ctrl+V");
        ui.separator();
        item(ui, icons::TEXT_SELECT, "Select All", "select_all", "Ctrl+A");
        item(ui, icons::SEARCH, "Find…", "show_find", "Ctrl+F");
        item(ui, icons::HASH, "Go to Line…", "goto_line", "Ctrl+G");
        ui.separator();
        item(ui, icons::CODE, "Toggle Comment", "toggle_comment", "Ctrl+/");
        item(ui, icons::BOOKMARK, "Toggle Bookmark", "toggle_bookmark", "Ctrl+F2");
        item(ui, icons::CHEVRON_RIGHT, "Fold / Unfold", "toggle_fold", "Ctrl+Shift+[");
    });

    // --- reveal the caret
    if view.reveal {
        view.reveal = false;
        let head = buf.primary().head;
        let (row, vc) = row_vcol(head, buf, &view.map);
        let y = row as f32 * rh;
        let h = geo.text.height();
        if view.center {
            if y < view.scroll.y + rh || y > view.scroll.y + h - rh * 2.0 {
                view.scroll.y = y - h / 2.0 + rh;
            }
        } else if y < view.scroll.y + rh {
            view.scroll.y = y - rh;
        } else if y + rh * 2.0 > view.scroll.y + h {
            view.scroll.y = y + rh * 2.0 - h;
        }
        view.center = false;
        if !st.wrap {
            let x = vc as f32 * cw;
            let w = geo.text.width() - cw * 4.0 - scrollbar_w;
            if x < view.scroll.x + cw {
                view.scroll.x = (x - cw * 6.0).max(0.0);
            } else if x > view.scroll.x + w {
                view.scroll.x = x - w + cw * 6.0;
            }
            view.max_cols = view.max_cols.max(vc);
        }
    }
    view.scroll.y = view.scroll.y.clamp(0.0, max_y);
    view.scroll.x = view.scroll.x.clamp(0.0, ((view.max_cols + 8) as f32 * cw - geo.text.width()).max(0.0).max(if st.wrap { 0.0 } else { max_x }));
    if st.wrap {
        view.scroll.x = 0.0;
    }
    let scroll = vec2(view.scroll.x.round(), view.scroll.y.round());

    // ------------------------------------------------------------ paint
    let painter = ui.painter_at(full);
    painter.rect_filled(full, CornerRadius::ZERO, pal.bg);
    let first_row = (scroll.y / rh).floor() as usize;
    view.top_line = view.map.line_at(first_row).0;
    let last_row = (((scroll.y + geo.text.height()) / rh).ceil() as usize).min(total_rows);
    let text_x0 = geo.text.min.x + cw - scroll.x;
    let row_y = |row: usize| geo.text.min.y + row as f32 * rh - scroll.y;
    let text_clip = Rect::from_min_max(pos2(geo.text.min.x, full.min.y), geo.text.max);
    let tp = ui.painter_at(text_clip);

    let mut sels = buf.sels.clone();
    sels.sort_by_key(|s| s.start());
    let primary_line = buf.line_of(buf.primary().head);
    let single_caret = sels.len() == 1 && sels[0].is_empty();

    // bracket match (cached per caret)
    if view.bracket.0 != buf.version || view.bracket.1 != buf.primary().head {
        let h = buf.primary().head;
        let found = edit::match_bracket(buf, h).map(|m| (h, m)).or_else(|| h.checked_sub(1).and_then(|q| edit::match_bracket(buf, q).map(|m| (q, m))));
        view.bracket = (buf.version, h, found);
    }
    let brackets = view.bracket.2;

    if st.find_visible(view) {
        view.find.update(buf);
    }

    let budget = Duration::from_millis(7);
    let weak = theme::mix(pal.bg, pal.text, 0.34);
    let guide = theme::mix(pal.bg, pal.text, 0.09);
    let sel_color = if focused { pal.selection() } else { theme::mix(pal.bg, pal.text, 0.12) };
    let mut carets: Vec<Rect> = Vec::new();
    let mut ime_rect = None;

    let mut row = first_row;
    let mut prev_indent = 0usize;
    while row < last_row {
        let (line, sub) = view.map.line_at(row);
        let text = buf.line_text(line);
        let ls = buf.line_start(line);
        let n_chars = text.chars().count();
        let br = line_rows(line, buf);
        let spans = hl.spans(buf, line, budget);
        let folded = view.folds.iter().find(|f| f.0 == line).copied();
        for s in sub..br.len() {
            if row >= last_row {
                break;
            }
            let y = row_y(row);
            let c0 = br[s];
            let c1 = br.get(s + 1).copied().unwrap_or(n_chars);
            let b0 = text.char_indices().nth(c0).map(|x| x.0).unwrap_or(text.len());
            let b1 = text.char_indices().nth(c1).map(|x| x.0).unwrap_or(text.len());
            let row_str = &text[b0..b1];
            let rt = layout_row(ui, row_str, b0, spans.as_deref(), tab, st.font_size, pal);
            let vc_of = |c: usize| vcol(row_str, c.saturating_sub(c0).min(c1 - c0), tab);
            if !st.wrap {
                view.max_cols = view.max_cols.max(rt.text.chars().count());
            }

            // current line
            if st.highlight_line && line == primary_line && single_caret && focused {
                tp.rect_filled(Rect::from_min_size(pos2(geo.text.min.x, y), vec2(geo.text.width(), rh)), CornerRadius::ZERO, pal.line_hl());
            }
            if view.error_line == Some(line) {
                tp.rect_filled(Rect::from_min_size(pos2(geo.text.min.x, y), vec2(geo.text.width(), rh)), CornerRadius::ZERO, theme::with_alpha(pal.danger, 0.16));
            }
            // indent guides
            if st.indent_guides && s == 0 {
                let ind = if text.trim().is_empty() { prev_indent } else { vcol(&text, text.chars().take_while(|c| *c == ' ' || *c == '\t').count(), tab) };
                if !text.trim().is_empty() {
                    prev_indent = ind;
                }
                let mut g = tab;
                while g < ind {
                    let x = (text_x0 + g as f32 * cw).round() + 0.5;
                    tp.line_segment([pos2(x, y), pos2(x, y + rh)], Stroke::new(1.0, guide));
                    g += tab;
                }
            }
            // find matches
            if st.find_visible(view) && !view.find.matches.is_empty() {
                let (ra, rz) = (ls + c0, ls + c1);
                let mut i = view.find.matches.partition_point(|m| m.1 <= ra);
                while i < view.find.matches.len() && view.find.matches[i].0 < rz.max(ra + 1) {
                    let (a, z) = view.find.matches[i];
                    let x0 = text_x0 + rt.x_of(vc_of(a.max(ra) - ls), cw);
                    let x1 = text_x0 + rt.x_of(vc_of(z.min(rz) - ls), cw);
                    let r = Rect::from_min_max(pos2(x0, y + 1.0), pos2(x1.max(x0 + 2.0), y + rh - 1.0));
                    tp.rect_filled(r, CornerRadius::same(3), theme::with_alpha(pal.warning, 0.28));
                    tp.rect_stroke(r, CornerRadius::same(3), Stroke::new(1.0, theme::with_alpha(pal.warning, 0.7)), egui::StrokeKind::Inside);
                    i += 1;
                }
            }
            // selections
            let (ra, rz) = (ls + c0, ls + c1);
            let last_sub = s + 1 == br.len();
            let si = sels.partition_point(|x| x.end() < ra);
            for sel in sels[si..].iter().take_while(|x| x.start() <= rz) {
                if !sel.is_empty() {
                    let a = sel.start().max(ra);
                    let z = sel.end().min(rz);
                    if a <= z && (a < z || (sel.end() > rz && last_sub)) {
                        let x0 = text_x0 + rt.x_of(vc_of(a - ls), cw);
                        let mut x1 = text_x0 + rt.x_of(vc_of(z - ls), cw);
                        if sel.end() > rz {
                            x1 += if last_sub { cw * 0.6 } else { 0.0 };
                        }
                        tp.rect_filled(Rect::from_min_max(pos2(x0, y), pos2(x1, y + rh)), CornerRadius::same(3), sel_color);
                    }
                }
                let h = sel.head;
                let in_row = h >= ra && (h < rz || (h == rz && (last_sub || h == ra)));
                if in_row {
                    let x = text_x0 + rt.x_of(vc_of(h - ls), cw);
                    let r = Rect::from_min_size(pos2(x - 1.0, y + 1.0), vec2(2.0, rh - 2.0));
                    carets.push(r);
                    if sel.head == buf.primary().head {
                        ime_rect = Some(r);
                    }
                }
            }
            // matching brackets
            if let Some((a, m)) = brackets {
                for p in [a, m] {
                    if p >= ra && p < rz.max(ra + 1) && focused {
                        let x = text_x0 + rt.x_of(vc_of(p - ls), cw);
                        tp.rect_stroke(Rect::from_min_size(pos2(x, y + 1.0), vec2(cw, rh - 2.0)), CornerRadius::same(2), Stroke::new(1.0, theme::with_alpha(pal.accent, 0.8)), egui::StrokeKind::Inside);
                    }
                }
            }
            // text
            let gy = y + (rh - rt.galley.size().y) / 2.0;
            tp.galley(pos2(text_x0, gy), rt.galley.clone(), pal.text);
            if let (Some(f), true) = (folded, last_sub) {
                let x = text_x0 + rt.galley.size().x + cw;
                let label = theme::text(ui, &format!(" {} {} lines ", icons::ELLIPSIS, f.1 - f.0), st.font_size * 0.8, pal.muted, 500.0);
                let r = Rect::from_min_size(pos2(x, y + 2.0), vec2(label.size().x, rh - 4.0));
                tp.rect_filled(r, CornerRadius::same(4), pal.chrome2);
                tp.galley(pos2(x, y + (rh - label.size().y) / 2.0), label, pal.muted);
            }
            // gutter
            if s == 0 {
                let gp = ui.painter_at(geo.gutter);
                if st.line_numbers {
                    let active = line == primary_line;
                    let g = theme::text(ui, &(line + 1).to_string(), st.font_size * 0.86, if active { pal.text } else { weak }, 400.0);
                    let job_x = geo.gutter.max.x - cw * 2.0 - g.size().x;
                    gp.galley(pos2(job_x, y + (rh - g.size().y) / 2.0), g, weak);
                }
                if view.bookmarks.contains(&line) {
                    gp.circle_filled(pos2(geo.gutter.min.x + 5.0, y + rh / 2.0), 3.0, pal.accent);
                }
                let gutter_hot = pointer.is_some_and(|p| geo.gutter.contains(p));
                if folded.is_some() || (gutter_hot && fold_range(buf, line, tab).is_some()) {
                    let icon = if folded.is_some() { icons::CHEVRON_RIGHT } else { icons::CHEVRON_DOWN };
                    let g = theme::text(ui, icon, st.font_size * 0.8, if folded.is_some() { pal.accent } else { weak }, 400.0);
                    gp.galley(pos2(geo.gutter.max.x - cw * 1.5, y + (rh - g.size().y) / 2.0), g, weak);
                }
            }
            row += 1;
        }
        if sub >= br.len() {
            row += 1; // defensive: never stall
        }
    }
    if hl.pending {
        // approximate colours are on screen: look again shortly for the exact ones
        ui.ctx().request_repaint_after(Duration::from_millis(40));
    }

    // rulers
    for r in &st.rulers {
        let x = (text_x0 + *r as f32 * cw).round() + 0.5;
        tp.line_segment([pos2(x, full.min.y), pos2(x, full.max.y)], Stroke::new(1.0, guide));
    }

    // carets
    if focused {
        let phase = ((now - view.blink_from) % 1.06) < 0.56;
        if phase {
            for c in &carets {
                tp.rect_filled(*c, CornerRadius::same(1), pal.accent);
            }
        }
        ui.ctx().request_repaint_after(Duration::from_millis(280));
        if let Some(r) = ime_rect {
            ui.ctx().output_mut(|o| {
                o.ime = Some(egui::output::IMEOutput { rect: geo.text, cursor_rect: r, purpose: egui::IMEPurpose::Normal, should_interrupt_composition: false });
            });
        }
    }

    // gutter separator shadow when scrolled sideways
    if scroll.x > 0.0 {
        painter.line_segment([pos2(geo.gutter.max.x, full.min.y), pos2(geo.gutter.max.x, full.max.y)], Stroke::new(1.0, pal.border_soft));
    }

    // minimap
    if let Some(mm) = geo.minimap {
        let lh = 2.0f32;
        let lines = buf.lines();
        let mm_rows = (mm.height() / lh) as usize;
        let first_line = view.map.line_at(first_row).0;
        let vis_lines = view.rows_visible.max(1);
        let scroll_frac = if max_y > 0.0 { scroll.y / max_y } else { 0.0 };
        let mm_first = if lines > mm_rows { ((lines - mm_rows) as f32 * scroll_frac) as usize } else { 0 };
        let mp = ui.painter_at(mm);
        mp.rect_filled(mm, CornerRadius::ZERO, pal.bg);
        mp.line_segment([mm.left_top(), mm.left_bottom()], Stroke::new(1.0, pal.border_soft));
        let ink = theme::with_alpha(pal.text, 0.26);
        let scale = 1.1f32;
        for i in 0..mm_rows.min(lines.saturating_sub(mm_first)) {
            let l = mm_first + i;
            let y = mm.min.y + i as f32 * lh;
            let mut x = 0usize;
            let mut run_start: Option<usize> = None;
            for c in buf.rope.line(l).chars().take(140).chain(std::iter::once('\n')) {
                let blank = c == ' ' || c == '\t' || c == '\n' || c == '\r';
                if !blank {
                    run_start.get_or_insert(x);
                } else if let Some(s) = run_start.take() {
                    let x0 = mm.min.x + 6.0 + s as f32 * scale;
                    let x1 = (mm.min.x + 6.0 + x as f32 * scale).min(mm.max.x - 2.0);
                    if x1 > x0 {
                        mp.rect_filled(Rect::from_min_max(pos2(x0, y), pos2(x1, y + lh - 0.6)), CornerRadius::ZERO, ink);
                    }
                }
                x += if c == '\t' { tab } else { 1 };
            }
        }
        let vy = mm.min.y + (first_line.saturating_sub(mm_first)) as f32 * lh;
        let vr = Rect::from_min_size(pos2(mm.min.x, vy), vec2(mm.width(), (vis_lines as f32 * lh).max(14.0)));
        mp.rect_filled(vr, CornerRadius::ZERO, theme::with_alpha(pal.text, 0.08));
        if view.drag == Drag::Minimap {
            if let Some(p) = pointer {
                let target_line = mm_first as f32 + (p.y - mm.min.y) / lh - vis_lines as f32 / 2.0;
                let frac = (target_line / (lines.saturating_sub(vis_lines)).max(1) as f32).clamp(0.0, 1.0);
                view.scroll.y = frac * max_y;
                ui.ctx().request_repaint();
            }
        }
    }

    // scrollbars
    let bar_hot = pointer.is_some_and(|p| vbar.expand(6.0).contains(p)) || matches!(view.drag, Drag::Scroll(_));
    if max_y > 0.0 {
        let top = vbar.min.y + scroll.y / max_y * (vbar.height() - thumb_h);
        let w = if bar_hot { 8.0 } else { 5.0 };
        let r = Rect::from_min_size(pos2(vbar.max.x - w - 2.0, top + 2.0), vec2(w, thumb_h - 4.0));
        painter.rect_filled(r, CornerRadius::same(4), theme::with_alpha(pal.text, if bar_hot { 0.36 } else { 0.18 }));
    }
    if max_x > 0.0 {
        let tw = (hbar.width() * geo.text.width() / (geo.text.width() + max_x)).max(28.0);
        let left = hbar.min.x + scroll.x / max_x * (hbar.width() - tw);
        painter.rect_filled(Rect::from_min_size(pos2(left, hbar.max.y - 7.0), vec2(tw, 5.0)), CornerRadius::same(3), theme::with_alpha(pal.text, 0.18));
    }

    // cursor shape
    if let Some(p) = pointer {
        if response.hovered() && geo.text.contains(p) && !vbar.contains(p) {
            ui.ctx().set_cursor_icon(CursorIcon::Text);
        }
    }

    // completion popup
    if let (Some(c), Some(caret)) = (&mut view.completion, ime_rect) {
        let mut accept = None;
        egui::Area::new(Id::new("ink-completion")).order(egui::Order::Foreground).fixed_pos(pos2(caret.min.x - cw * c.prefix_len as f32 - 8.0, caret.max.y + 4.0)).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).inner_margin(4).show(ui, |ui| {
                ui.set_min_width(220.0);
                ui.spacing_mut().item_spacing.y = 1.0;
                let start = c.selected.saturating_sub(7);
                for (i, (label, body)) in c.items.iter().enumerate().skip(start).take(9) {
                    let sel = i == c.selected;
                    let text = if body.is_some() { format!("{}  {label}", icons::ZAP) } else { label.clone() };
                    let rt = egui::RichText::new(text).monospace().size(st.font_size * 0.92).color(if sel { pal.text } else { pal.muted });
                    let b = egui::Button::new(rt).fill(if sel { pal.accent_soft() } else { Color32::TRANSPARENT }).min_size(vec2(220.0, 0.0)).right_text(egui::RichText::new(if body.is_some() { "snippet" } else { "" }).size(10.5).color(pal.faint));
                    if ui.add(b).clicked() {
                        accept = Some(i);
                    }
                }
            });
        });
        if let Some(i) = accept {
            c.selected = i;
            view.accept_completion(buf, cfg);
            view.touch(now);
        }
    }

    if buf.version != v0 {
        out.changed = true;
        view.sync(buf, hl);
        ui.ctx().request_repaint();
    }
    out
}

impl EdStyle {
    fn find_visible(&self, view: &EdView) -> bool {
        view.find.open && !view.find.query.is_empty()
    }
}

/// The find / replace bar shown under the editor. Returns true if the text changed.
pub fn find_bar(ui: &mut Ui, view: &mut EdView, buf: &mut Buffer, pal: &Palette) -> bool {
    if !view.find.open {
        return false;
    }
    let mut changed = false;
    let mut close = false;
    egui::Frame::new().fill(pal.chrome).inner_margin(egui::Margin::symmetric(10, 7)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.x = 4.0;
        let toggle = |ui: &mut Ui, on: &mut bool, icon: &str, tip: &str| {
            let b = egui::Button::new(egui::RichText::new(icon).size(14.0).color(if *on { pal.accent } else { pal.muted })).fill(if *on { pal.accent_soft() } else { Color32::TRANSPARENT }).min_size(vec2(28.0, 26.0));
            if ui.add(b).on_hover_text(tip).clicked() {
                *on = !*on;
            }
        };
        ui.horizontal(|ui| {
            toggle(ui, &mut view.find.regex, icons::REGEX, "Regular expression (Alt+R)");
            toggle(ui, &mut view.find.case, icons::CASE_SENSITIVE, "Case sensitive (Alt+C)");
            toggle(ui, &mut view.find.word, icons::WHOLE_WORD, "Whole word (Alt+W)");
            let before = view.find.in_selection;
            toggle(ui, &mut view.find.in_selection, icons::TEXT_SELECT, "In selection");
            if view.find.in_selection && !before {
                let s = buf.primary();
                view.find.scope = (!s.is_empty()).then(|| (s.start(), s.end()));
                view.find.in_selection = view.find.scope.is_some();
            }
            let qid = Id::new("ink-find-query");
            let w = (ui.available_width() - 250.0).clamp(120.0, 520.0);
            let resp = ui.add(egui::TextEdit::singleline(&mut view.find.query).id(qid).hint_text("Find").desired_width(w).font(egui::TextStyle::Monospace));
            if view.find.focus {
                view.find.focus = false;
                resp.request_focus();
                // select the whole query so typing replaces it
                if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), qid) {
                    state.cursor.set_char_range(Some(egui::text::CCursorRange::two(CCursor::new(0), CCursor::new(view.find.query.chars().count()))));
                    state.store(ui.ctx(), qid);
                }
            }
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if resp.changed() {
                view.find.update(buf);
                // incremental: jump to the first match from the selection start
                let from = buf.primary().start();
                if let Some(i) = view.find.next_from(from) {
                    let (a, z) = view.find.matches[i];
                    buf.set_sel(a, z);
                    view.reveal = true;
                    view.center = true;
                }
            }
            if enter {
                let back = ui.input(|i| i.modifiers.shift);
                view.find_step(buf, !back);
                resp.request_focus();
            }
            if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
            view.find.update(buf);
            let count = match (&view.find.error, view.find.matches.len()) {
                (Some(e), _) => e.clone(),
                (None, 0) if view.find.query.is_empty() => String::new(),
                (None, 0) => "No results".into(),
                (None, n) => {
                    let s = buf.primary();
                    match view.find.matches.binary_search(&(s.start(), s.end())) {
                        Ok(i) => format!("{} of {n}", i + 1),
                        Err(_) => format!("{n} found"),
                    }
                }
            };
            if ui.add(egui::Button::new(icons::ARROW_UP).min_size(vec2(28.0, 26.0))).on_hover_text("Previous (Shift+F3)").clicked() {
                view.find_step(buf, false);
            }
            if ui.add(egui::Button::new(icons::ARROW_DOWN).min_size(vec2(28.0, 26.0))).on_hover_text("Next (F3)").clicked() {
                view.find_step(buf, true);
            }
            if ui.add(egui::Button::new(icons::REPLACE).min_size(vec2(28.0, 26.0))).on_hover_text("Replace (Ctrl+H)").clicked() {
                view.find.replace_open = !view.find.replace_open;
            }
            ui.label(egui::RichText::new(count).size(12.0).color(if view.find.error.is_some() || (view.find.matches.is_empty() && !view.find.query.is_empty()) { pal.danger } else { pal.muted }));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(icons::X).min_size(vec2(26.0, 26.0))).clicked() {
                    close = true;
                }
            });
        });
        if view.find.replace_open {
            ui.horizontal(|ui| {
                ui.add_space(4.0 * 28.0 + 16.0);
                let w = (ui.available_width() - 250.0).clamp(120.0, 520.0);
                let resp = ui.add(egui::TextEdit::singleline(&mut view.find.replacement).id(Id::new("ink-find-replace")).hint_text("Replace with").desired_width(w).font(egui::TextStyle::Monospace));
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    changed |= view.replace_one(buf);
                    resp.request_focus();
                }
                if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
                if ui.button("Replace").clicked() {
                    changed |= view.replace_one(buf);
                }
                if ui.button("Replace All").clicked() {
                    changed |= view.replace_all(buf);
                }
            });
        }
    });
    if close {
        view.find.open = false;
        view.focus = true;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapping() {
        assert_eq!(wrap_breaks("short", 20, 4), vec![0]);
        assert_eq!(wrap_breaks("aaaa bbbb cccc dddd", 10, 4), vec![0, 10]);
        assert_eq!(wrap_breaks("abcdefghijklmnopqrstuvwxyz", 10, 4), vec![0, 10, 20]);
    }

    #[test]
    fn display_map_folds() {
        let b = Buffer::new("a\n  b\n  c\nd\n");
        let mut m = DisplayMap::default();
        m.rebuild(&b, 80, false, &[(0, 2)], 4);
        assert_eq!(m.total(), 3);
        assert_eq!(m.line_at(0), (0, 0));
        assert_eq!(m.line_at(1), (3, 0));
        assert_eq!(fold_range(&b, 0, 4), Some((0, 2)));
        assert_eq!(fold_range(&b, 1, 4), None);
    }

    #[test]
    fn snippets_expand() {
        let (text, stops) = snippet_stops("fn ${1:name}($2) {\n\t$0\n}", "  ", "    ");
        assert_eq!(text, "fn name() {\n      \n  }");
        assert_eq!(stops[0], (1, 3, 7));
        assert_eq!(stops.last().unwrap().0, 0);
    }

    #[test]
    fn find_and_replace() {
        let mut b = Buffer::new("foo bar Foo\nfoo");
        let mut v = EdView::default();
        v.find.query = "foo".into();
        v.find.update(&b);
        assert_eq!(v.find.matches.len(), 3);
        v.find.case = true;
        v.find.update(&b);
        assert_eq!(v.find.matches.len(), 2);
        v.find.regex = true;
        v.find.query = "(f)oo".into();
        v.find.replacement = "${1}ee".into();
        assert!(v.replace_all(&mut b));
        assert_eq!(b.text(), "fee bar Foo\nfee");
    }
}
