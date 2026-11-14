//! Write mode: Word-style editing of Markdown.
//!
//! The Markdown text stays the document. Each line is drawn the way it will read, with
//! headings, emphasis, lists, check boxes and quotes, and its markup is hidden unless the
//! caret is on that line. Tables, diagrams, math and images are shown rendered and turn
//! into their source while the caret is inside them. Because positions map one to one to
//! the text, every editing command, multi-cursor, undo and find work exactly as in Code mode.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use egui::text::{CCursor, LayoutJob};
use egui::{pos2, vec2, Align, Color32, CornerRadius, Event, FontFamily, FontId, Galley, Id, Rect, Sense, Stroke, TextFormat, Ui};

use crate::app::InkApp;
use crate::assets::hash64;
use crate::buffer::{Buffer, Sel};
use crate::edit::{self, Cmd};
use crate::icons;
use crate::md::{self, Block, Draw, Laid, Lay, Top};
use crate::theme::{self, Palette};

#[derive(Clone, Copy, PartialEq, Debug)]
enum Deco {
    None,
    Bullet,
    Check(bool),
    Rule,
}

struct RLine {
    galley: Arc<Galley>,
    /// source column -> display column
    map: Vec<u32>,
    x0: f32,
    height: f32,
    gap: f32,
    deco: Deco,
    quote: u8,
    heading: u8,
    code: bool,
    /// A rendered block (table, diagram, image …) shown in place of its source lines.
    widget: Option<Rc<Laid>>,
    /// Hidden continuation line of a rendered block.
    skip: bool,
}

#[derive(Default)]
pub struct Rich {
    pub goto_line: Option<usize>,
    pub resync: bool,
    parsed: Option<u64>,
    tops: Vec<Top>,
    lines: Vec<Rc<RLine>>,
    ys: Vec<f32>,
    total: f32,
    key: (u64, u32, u64, u64),
    cache: HashMap<u64, Rc<RLine>>,
    pub scroll: f32,
    pub reveal: bool,
    drag: Option<(usize, u8)>,
    blink: f64,
    /// Vertical moves need the laid-out rows, so they are queued for `show`.
    pub nav: Vec<String>,
    goal_x: Option<f32>,
    pub focus: bool,
}

#[derive(Clone, Default, PartialEq)]
struct St {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
    mark: bool,
    link: bool,
    kbd: bool,
    sup: bool,
}

struct Seg {
    a: usize,
    b: usize,
    st: St,
    marker: bool,
}

fn find_close(c: &[char], from: usize, hi: usize, pat: &[char]) -> Option<usize> {
    let n = pat.len();
    let mut i = from;
    while i + n <= hi {
        if c[i] == '\\' {
            i += 2;
            continue;
        }
        if c[i..i + n] == *pat && (n > 1 || c.get(i + 1) != Some(&pat[0]) || pat[0] == '`') {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Split a line into styled runs and markup runs.
fn scan(c: &[char], lo: usize, hi: usize, st: &St, out: &mut Vec<Seg>) {
    let mut i = lo;
    let mut run = lo;
    let flush = |out: &mut Vec<Seg>, a: usize, b: usize| {
        if b > a {
            out.push(Seg { a, b, st: st.clone(), marker: false });
        }
    };
    let mark = |out: &mut Vec<Seg>, a: usize, b: usize, st: &St| out.push(Seg { a, b, st: st.clone(), marker: true });
    while i < hi {
        let ch = c[i];
        let next = c.get(i + 1).copied().filter(|_| i + 1 < hi);
        let pair = |m: char| ch == m && next == Some(m);
        // escapes
        if ch == '\\' && next.is_some_and(|n| n.is_ascii_punctuation()) {
            flush(out, run, i);
            mark(out, i, i + 1, st);
            out.push(Seg { a: i + 1, b: i + 2, st: st.clone(), marker: false });
            i += 2;
            run = i;
            continue;
        }
        if ch == '`' {
            let ticks = c[i..hi].iter().take_while(|x| **x == '`').count();
            let pat: Vec<char> = vec!['`'; ticks];
            if let Some(close) = find_close(c, i + ticks, hi, &pat) {
                flush(out, run, i);
                mark(out, i, i + ticks, st);
                let mut s = st.clone();
                s.code = true;
                out.push(Seg { a: i + ticks, b: close, st: s, marker: false });
                mark(out, close, close + ticks, st);
                i = close + ticks;
                run = i;
                continue;
            }
        }
        let two = if pair('*') || pair('_') {
            Some(0)
        } else if pair('~') {
            Some(1)
        } else if pair('=') {
            Some(2)
        } else {
            None
        };
        if let Some(kind) = two {
            if let Some(close) = find_close(c, i + 2, hi, &[ch, ch]).filter(|cl| *cl > i + 2) {
                flush(out, run, i);
                mark(out, i, i + 2, st);
                let mut s = st.clone();
                match kind {
                    0 => s.bold = true,
                    1 => s.strike = true,
                    _ => s.mark = true,
                }
                scan(c, i + 2, close, &s, out);
                mark(out, close, close + 2, st);
                i = close + 2;
                run = i;
                continue;
            }
        }
        if (ch == '*' || ch == '_') && next.is_some_and(|n| !n.is_whitespace()) {
            let word_before = i > lo && c[i - 1].is_alphanumeric();
            if !(ch == '_' && word_before) {
                if let Some(close) = find_close(c, i + 1, hi, &[ch]).filter(|cl| *cl > i + 1 && !c[*cl - 1].is_whitespace()) {
                    if !(ch == '_' && c.get(close + 1).is_some_and(|n| n.is_alphanumeric())) {
                        flush(out, run, i);
                        mark(out, i, i + 1, st);
                        let mut s = st.clone();
                        s.italic = true;
                        scan(c, i + 1, close, &s, out);
                        mark(out, close, close + 1, st);
                        i = close + 1;
                        run = i;
                        continue;
                    }
                }
            }
        }
        if ch == '[' || (ch == '!' && next == Some('[')) {
            let open = if ch == '!' { i + 1 } else { i };
            if let Some(close) = find_close(c, open + 1, hi, &[']']) {
                if c.get(close + 1) == Some(&'(') {
                    if let Some(end) = find_close(c, close + 2, hi, &[')']) {
                        flush(out, run, i);
                        mark(out, i, open + 1, st);
                        let mut s = st.clone();
                        s.link = true;
                        if close > open + 1 {
                            scan(c, open + 1, close, &s, out);
                        }
                        mark(out, close, end + 1, st);
                        i = end + 1;
                        run = i;
                        continue;
                    }
                } else if c.get(open + 1) == Some(&'^') && ch == '[' {
                    flush(out, run, i);
                    let mut s = st.clone();
                    s.sup = true;
                    s.link = true;
                    out.push(Seg { a: i, b: close + 1, st: s, marker: false });
                    i = close + 1;
                    run = i;
                    continue;
                }
            }
        }
        if ch == '<' {
            if let Some(close) = find_close(c, i + 1, hi, &['>']) {
                let tag: String = c[i + 1..close].iter().collect::<String>().to_lowercase();
                let name = tag.trim_start_matches('/').split_whitespace().next().unwrap_or("").to_string();
                if matches!(name.as_str(), "kbd" | "b" | "strong" | "i" | "em" | "u" | "mark" | "sub" | "sup" | "br" | "code" | "s" | "del" | "ins" | "span") {
                    flush(out, run, i);
                    mark(out, i, close + 1, st);
                    i = close + 1;
                    run = i;
                    if !tag.starts_with('/') && name != "br" {
                        let closer: Vec<char> = format!("</{name}>").chars().collect();
                        if let Some(end) = find_close(c, i, hi, &closer) {
                            let mut s = st.clone();
                            match name.as_str() {
                                "kbd" => s.kbd = true,
                                "b" | "strong" => s.bold = true,
                                "i" | "em" => s.italic = true,
                                "mark" => s.mark = true,
                                "code" => s.code = true,
                                "s" | "del" => s.strike = true,
                                "sup" => s.sup = true,
                                _ => {}
                            }
                            scan(c, i, end, &s, out);
                            mark(out, end, end + closer.len(), st);
                            i = end + closer.len();
                            run = i;
                        }
                    }
                    continue;
                }
            }
        }
        if ch == 'h' && (c[i..hi].starts_with(&['h', 't', 't', 'p', ':', '/', '/']) || c[i..hi].starts_with(&['h', 't', 't', 'p', 's', ':', '/', '/'])) && (i == lo || !c[i - 1].is_alphanumeric()) {
            let end = i + c[i..hi].iter().take_while(|x| !x.is_whitespace() && **x != ')' && **x != '>').count();
            flush(out, run, i);
            let mut s = st.clone();
            s.link = true;
            out.push(Seg { a: i, b: end, st: s, marker: false });
            i = end;
            run = i;
            continue;
        }
        i += 1;
    }
    flush(out, run, hi);
}

struct Prefix {
    len: usize,
    heading: u8,
    quote: u8,
    deco: Deco,
    indent_cols: usize,
    /// chars of the prefix that stay visible (ordered list numbers)
    keep_from: Option<usize>,
}

fn prefix(c: &[char]) -> Prefix {
    let mut p = Prefix { len: 0, heading: 0, quote: 0, deco: Deco::None, indent_cols: 0, keep_from: None };
    let mut i = 0;
    // block quotes
    loop {
        let ws = c[i..].iter().take_while(|x| **x == ' ').count();
        if ws <= 3 && c.get(i + ws) == Some(&'>') {
            p.quote += 1;
            i += ws + 1;
            if c.get(i) == Some(&' ') {
                i += 1;
            }
        } else {
            break;
        }
    }
    let ws = c[i..].iter().take_while(|x| **x == ' ' || **x == '\t').count();
    let ws_cols: usize = c[i..i + ws].iter().map(|x| if *x == '\t' { 4 } else { 1 }).sum();
    let j = i + ws;
    let rest = &c[j..];
    let hashes = rest.iter().take_while(|x| **x == '#').count();
    if (1..=6).contains(&hashes) && ws < 4 && (rest.get(hashes) == Some(&' ') || rest.len() == hashes) {
        p.heading = hashes as u8;
        p.len = (j + hashes + usize::from(rest.get(hashes) == Some(&' '))).min(c.len());
        return p;
    }
    if rest.len() >= 3 && rest.iter().all(|x| *x == '-' || *x == ' ') && rest.iter().filter(|x| **x == '-').count() >= 3 || rest.len() >= 3 && rest.iter().all(|x| *x == '*' || *x == ' ') && rest.iter().filter(|x| **x == '*').count() >= 3 {
        p.deco = Deco::Rule;
        p.len = 0;
        return p;
    }
    if matches!(rest.first(), Some('-' | '*' | '+')) && rest.get(1) == Some(&' ') {
        p.indent_cols = ws_cols;
        p.len = j + 2;
        p.deco = Deco::Bullet;
        if rest.get(2) == Some(&'[') && matches!(rest.get(3), Some(' ' | 'x' | 'X')) && rest.get(4) == Some(&']') && (rest.get(5) == Some(&' ') || rest.len() == 5) {
            p.deco = Deco::Check(rest[3] != ' ');
            p.len = (j + 6).min(c.len());
        }
        return p;
    }
    let digits = rest.iter().take_while(|x| x.is_ascii_digit()).count();
    if digits > 0 && digits < 10 && matches!(rest.get(digits), Some('.' | ')')) && rest.get(digits + 1) == Some(&' ') {
        p.indent_cols = ws_cols;
        p.len = j + digits + 2;
        p.keep_from = Some(j);
        return p;
    }
    // plain or continuation text: keep the indentation as visual indent
    p.indent_cols = ws_cols;
    p.len = j;
    p
}

fn fmt_for(st: &St, base: &TextFormat, pal: &Palette) -> TextFormat {
    let mut f = base.clone();
    let size = base.font_id.size;
    if st.bold {
        f.coords = theme::weight(700.0);
    }
    if st.italic {
        f.font_id.family = theme::italic_family();
    }
    if st.code || st.kbd {
        f.font_id = FontId::new((size * 0.88 * 4.0).round() / 4.0, FontFamily::Monospace);
        f.coords = Default::default();
        f.background = pal.doc_code;
        f.expand_bg = 2.0;
    }
    if st.mark {
        f.background = pal.mark;
        f.expand_bg = 2.0;
    }
    if st.strike {
        f.strikethrough = Stroke::new(1.0, theme::with_alpha(f.color, 0.7));
        f.color = theme::mix(f.color, pal.doc_bg, 0.35);
    }
    if st.link {
        f.color = pal.accent;
    }
    if st.sup {
        f.font_id.size = (size * 0.72 * 4.0).round() / 4.0;
        f.valign = Align::TOP;
    }
    f
}

#[derive(Clone, Copy, PartialEq, Hash)]
enum Ctx {
    Text,
    Code,
    Fence,
    Raw,
}

#[allow(clippy::too_many_arguments)]
fn layout_line(ui: &Ui, text: &str, ctx: Ctx, active: bool, width: f32, size: f32, pal: &Palette, lang: &str) -> RLine {
    let c: Vec<char> = text.chars().collect();
    let n = c.len();
    let mut job = LayoutJob::default();
    let mut map = vec![0u32; n + 1];
    let mut shown = 0u32;
    let mut line = RLine { galley: ui.fonts_mut(|f| f.layout_job(LayoutJob::default())), map: Vec::new(), x0: 0.0, height: 0.0, gap: 0.0, deco: Deco::None, quote: 0, heading: 0, code: false, widget: None, skip: false };
    let mono = |sz: f32, color: Color32| TextFormat { font_id: FontId::monospace((sz * 4.0).round() / 4.0), color, line_height: Some((sz * 1.6).round()), ..Default::default() };
    match ctx {
        Ctx::Code | Ctx::Fence | Ctx::Raw => {
            line.code = ctx != Ctx::Raw;
            let sz = size * 0.86;
            if ctx == Ctx::Code && !lang.is_empty() && n < 2000 {
                let spans = crate::syntax::highlight_snippet(lang, text);
                let mut last = 0usize;
                for (end, tok) in spans.first().map(|s| s.iter()).into_iter().flatten() {
                    let end = (*end as usize).min(text.len());
                    if end > last && text.is_char_boundary(last) && text.is_char_boundary(end) {
                        job.append(&text[last..end], 0.0, mono(sz, pal.token(*tok)));
                        last = end;
                    }
                }
            } else {
                job.append(text, 0.0, mono(sz, if ctx == Ctx::Code { pal.doc_text } else { pal.muted }));
            }
            for (i, m) in map.iter_mut().enumerate() {
                *m = i as u32;
            }
            line.x0 = if line.code { size * 0.9 } else { 0.0 };
        }
        Ctx::Text => {
            let p = prefix(&c);
            line.deco = p.deco;
            line.quote = p.quote;
            line.heading = p.heading;
            let (scale, weight) = match p.heading {
                1 => (2.05, 720.0),
                2 => (1.55, 680.0),
                3 => (1.27, 660.0),
                4 => (1.1, 650.0),
                5 => (1.0, 650.0),
                6 => (0.88, 650.0),
                _ => (1.0, 400.0),
            };
            let fs = (size * scale * 4.0).round() / 4.0;
            let color = if p.quote > 0 || p.deco == Deco::Check(true) { pal.muted } else { pal.doc_text };
            let base = TextFormat { font_id: FontId::proportional(fs), color, coords: theme::weight(weight), line_height: Some((fs * if p.heading > 0 { 1.3 } else { 1.62 }).round()), ..Default::default() };
            let faint = TextFormat { color: pal.faint, coords: theme::weight(400.0), ..base.clone() };
            line.x0 = p.quote as f32 * size * 1.1 + p.indent_cols as f32 * size * 0.5 + if matches!(p.deco, Deco::Bullet | Deco::Check(_)) { size * 1.5 } else { 0.0 };
            line.gap = match p.heading {
                1 | 2 => size * 1.1,
                3..=6 => size * 0.8,
                _ => 0.0,
            };
            // the block prefix
            if p.deco == Deco::Rule && !active {
                for m in map.iter_mut() {
                    *m = 0;
                }
                job.append(" ", 0.0, base.clone());
            } else {
                for (i, ch) in c.iter().enumerate().take(p.len) {
                    map[i] = shown;
                    let visible = active || p.keep_from.is_some_and(|k| i >= k);
                    if visible {
                        job.append(&ch.to_string(), 0.0, if p.keep_from.is_some_and(|k| i >= k) && !active { TextFormat { color: pal.muted, ..base.clone() } } else { faint.clone() });
                        shown += 1;
                    }
                }
                if active && p.len > 0 {
                    line.x0 = p.quote as f32 * size * 1.1;
                    if p.heading == 0 {
                        line.deco = Deco::None;
                    }
                }
                let mut segs = Vec::new();
                scan(&c, p.len, n, &St::default(), &mut segs);
                for s in segs {
                    for i in s.a..s.b {
                        map[i] = shown;
                        if !s.marker || active {
                            shown += 1;
                        }
                    }
                    if s.marker && !active {
                        continue;
                    }
                    let t: String = c[s.a..s.b].iter().collect();
                    job.append(&t, 0.0, if s.marker { faint.clone() } else { fmt_for(&s.st, &base, pal) });
                }
                map[n] = shown;
            }
        }
    }
    if job.sections.is_empty() {
        let h = if ctx == Ctx::Text { size * 0.75 } else { size * 0.86 * 1.6 };
        job.append("", 0.0, TextFormat { font_id: FontId::proportional(size), line_height: Some(h.round()), ..Default::default() });
    }
    job.wrap.max_width = (width - line.x0 - if line.code { size * 0.9 } else { 0.0 }).max(60.0);
    line.galley = ui.fonts_mut(|f| f.layout_job(job));
    line.height = line.galley.size().y.max(if n == 0 && ctx == Ctx::Text { size * 0.75 } else { 0.0 });
    line.map = map;
    line
}

impl RLine {
    fn dcol(&self, col: usize) -> usize {
        self.map.get(col).copied().unwrap_or_else(|| self.map.last().copied().unwrap_or(0)) as usize
    }
    /// Source column for a display column (first source position that maps to it or beyond).
    fn scol(&self, d: usize) -> usize {
        self.map.partition_point(|m| (*m as usize) < d).min(self.map.len().saturating_sub(1))
    }
    fn caret(&self, col: usize) -> Rect {
        self.galley.pos_from_cursor(CCursor::new(self.dcol(col)))
    }
}

impl Rich {
    fn rebuild(&mut self, ui: &Ui, buf: &Buffer, width: f32, epoch: u64, size: f32, pal: &Palette, lay: &mut Lay<'_>) {
        if self.parsed != Some(buf.version) {
            self.tops = md::parse(&buf.text());
            self.parsed = Some(buf.version);
        }
        // lines that hold a caret show their markup
        let mut active: Vec<usize> = buf.sels.iter().map(|s| buf.line_of(s.head)).collect();
        active.sort();
        active.dedup();
        let active_hash = hash64(&[&active]);
        let key = (buf.version, width as u32, epoch ^ lay.assets.epoch.wrapping_mul(0x9e37), active_hash);
        if key == self.key && self.lines.len() == buf.lines() {
            return;
        }
        self.key = key;
        let n = buf.lines();
        // what each line belongs to: (context, language, widget block range)
        let mut ctx = vec![(Ctx::Text, 0usize); n];
        let mut langs: Vec<String> = vec![String::new()];
        let mut widgets: Vec<(usize, usize, usize)> = Vec::new(); // (first line, last line, top index)
        for (ti, t) in self.tops.iter().enumerate() {
            let (l0, l1) = (t.line0 - 1, (t.line1 - 1).min(n.saturating_sub(1)));
            match &t.block {
                Block::Code { lang, .. } => {
                    langs.push(lang.clone());
                    let li = langs.len() - 1;
                    for (i, c) in ctx.iter_mut().enumerate().take(l1 + 1).skip(l0) {
                        *c = (if i == l0 || (i == l1 && l1 > l0) { Ctx::Fence } else { Ctx::Code }, li);
                    }
                }
                Block::Table { .. } | Block::Math(_) | Block::Diagram { .. } | Block::Image { .. } | Block::FrontMatter(_) | Block::Details { .. } | Block::Center(_) => {
                    let inside = active.iter().any(|a| *a >= l0 && *a <= l1);
                    if inside {
                        for c in ctx.iter_mut().take(l1 + 1).skip(l0) {
                            *c = (Ctx::Raw, 0);
                        }
                    } else {
                        widgets.push((l0, l1, ti));
                    }
                }
                _ => {}
            }
        }
        let mut next: HashMap<u64, Rc<RLine>> = HashMap::with_capacity(n);
        let mut lines: Vec<Rc<RLine>> = Vec::with_capacity(n);
        let mut wi = 0;
        let mut i = 0;
        while i < n {
            while wi < widgets.len() && widgets[wi].1 < i {
                wi += 1;
            }
            if wi < widgets.len() && widgets[wi].0 == i {
                let (l0, l1, ti) = widgets[wi];
                let h = hash64(&[&"w", &self.tops[ti].block, &(width as u32), &epoch, &lay.assets.epoch]);
                let first = match self.cache.get(&h).filter(|l| !l.widget.as_ref().is_some_and(|w| w.pending)) {
                    Some(l) => l.clone(),
                    None => {
                        let laid = Rc::new(md::layout_top(lay, &self.tops[ti].block, width));
                        let mut l = layout_line(ui, "", Ctx::Raw, false, width, size, pal, "");
                        l.height = laid.height;
                        l.gap = size * 0.3;
                        l.widget = Some(laid);
                        Rc::new(l)
                    }
                };
                next.insert(h, first.clone());
                lines.push(first);
                let hs = hash64(&[&"skip"]);
                let skip = self.cache.get(&hs).cloned().or_else(|| next.get(&hs).cloned()).unwrap_or_else(|| {
                    let mut l = layout_line(ui, "", Ctx::Raw, false, width, size, pal, "");
                    l.height = 0.0;
                    l.skip = true;
                    Rc::new(l)
                });
                next.insert(hs, skip.clone());
                for _ in l0 + 1..=l1 {
                    lines.push(skip.clone());
                }
                i = l1 + 1;
                continue;
            }
            let text = buf.line_text(i);
            let is_active = active.binary_search(&i).is_ok();
            let (c, li) = ctx[i];
            let h = hash64(&[&text, &c, &langs[li], &is_active, &(width as u32), &epoch, &((size * 8.0) as u32)]);
            let l = match self.cache.get(&h).or_else(|| next.get(&h)) {
                Some(l) => l.clone(),
                None => Rc::new(layout_line(ui, &text, c, is_active, width, size, pal, &langs[li])),
            };
            next.insert(h, l.clone());
            lines.push(l);
            i += 1;
        }
        self.cache = next;
        self.ys.clear();
        self.ys.reserve(n + 1);
        let mut y = 0.0;
        for l in &lines {
            y += l.gap;
            self.ys.push(y);
            y += l.height + if l.skip { 0.0 } else { size * 0.18 };
        }
        self.total = y;
        self.lines = lines;
    }

    fn line_at_y(&self, y: f32) -> usize {
        let i = self.ys.partition_point(|v| *v <= y).saturating_sub(1);
        let mut i = i.min(self.lines.len().saturating_sub(1));
        while i > 0 && self.lines[i].skip {
            i -= 1;
        }
        i
    }

    fn hit(&self, buf: &Buffer, p: egui::Vec2) -> usize {
        if self.lines.is_empty() {
            return 0;
        }
        let li = self.line_at_y(p.y);
        let l = &self.lines[li];
        if l.widget.is_some() {
            return buf.line_start(li);
        }
        let local = vec2(p.x - l.x0, (p.y - self.ys[li]).clamp(0.0, (l.height - 1.0).max(0.0)));
        let d = l.galley.cursor_from_pos(local).index.0;
        buf.line_start(li) + l.scol(d).min(buf.line_len(li))
    }

    fn caret_rect(&self, buf: &Buffer, pos: usize) -> Rect {
        let (li, col) = buf.line_col(pos);
        match self.lines.get(li) {
            Some(l) => l.caret(col).translate(vec2(l.x0, self.ys[li])),
            None => Rect::ZERO,
        }
    }

    /// Move the caret one visual row up or down, keeping its x position.
    fn vertical(&mut self, buf: &mut Buffer, down: bool, extend: bool, rows: usize) {
        let mut sels = buf.sels.clone();
        for s in &mut sels {
            let mut pos = s.head;
            let x = self.goal_x.unwrap_or_else(|| self.caret_rect(buf, pos).min.x);
            for _ in 0..rows {
                let r = self.caret_rect(buf, pos);
                let (li, _) = buf.line_col(pos);
                let l = &self.lines[li];
                let within = if down { r.max.y + 2.0 < self.ys[li] + l.galley.size().y } else { r.min.y - 2.0 > self.ys[li] };
                let target_y = if within {
                    if down { r.max.y + 2.0 } else { r.min.y - 2.0 }
                } else {
                    // step to the neighbouring line that is not a hidden continuation
                    let mut t = li as isize + if down { 1 } else { -1 };
                    while t >= 0 && (t as usize) < self.lines.len() && self.lines[t as usize].skip {
                        t += if down { 1 } else { -1 };
                    }
                    if t < 0 {
                        pos = 0;
                        break;
                    }
                    if t as usize >= self.lines.len() {
                        pos = buf.len();
                        break;
                    }
                    let t = t as usize;
                    if down { self.ys[t] + 1.0 } else { self.ys[t] + self.lines[t].height - 2.0 }
                };
                pos = self.hit(buf, vec2(x, target_y));
            }
            self.goal_x = Some(x);
            *s = Sel { anchor: if extend { s.anchor } else { pos }, head: pos, goal: None };
        }
        buf.sels = sels;
        buf.normalize();
    }
}

fn paint_laid(ui: &Ui, laid: &Laid, off: egui::Vec2, pal: &Palette, assets: &mut crate::assets::Assets) -> bool {
    let mut stale = false;
    for d in &laid.items {
        match d {
            Draw::Rect { rect, fill, stroke, radius } => {
                let r = rect.translate(off);
                if *fill != Color32::TRANSPARENT {
                    ui.painter().rect_filled(r, CornerRadius::same(*radius), *fill);
                }
                if *stroke != Stroke::NONE {
                    ui.painter().rect_stroke(r, CornerRadius::same(*radius), *stroke, egui::StrokeKind::Inside);
                }
            }
            Draw::Line { a, b, stroke } => {
                ui.painter().line_segment([*a + off, *b + off], *stroke);
            }
            Draw::Text { pos, galley, .. } => {
                ui.painter().galley(*pos + off, galley.clone(), pal.doc_text);
            }
            Draw::Image { rect, key, radius, .. } => match assets.peek(*key) {
                Some(a) => {
                    let r = rect.translate(off);
                    egui::Image::from_texture((a.tex.id(), r.size())).corner_radius(CornerRadius::same(*radius)).paint_at(ui, r);
                }
                None => stale = true,
            },
            _ => {}
        }
    }
    stale
}

fn ribbon(ui: &mut Ui, app: &mut InkApp) {
    let pal = app.pal;
    let heading = app.docs.get(app.active).map(|d| {
        let t = d.buf.line_text(d.buf.line_of(d.buf.primary().head));
        let h = t.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&h) && t.chars().nth(h) == Some(' ') { h } else { 0 }
    });
    let cmds = std::cell::RefCell::new(Vec::<String>::new());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        let avail = ui.available_width();
        let btn = |ui: &mut Ui, icon: &str, tip: &str, cmd: &str| {
            let b = egui::Button::new(egui::RichText::new(icon).size(15.0).color(pal.muted)).frame_when_inactive(false).min_size(vec2(30.0, 28.0)).corner_radius(6);
            if ui.add(b).on_hover_text(tip).clicked() {
                cmds.borrow_mut().push(cmd.to_string());
            }
        };
        let sep = |ui: &mut Ui| {
            ui.add_space(5.0);
            let (r, _) = ui.allocate_exact_size(vec2(1.0, 18.0), Sense::hover());
            ui.painter().rect_filled(r, 0, pal.border_soft);
            ui.add_space(5.0);
        };
        btn(ui, icons::UNDO_2, "Undo (Ctrl+Z)", "undo");
        btn(ui, icons::REDO_2, "Redo (Ctrl+Y)", "redo");
        sep(ui);
        let label = match heading {
            Some(0) | None => "Paragraph".to_string(),
            Some(h) => format!("Heading {h}"),
        };
        let r = ui.add(egui::Button::new(egui::RichText::new(format!("{label}  {}", icons::CHEVRON_DOWN)).size(12.5)).min_size(vec2(112.0, 28.0)).corner_radius(6));
        egui::Popup::menu(&r).show(|ui| {
            ui.set_min_width(170.0);
            for (name, cmd, size) in [("Paragraph", "md_paragraph", 13.5), ("Heading 1", "md_h1", 20.0), ("Heading 2", "md_h2", 17.0), ("Heading 3", "md_h3", 15.0), ("Quote", "md_quote", 13.5), ("Code block", "md_codeblock", 13.5)] {
                if ui.add(egui::Button::new(egui::RichText::new(name).size(size).strong()).min_size(vec2(170.0, 0.0))).clicked() {
                    cmds.borrow_mut().push(cmd.to_string());
                    ui.close();
                }
            }
        });
        sep(ui);
        btn(ui, icons::BOLD, "Bold (Ctrl+B)", "md_bold");
        btn(ui, icons::ITALIC, "Italic (Ctrl+I)", "md_italic");
        btn(ui, icons::STRIKETHROUGH, "Strikethrough", "md_strike");
        btn(ui, icons::HIGHLIGHTER, "Highlight", "md_highlight");
        btn(ui, icons::CODE, "Inline code", "md_code");
        if avail > 620.0 {
            sep(ui);
            btn(ui, icons::LIST, "Bullet list", "md_ul");
            btn(ui, icons::LIST_ORDERED, "Numbered list", "md_ol");
            btn(ui, icons::LIST_CHECKS, "Check list", "md_task");
            btn(ui, icons::QUOTE, "Quote", "md_quote");
        }
        if avail > 800.0 {
            sep(ui);
            btn(ui, icons::LINK, "Link", "md_link");
            btn(ui, icons::IMAGE, "Image", "md_image");
            btn(ui, icons::TABLE, "Table", "md_table");
            btn(ui, icons::MINUS, "Divider", "md_hr");
        }
        if avail > 960.0 {
            sep(ui);
            btn(ui, icons::SIGMA, "Equation", "md_math");
            btn(ui, icons::WORKFLOW, "Diagram", "md_mermaid");
            btn(ui, icons::CHART_COLUMN, "Chart", "md_chart");
            btn(ui, icons::INFO, "Callout", "md_note");
        }
    });
    app.pending.extend(cmds.into_inner());
}

pub fn show(ui: &mut Ui, app: &mut InkApp) {
    let pal = app.pal;
    let now = ui.input(|i| i.time);
    let full_outer = ui.available_rect_before_wrap();
    ui.painter().rect_filled(full_outer, CornerRadius::ZERO, pal.chrome);
    egui::Panel::top(("rich-ribbon", crate::editor::pane())).frame(egui::Frame::new().fill(pal.chrome).inner_margin(egui::Margin::symmetric(10, 5))).show_separator_line(false).show(ui, |ui| ribbon(ui, app));
    let find_open = app.docs.get(app.active).is_some_and(|d| d.ed.find.open);
    if find_open {
        let d = &mut app.docs[app.active];
        egui::Panel::bottom(("rich-find", crate::editor::pane())).frame(egui::Frame::new()).show(ui, |ui| {
            if crate::editor::find_bar(ui, &mut d.ed, &mut d.buf, &pal) {
                d.edited_at = Some(now);
            }
        });
    }
    let size = app.set.document_font_size;
    let doc_width = app.set.document_width;
    let epoch = app.pal_epoch;
    let Some(d) = app.docs.get_mut(app.active) else { return };
    let base = d.dir();
    let rich = d.view.rich.get_or_insert_with(Default::default);
    let buf = &mut d.buf;
    let cfg = d.cfg.clone();
    buf.now = now;
    let mut toggles: Vec<usize> = Vec::new();
    let mut edited = false;
    let assets = &mut app.assets;
    egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| {
        let full = ui.available_rect_before_wrap();
        let narrow = full.width() < 560.0;
        let margin = if narrow { 18.0 } else { 44.0 };
        let column = (full.width() - margin * 2.0).min(doc_width).max(160.0);
        let paged = full.width() > column + margin * 2.0 + 140.0;
        ui.painter().rect_filled(full, CornerRadius::ZERO, if paged { pal.desk } else { pal.doc_bg });
        let id = crate::editor::editor_id();
        let (top_pad, page_pad) = (if paged { 28.0 } else { 22.0 }, if paged { 56.0 } else { 0.0 });

        // focus
        if d.ed.focus || rich.focus {
            ui.memory_mut(|m| m.request_focus(id));
            d.ed.focus = false;
            rich.focus = false;
        }
        let focused = ui.memory(|m| m.has_focus(id));
        d.ed.has_focus = focused;
        if focused {
            ui.memory_mut(|m| m.set_focus_lock_filter(id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true }));
        }
        let v0 = buf.version;
        let sel0 = buf.sels.clone();

        // text input
        if focused && !d.read_only {
            for ev in ui.input(|i| i.events.clone()) {
                match ev {
                    Event::Text(t) if !t.is_empty() && t != "\n" && t != "\r" => {
                        edit::exec(buf, Cmd::Insert(t), &cfg);
                    }
                    Event::Paste(t) if !t.is_empty() => {
                        edit::exec(buf, Cmd::Paste(t), &cfg);
                    }
                    Event::Copy | Event::Cut => {
                        let mut sels = buf.sels.clone();
                        sels.sort_by_key(|s| s.start());
                        let text = sels.iter().filter(|s| !s.is_empty()).map(|s| buf.slice(s.start(), s.end())).collect::<Vec<_>>().join("\n");
                        if !text.is_empty() {
                            ui.ctx().copy_text(text);
                            if matches!(ev, Event::Cut) {
                                edit::exec(buf, Cmd::Insert(String::new()), &cfg);
                            }
                        }
                    }
                    Event::Ime(egui::ImeEvent::Commit(t)) if !t.is_empty() => {
                        edit::exec(buf, Cmd::Insert(t), &cfg);
                    }
                    _ => {}
                }
            }
        }
        d.ed.sync(buf, &mut d.hl);

        let ppp = ui.ctx().pixels_per_point();
        let open = std::collections::HashSet::new();
        {
            let mut lay = Lay { ui, pal: &pal, size, assets: &mut *assets, base: base.clone(), ppp, open: &open, pending: false, anchors: Vec::new(), column };
            rich.rebuild(ui, buf, column, epoch, size, &pal, &mut lay);
        }

        // queued vertical movement
        let nav = std::mem::take(&mut rich.nav);
        let page = ((full.height() / (size * 1.62)) as usize).saturating_sub(2).max(1);
        for cmd in &nav {
            let extend = cmd.starts_with("select");
            let (down, rows) = match cmd.as_str() {
                "move_up" | "select_up" => (false, 1),
                "move_down" | "select_down" => (true, 1),
                "page_up" | "select_page_up" => (false, page),
                _ => (true, page),
            };
            let goal = rich.goal_x;
            rich.vertical(buf, down, extend, rows);
            if goal.is_none() && rows == 1 {
                // keep the column for the next consecutive vertical move
            }
        }
        if nav.is_empty() && (buf.sels != sel0 || buf.version != v0) {
            rich.goal_x = None;
        }
        if buf.sels != sel0 || buf.version != v0 || d.ed.reveal {
            rich.reveal = true;
            rich.blink = now;
            d.ed.reveal = false;
            let mut lay = Lay { ui, pal: &pal, size, assets: &mut *assets, base: base.clone(), ppp, open: &open, pending: false, anchors: Vec::new(), column };
            rich.rebuild(ui, buf, column, epoch, size, &pal, &mut lay);
        }
        if let Some(l) = rich.goto_line.take() {
            buf.set_caret(buf.line_start(l.min(buf.lines().saturating_sub(1))));
            rich.reveal = true;
            let mut lay = Lay { ui, pal: &pal, size, assets: &mut *assets, base: base.clone(), ppp, open: &open, pending: false, anchors: Vec::new(), column };
            rich.rebuild(ui, buf, column, epoch, size, &pal, &mut lay);
        }

        let content_h = rich.total + top_pad * 2.0 + page_pad * 2.0 + full.height() * 0.4;
        let mut area = egui::ScrollArea::vertical().id_salt(("rich", d.id)).auto_shrink([false, false]);
        if rich.reveal {
            rich.reveal = false;
            let r = rich.caret_rect(buf, buf.primary().head);
            let (y0, y1) = (r.min.y + top_pad + page_pad, r.max.y + top_pad + page_pad);
            let view_h = full.height();
            if y0 < rich.scroll + 30.0 {
                area = area.vertical_scroll_offset((y0 - 60.0).max(0.0));
            } else if y1 > rich.scroll + view_h - 40.0 {
                area = area.vertical_scroll_offset(y1 - view_h + 90.0);
            }
        }
        let mut stale = false;
        let out = area.show_viewport(ui, |ui, viewport| {
            ui.set_width(full.width());
            ui.set_height(content_h);
            let origin_y = ui.min_rect().min.y;
            let page_w = column + if paged { 128.0 } else { 0.0 };
            let col_x = (full.min.x + (full.width() - column) / 2.0).round();
            if paged {
                let page = Rect::from_min_size(pos2(full.min.x + (full.width() - page_w) / 2.0, origin_y + top_pad), vec2(page_w, (rich.total + page_pad * 2.0).max(full.height() - top_pad * 2.0)));
                let shadow = egui::Shadow { offset: [0, 6], blur: 28, spread: 0, color: Color32::from_black_alpha(if pal.dark { 110 } else { 28 }) };
                ui.painter().add(shadow.as_shape(page, CornerRadius::same(10)));
                ui.painter().rect_filled(page, CornerRadius::same(10), pal.doc_bg);
                ui.painter().rect_stroke(page, CornerRadius::same(10), Stroke::new(1.0, pal.border_soft), egui::StrokeKind::Outside);
            }
            let doc = vec2(col_x, origin_y + top_pad + page_pad);
            let area_rect = Rect::from_min_size(pos2(full.min.x, origin_y), vec2(full.width(), content_h));
            // touch drags belong to the scroll area; with a mouse they select text
            let touch = ui.input(|i| i.any_touches());
            let resp = ui.interact(area_rect, id, if touch { Sense::click() } else { Sense::click_and_drag() });

            // pointer
            let pointer = ui.input(|i| i.pointer.interact_pos());
            let (pressed, down, mods) = ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down(), i.modifiers));
            if let Some(p) = pointer {
                let local = (p - doc).to_vec2();
                if pressed && resp.hovered() && !egui::Popup::is_any_open(ui.ctx()) {
                    ui.memory_mut(|m| m.request_focus(id));
                    let li = rich.line_at_y(local.y);
                    let l = rich.lines.get(li).cloned();
                    // check boxes toggle instead of placing the caret
                    let on_check = l.as_ref().is_some_and(|l| matches!(l.deco, Deco::Check(_)) && local.x >= l.x0 - size * 1.6 && local.x < l.x0 - size * 0.2);
                    if on_check {
                        toggles.push(li);
                    } else {
                        let pos = rich.hit(buf, local);
                        let clicks = if resp.triple_clicked() { 3 } else if resp.double_clicked() { 2 } else { 1 };
                        if clicks == 3 {
                            let line = buf.line_of(pos);
                            buf.set_sel(buf.line_start(line), buf.line_end(line));
                        } else if clicks == 2 {
                            let (a, z) = edit::word_at(buf, pos);
                            buf.set_sel(a, z);
                        } else if mods.command {
                            buf.sels.push(Sel::caret(pos));
                        } else if mods.shift {
                            let a = buf.primary().anchor;
                            buf.set_sel(a, pos);
                        } else {
                            buf.set_caret(pos);
                        }
                        rich.drag = Some((buf.primary().anchor, clicks));
                        rich.blink = now;
                        rich.goal_x = None;
                    }
                } else if down && !pressed {
                    if let Some((anchor, clicks)) = rich.drag {
                        let pos = rich.hit(buf, local);
                        let head = if clicks == 2 {
                            let (a, z) = edit::word_at(buf, pos);
                            if pos < anchor { a } else { z }
                        } else {
                            pos
                        };
                        if let Some(last) = buf.sels.last_mut() {
                            *last = Sel::new(anchor, head);
                        }
                        ui.ctx().request_repaint();
                    }
                }
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
                }
            }
            if !down {
                if rich.drag.take().is_some() {
                    buf.normalize();
                }
            }

            // paint the visible lines
            let (vis0, vis1) = (viewport.min.y - top_pad - page_pad - 100.0, viewport.max.y - top_pad - page_pad + 100.0);
            let first = rich.ys.partition_point(|y| *y < vis0).saturating_sub(1);
            let mut sels = buf.sels.clone();
            sels.sort_by_key(|s| s.start());
            let sel_color = if focused { pal.selection() } else { theme::mix(pal.doc_bg, pal.doc_text, 0.12) };
            let p = ui.painter().clone();
            for li in first..rich.lines.len() {
                let l = &rich.lines[li];
                let y = rich.ys[li];
                if y > vis1 {
                    break;
                }
                if l.skip {
                    continue;
                }
                let top = doc.y + y;
                // code block background: painted once, at the first visible line of the run
                if l.code && (li == first || !rich.lines[li - 1].code) {
                    let mut e = li;
                    while e + 1 < rich.lines.len() && rich.lines[e + 1].code {
                        e += 1;
                    }
                    let bottom = doc.y + rich.ys[e] + rich.lines[e].height + 5.0;
                    p.rect_filled(Rect::from_min_max(pos2(doc.x, top - 5.0), pos2(doc.x + column, bottom)), CornerRadius::same(8), pal.doc_code);
                }
                if let Some(w) = &l.widget {
                    stale |= paint_laid(ui, w, vec2(doc.x, top), &pal, &mut *assets);
                    continue;
                }
                for q in 0..l.quote {
                    p.rect_filled(Rect::from_min_size(pos2(doc.x + q as f32 * size * 1.1, top - 2.0), vec2(3.0, l.height + 4.0)), CornerRadius::same(2), pal.doc_border);
                }
                match l.deco {
                    Deco::Bullet => {
                        p.circle_filled(pos2(doc.x + l.x0 - size * 0.8, top + size * 0.82), size * 0.17, pal.muted);
                    }
                    Deco::Check(on) => {
                        let s = size.round();
                        let r = Rect::from_min_size(pos2(doc.x + l.x0 - size * 1.4, top + (size * 1.62 - s) / 2.0), vec2(s, s));
                        if on {
                            p.rect_filled(r, CornerRadius::same(5), pal.accent);
                            let g = theme::text(ui, icons::CHECK, s * 0.78, pal.on_accent(), 700.0);
                            p.galley(r.center() - g.size() / 2.0, g, pal.on_accent());
                        } else {
                            p.rect_stroke(r, CornerRadius::same(5), Stroke::new(1.5, pal.doc_border), egui::StrokeKind::Inside);
                        }
                    }
                    Deco::Rule if l.galley.job.text.trim().is_empty() => {
                        let my = top + l.height / 2.0;
                        p.line_segment([pos2(doc.x, my), pos2(doc.x + column, my)], Stroke::new(1.0, pal.doc_border));
                    }
                    _ => {}
                }
                // selection
                let (ls, le) = (buf.line_start(li), buf.line_end(li));
                let gp = pos2(doc.x + l.x0, top);
                let si = sels.partition_point(|s| s.end() < ls);
                for s in sels[si..].iter().take_while(|s| s.start() <= le) {
                    if s.is_empty() {
                        continue;
                    }
                    let (a, z) = (s.start().max(ls) - ls, s.end().min(le) - ls);
                    let (da, dz) = (l.dcol(a), l.dcol(z));
                    let mut rects = md::char_rects(&l.galley, da, dz);
                    if s.end() > le {
                        // the line break is part of the selection
                        let end = l.caret(z);
                        rects.push(Rect::from_min_size(end.min, vec2(size * 0.4, end.height())));
                    }
                    for r in rects {
                        p.rect_filled(r.translate(gp.to_vec2()), CornerRadius::same(3), sel_color);
                    }
                }
                p.galley(gp, l.galley.clone(), pal.doc_text);
                if l.heading > 0 && l.heading <= 2 {
                    let by = top + l.height + size * 0.2;
                    p.line_segment([pos2(doc.x, by), pos2(doc.x + column, by)], Stroke::new(1.0, pal.doc_border));
                }
            }
            // carets
            if focused {
                let on = ((now - rich.blink) % 1.06) < 0.56;
                for s in &buf.sels {
                    let r = rich.caret_rect(buf, s.head).translate(doc);
                    let r = Rect::from_min_size(pos2(r.min.x - 1.0, r.min.y + 1.0), vec2(2.0, (r.height() - 2.0).max(size)));
                    if on {
                        p.rect_filled(r, CornerRadius::same(1), pal.accent);
                    }
                    if s.head == buf.primary().head {
                        ui.ctx().output_mut(|o| o.ime = Some(egui::output::IMEOutput { rect: full, cursor_rect: r, purpose: egui::IMEPurpose::Normal, should_interrupt_composition: false }));
                    }
                }
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(280));
            }
        });
        rich.scroll = out.state.offset().y;
        if stale {
            rich.cache.clear();
            rich.key.0 = u64::MAX;
            ui.ctx().request_repaint();
        }
        if buf.version != v0 {
            edited = true;
        }
    });
    // toggled check boxes
    for li in toggles {
        let text = buf.line_text(li);
        if let Some((byte, new)) = text.find("[ ]").map(|i| (i, "[x]")).or_else(|| text.find("[x]").or(text.find("[X]")).map(|i| (i, "[ ]"))) {
            let st = buf.line_start(li) + text[..byte].chars().count();
            let sels = buf.sels.clone();
            buf.edit(vec![crate::buffer::Edit::new(st, st + 3, new)], crate::buffer::EditKind::Other);
            buf.sels = sels;
            buf.commit_sels();
            edited = true;
        }
    }
    if edited {
        d.edited_at = Some(now);
        ui.ctx().request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(text: &str) -> (String, Vec<bool>) {
        let c: Vec<char> = text.chars().collect();
        let mut segs = Vec::new();
        scan(&c, 0, c.len(), &St::default(), &mut segs);
        let visible: String = segs.iter().filter(|s| !s.marker).flat_map(|s| c[s.a..s.b].iter()).collect();
        let bold: Vec<bool> = segs.iter().filter(|s| !s.marker).map(|s| s.st.bold).collect();
        (visible, bold)
    }

    #[test]
    fn hides_markup() {
        assert_eq!(shown("a **bold** and *it* `c*d*` ~~x~~ ==m==").0, "a bold and it c*d* x m");
        assert_eq!(shown("see [the docs](http://x.y) now").0, "see the docs now");
        assert_eq!(shown("press <kbd>Ctrl</kbd>+<kbd>S</kbd>").0, "press Ctrl+S");
        assert_eq!(shown("2 * 3 * 4 and snake_case_name").0, "2 * 3 * 4 and snake_case_name");
        assert_eq!(shown(r"\*not\*").0, "*not*");
        assert!(shown("x **b**").1.contains(&true));
        // every source position is covered exactly once
        let c: Vec<char> = "a **b** [c](d) `e`".chars().collect();
        let mut segs = Vec::new();
        scan(&c, 0, c.len(), &St::default(), &mut segs);
        let mut at = 0;
        for s in &segs {
            assert_eq!(s.a, at);
            at = s.b;
        }
        assert_eq!(at, c.len());
    }

    #[test]
    fn block_prefixes() {
        let p = |s: &str| prefix(&s.chars().collect::<Vec<_>>());
        assert_eq!((p("## Title").heading, p("## Title").len), (2, 3));
        assert_eq!(p("- [x] done").deco, Deco::Check(true));
        assert_eq!(p("- [x] done").len, 6);
        assert_eq!(p("  * item").deco, Deco::Bullet);
        assert_eq!(p("  * item").indent_cols, 2);
        assert_eq!(p("12. item").keep_from, Some(0));
        assert_eq!(p("> > quoted").quote, 2);
        assert_eq!(p("---").deco, Deco::Rule);
        assert_eq!(p("plain").len, 0);
    }
}
