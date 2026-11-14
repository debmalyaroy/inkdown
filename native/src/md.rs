//! Markdown: parse (comrak) into an owned block model, and lay that model out into
//! draw items. The reader paints the items; export and the Word-style editor reuse the model.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;

use comrak::nodes::{AlertType, AstNode, ListType, NodeValue, TableAlignment};
use egui::text::LayoutJob;
use egui::{pos2, vec2, Align, Color32, FontFamily, FontId, Galley, Pos2, Rect, Stroke, TextFormat};

use crate::assets::{self, hash64, Assets, State};
use crate::icons;
use crate::theme::{self, Palette};

// ------------------------------------------------------------------ model

#[derive(Clone, Default, PartialEq, Eq, Hash, Debug)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub underline: bool,
    pub mark: bool,
    pub sup: bool,
    pub sub: bool,
    pub kbd: bool,
    pub link: Option<Arc<str>>,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Inl {
    Text(String, Style),
    Image { url: String, alt: String, title: String, link: Option<Arc<str>>, width: Option<u32> },
    Math(String),
    Break,
    FootRef(String, u32),
}

/// A source line that does not take part in hashing, so cached layouts survive edits above them.
#[derive(Clone, Copy, Debug, Default)]
pub struct Line(pub usize);
impl PartialEq for Line {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl Eq for Line {}
impl Hash for Line {
    fn hash<H: Hasher>(&self, _: &mut H) {}
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Item {
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
    /// Offset of this item's line from the first line of its top-level block.
    pub rel_line: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AlertKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CellAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DiagramKind {
    Mermaid,
    Dot,
    Svg,
    Chart,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Block {
    Heading { level: u8, inl: Vec<Inl>, id: String },
    Para(Vec<Inl>),
    Code { lang: String, text: String },
    Quote(Vec<Block>),
    Alert { kind: AlertKind, title: Option<String>, body: Vec<Block> },
    List { ordered: bool, start: usize, tight: bool, items: Vec<Item> },
    Table { aligns: Vec<CellAlign>, rows: Vec<Vec<Vec<Inl>>> },
    Rule,
    Math(String),
    Diagram { kind: DiagramKind, src: String },
    Details { summary: Vec<Inl>, body: Vec<Block> },
    Footnote { index: u32, name: String, body: Vec<Block> },
    DefList(Vec<(Vec<Inl>, Vec<Vec<Block>>)>),
    FrontMatter(Vec<(String, String)>),
    Center(Vec<Block>),
    Image { url: String, alt: String, title: String, link: Option<Arc<str>>, width: Option<u32> },
}

pub struct Top {
    pub block: Block,
    pub line0: usize,
    pub line1: usize,
}

pub fn options() -> comrak::Options<'static> {
    let mut o = comrak::Options::default();
    let e = &mut o.extension;
    e.strikethrough = true;
    e.table = true;
    e.autolink = true;
    e.tasklist = true;
    e.superscript = true;
    e.subscript = true;
    e.footnotes = true;
    e.inline_footnotes = true;
    e.description_lists = true;
    e.front_matter_delimiter = Some("---".to_string());
    e.multiline_block_quotes = true;
    e.alerts = true;
    e.math_dollars = true;
    e.math_code = true;
    e.shortcodes = true;
    e.wikilinks_title_after_pipe = true;
    e.highlight = true;
    e.insert = true;
    e.underline = false;
    let p = &mut o.parse;
    p.relaxed_tasklist_matching = true;
    p.relaxed_autolinks = true;
    p.tasklist_in_table = true;
    o
}

pub fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.trim().chars() {
        if c.is_alphanumeric() || c == '_' {
            out.extend(c.to_lowercase());
        } else if (c == ' ' || c == '-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

pub fn plain(inl: &[Inl]) -> String {
    let mut s = String::new();
    for i in inl {
        match i {
            Inl::Text(t, _) => s.push_str(t),
            Inl::Image { alt, .. } => s.push_str(alt),
            Inl::Math(t) => s.push_str(t),
            Inl::Break => s.push(' '),
            Inl::FootRef(..) => {}
        }
    }
    s
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest.find(';').filter(|e| *e < 10);
        let Some(e) = end else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..e];
        let ch = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            "copy" => Some('©'),
            "reg" => Some('®'),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            "hellip" => Some('…'),
            "rarr" => Some('→'),
            "larr" => Some('←'),
            "times" => Some('×'),
            _ => name.strip_prefix("#x").and_then(|h| u32::from_str_radix(h, 16).ok()).or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok())).and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let at = from + i;
        let before_ok = at == 0 || lower.as_bytes()[at - 1].is_ascii_whitespace();
        let rest = tag[at + name.len()..].trim_start();
        if before_ok && rest.starts_with('=') {
            let v = rest[1..].trim_start();
            return Some(match v.chars().next() {
                Some(q @ ('"' | '\'')) => v[1..].split(q).next().unwrap_or("").to_string(),
                _ => v.split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or("").to_string(),
            });
        }
        from = at + name.len();
    }
    None
}

/// Tracks the inline style while walking nodes and raw HTML tags.
#[derive(Default)]
struct Inliner {
    out: Vec<Inl>,
    style: Style,
    stack: Vec<(String, Style)>,
}

impl Inliner {
    fn text(&mut self, t: &str) {
        if t.is_empty() {
            return;
        }
        if let Some(Inl::Text(prev, st)) = self.out.last_mut() {
            if *st == self.style {
                prev.push_str(t);
                return;
            }
        }
        self.out.push(Inl::Text(t.to_string(), self.style.clone()));
    }

    fn with(&mut self, f: impl FnOnce(&mut Style), body: impl FnOnce(&mut Self)) {
        let saved = self.style.clone();
        f(&mut self.style);
        body(self);
        self.style = saved;
    }

    /// One raw HTML tag such as `<kbd>`, `</kbd>`, `<br>`, `<img …>`.
    fn tag(&mut self, raw: &str) {
        let t = raw.trim();
        if t.starts_with("<!--") {
            return;
        }
        let inner = t.trim_start_matches('<').trim_end_matches('>').trim_end_matches('/').trim();
        let closing = inner.starts_with('/');
        let name: String = inner.trim_start_matches('/').chars().take_while(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
        if closing {
            if let Some(i) = self.stack.iter().rposition(|(n, _)| *n == name) {
                self.style = self.stack[i].1.clone();
                self.stack.truncate(i);
            }
            return;
        }
        match name.as_str() {
            "br" => self.out.push(Inl::Break),
            "img" => {
                if let Some(src) = attr(t, "src") {
                    let width = attr(t, "width").and_then(|w| w.trim_end_matches("px").parse().ok());
                    self.out.push(Inl::Image { url: src, alt: attr(t, "alt").unwrap_or_default(), title: attr(t, "title").unwrap_or_default(), link: self.style.link.clone(), width });
                }
            }
            "b" | "strong" | "i" | "em" | "code" | "kbd" | "samp" | "mark" | "sub" | "sup" | "u" | "ins" | "s" | "del" | "strike" | "a" | "span" | "small" | "tt" | "var" | "abbr" | "cite" | "q" | "font" => {
                self.stack.push((name.clone(), self.style.clone()));
                let s = &mut self.style;
                match name.as_str() {
                    "b" | "strong" => s.bold = true,
                    "i" | "em" | "var" | "cite" => s.italic = true,
                    "code" | "samp" | "tt" => s.code = true,
                    "kbd" => s.kbd = true,
                    "mark" => s.mark = true,
                    "sub" => s.sub = true,
                    "sup" => s.sup = true,
                    "u" | "ins" => s.underline = true,
                    "s" | "del" | "strike" => s.strike = true,
                    "a" => {
                        if let Some(h) = attr(t, "href") {
                            s.link = Some(h.into());
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    /// A fragment of HTML: tags are interpreted, text is kept.
    fn html(&mut self, html: &str) {
        let mut rest = html;
        while let Some(i) = rest.find('<') {
            let text = &rest[..i];
            if !text.is_empty() {
                let collapsed = collapse_ws(&decode_entities(text));
                self.text(&collapsed);
            }
            match rest[i..].find('>') {
                Some(e) => {
                    self.tag(&rest[i..i + e + 1]);
                    rest = &rest[i + e + 1..];
                }
                None => {
                    rest = "";
                }
            }
        }
        if !rest.is_empty() {
            let collapsed = collapse_ws(&decode_entities(rest));
            self.text(&collapsed);
        }
    }

    fn node<'a>(&mut self, node: &'a AstNode<'a>) {
        let value = node.data.borrow().value.clone();
        let kids = |me: &mut Self| {
            for c in node.children() {
                me.node(c);
            }
        };
        match value {
            NodeValue::Text(t) => self.text(&t),
            NodeValue::SoftBreak => self.text(" "),
            NodeValue::LineBreak => self.out.push(Inl::Break),
            NodeValue::Code(c) => self.with(|s| s.code = true, |me| me.text(&c.literal)),
            NodeValue::Emph => self.with(|s| s.italic = true, kids),
            NodeValue::Strong => self.with(|s| s.bold = true, kids),
            NodeValue::Strikethrough => self.with(|s| s.strike = true, kids),
            NodeValue::Highlight => self.with(|s| s.mark = true, kids),
            NodeValue::Insert | NodeValue::Underline => self.with(|s| s.underline = true, kids),
            NodeValue::Superscript => self.with(|s| s.sup = true, kids),
            NodeValue::Subscript => self.with(|s| s.sub = true, kids),
            NodeValue::Link(l) => {
                let url: Arc<str> = l.url.as_str().into();
                self.with(|s| s.link = Some(url), kids)
            }
            NodeValue::WikiLink(l) => {
                let url: Arc<str> = l.url.as_str().into();
                self.with(|s| s.link = Some(url), kids)
            }
            NodeValue::Image(l) => {
                let mut alt = Inliner::default();
                for c in node.children() {
                    alt.node(c);
                }
                self.out.push(Inl::Image { url: l.url.clone(), alt: plain(&alt.out), title: l.title.clone(), link: self.style.link.clone(), width: None });
            }
            NodeValue::Math(m) => {
                if m.display_math {
                    self.out.push(Inl::Break);
                }
                self.out.push(Inl::Math(m.literal.clone()));
                if m.display_math {
                    self.out.push(Inl::Break);
                }
            }
            NodeValue::FootnoteReference(f) => self.out.push(Inl::FootRef(f.name.clone(), f.ix)),
            NodeValue::ShortCode(s) => self.text(&s.emoji),
            NodeValue::HtmlInline(h) => self.tag(&h),
            NodeValue::Raw(r) => self.text(&r),
            NodeValue::EscapedTag(t) => self.text(t),
            _ => kids(self),
        }
    }
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = false;
    for c in s.chars() {
        if c.is_whitespace() && c != '\u{a0}' {
            if !prev_ws {
                out.push(' ');
            }
            prev_ws = true;
        } else {
            out.push(c);
            prev_ws = false;
        }
    }
    out
}

fn inlines<'a>(node: &'a AstNode<'a>) -> Vec<Inl> {
    let mut i = Inliner::default();
    for c in node.children() {
        i.node(c);
    }
    i.out
}

fn front_matter(raw: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in raw.lines() {
        let t = line.trim_end();
        if t.trim() == "---" || t.trim().is_empty() || t.trim_start().starts_with('#') {
            continue;
        }
        let nested = t.starts_with(' ') || t.starts_with('\t') || t.starts_with('-');
        match t.split_once(':') {
            Some((k, v)) if !nested && !k.contains(' ') => out.push((k.trim().to_string(), v.trim().trim_matches(|c| c == '"' || c == '\'').to_string())),
            _ => {
                if let Some(last) = out.last_mut() {
                    if !last.1.is_empty() {
                        last.1.push_str(", ");
                    }
                    last.1.push_str(t.trim().trim_start_matches("- ").trim());
                }
            }
        }
    }
    out
}

fn html_block(html: &str) -> Option<Block> {
    let t = html.trim();
    let lower = t.to_lowercase();
    if t.starts_with("<!--") && t.ends_with("-->") {
        return None;
    }
    if lower.starts_with("<details") {
        if let (Some(s0), Some(s1)) = (lower.find("<summary"), lower.find("</summary>")) {
            let s_open_end = lower[s0..].find('>').map(|i| s0 + i + 1).unwrap_or(s0);
            let mut sum = Inliner::default();
            sum.html(&t[s_open_end..s1]);
            let body_end = lower.rfind("</details>").unwrap_or(t.len());
            let body = &t[s1 + "</summary>".len()..body_end];
            return Some(Block::Details { summary: sum.out, body: parse(body).into_iter().map(|t| t.block).collect() });
        }
    }
    if lower.starts_with("<hr") {
        return Some(Block::Rule);
    }
    let centered = lower.starts_with("<center") || ((lower.starts_with("<p") || lower.starts_with("<div") || lower.starts_with("<h")) && lower[..lower.find('>').unwrap_or(0)].contains("center"));
    let heading = lower.strip_prefix("<h").and_then(|r| r.chars().next()).and_then(|c| c.to_digit(10)).filter(|d| (1..=6).contains(d));
    let mut i = Inliner::default();
    i.html(t);
    let inl: Vec<Inl> = i.out;
    if inl.is_empty() || inl.iter().all(|x| matches!(x, Inl::Text(s, _) if s.trim().is_empty())) {
        return None;
    }
    // split standalone images out so they are laid out as blocks
    let mut blocks = Vec::new();
    let mut run: Vec<Inl> = Vec::new();
    let only_images = inl.iter().all(|x| matches!(x, Inl::Image { .. } | Inl::Break) || matches!(x, Inl::Text(s, _) if s.trim().is_empty()));
    let n_images = inl.iter().filter(|x| matches!(x, Inl::Image { .. })).count();
    if only_images && n_images == 1 {
        for x in inl {
            if let Inl::Image { url, alt, title, link, width } = x {
                blocks.push(Block::Image { url, alt, title, link, width });
            }
        }
    } else {
        run.extend(inl);
    }
    if !run.is_empty() {
        blocks.push(match heading {
            Some(level) => {
                let id = slug(&plain(&run));
                Block::Heading { level: level as u8, inl: run, id }
            }
            None => Block::Para(run),
        });
    }
    Some(if centered { Block::Center(blocks) } else if blocks.len() == 1 { blocks.pop().unwrap() } else { Block::Center(blocks) })
}

fn blocks_of<'a>(node: &'a AstNode<'a>, top_line: usize) -> Vec<Block> {
    node.children().filter_map(|c| block_of(c, top_line)).collect()
}

fn block_of<'a>(node: &'a AstNode<'a>, top_line: usize) -> Option<Block> {
    let data = node.data.borrow();
    let line = data.sourcepos.start.line;
    Some(match &data.value {
        NodeValue::Paragraph => {
            let inl = inlines(node);
            // a paragraph that is only an image becomes a block image
            if let [Inl::Image { url, alt, title, link, width }] = inl.as_slice() {
                return Some(Block::Image { url: url.clone(), alt: alt.clone(), title: title.clone(), link: link.clone(), width: *width });
            }
            // display math on its own
            if let [Inl::Break, Inl::Math(m), Inl::Break] = inl.as_slice() {
                return Some(Block::Math(m.clone()));
            }
            Block::Para(inl)
        }
        NodeValue::Heading(h) => {
            let inl = inlines(node);
            let id = slug(&plain(&inl));
            Block::Heading { level: h.level, inl, id }
        }
        NodeValue::CodeBlock(c) => {
            let lang = c.info.split_whitespace().next().unwrap_or("").to_lowercase();
            let text = c.literal.trim_end_matches('\n').to_string();
            match lang.as_str() {
                "mermaid" => Block::Diagram { kind: DiagramKind::Mermaid, src: text },
                "dot" | "graphviz" | "gv" => Block::Diagram { kind: DiagramKind::Dot, src: text },
                "svg" => Block::Diagram { kind: DiagramKind::Svg, src: text },
                "chart" | "echarts" => Block::Diagram { kind: DiagramKind::Chart, src: text },
                "math" | "latex" | "tex" if c.fenced && lang == "math" => Block::Math(text),
                _ => Block::Code { lang, text },
            }
        }
        NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) => Block::Quote(blocks_of(node, top_line)),
        NodeValue::Alert(a) => Block::Alert {
            kind: match a.alert_type {
                AlertType::Note => AlertKind::Note,
                AlertType::Tip => AlertKind::Tip,
                AlertType::Important => AlertKind::Important,
                AlertType::Warning => AlertKind::Warning,
                AlertType::Caution => AlertKind::Caution,
            },
            title: a.title.clone(),
            body: blocks_of(node, top_line),
        },
        NodeValue::List(l) => {
            let items = node
                .children()
                .map(|item| {
                    let d = item.data.borrow();
                    let task = match &d.value {
                        NodeValue::TaskItem(t) => Some(t.symbol.is_some()),
                        _ => None,
                    };
                    Item { task, blocks: blocks_of(item, top_line), rel_line: d.sourcepos.start.line.saturating_sub(top_line) }
                })
                .collect();
            Block::List { ordered: l.list_type == ListType::Ordered, start: l.start, tight: l.tight, items }
        }
        NodeValue::Table(t) => {
            let aligns = t
                .alignments
                .iter()
                .map(|a| match a {
                    TableAlignment::Center => CellAlign::Center,
                    TableAlignment::Right => CellAlign::Right,
                    _ => CellAlign::Left,
                })
                .collect();
            let rows = node.children().map(|row| row.children().map(inlines).collect()).collect();
            Block::Table { aligns, rows }
        }
        NodeValue::ThematicBreak => Block::Rule,
        NodeValue::HtmlBlock(h) => return html_block(&h.literal),
        NodeValue::FootnoteDefinition(f) => Block::Footnote { index: 0, name: f.name.clone(), body: blocks_of(node, top_line) },
        NodeValue::DescriptionList => {
            let mut out: Vec<(Vec<Inl>, Vec<Vec<Block>>)> = Vec::new();
            for item in node.children() {
                let mut term = Vec::new();
                let mut details = Vec::new();
                for part in item.children() {
                    match part.data.borrow().value {
                        NodeValue::DescriptionTerm => term = part.children().flat_map(inlines).collect(),
                        NodeValue::DescriptionDetails => details.push(blocks_of(part, top_line)),
                        _ => {}
                    }
                }
                out.push((term, details));
            }
            Block::DefList(out)
        }
        NodeValue::FrontMatter(raw) => Block::FrontMatter(front_matter(raw)),
        _ => {
            let _ = line;
            return None;
        }
    })
}

pub fn parse(text: &str) -> Vec<Top> {
    let arena = comrak::Arena::new();
    let root = comrak::parse_document(&arena, text, &options());
    let mut out = Vec::new();
    let mut foot = 0u32;
    let nodes: Vec<&AstNode> = root.children().collect();
    let mut i = 0;
    while i < nodes.len() {
        let node = nodes[i];
        i += 1;
        let (l0, mut l1) = {
            let d = node.data.borrow();
            (d.sourcepos.start.line, d.sourcepos.end.line)
        };
        let mut made = block_of(node, l0);
        // `<details>` written with blank lines arrives as several blocks: gather them up to `</details>`
        let open_details = match &node.data.borrow().value {
            NodeValue::HtmlBlock(h) => {
                let l = h.literal.to_lowercase();
                l.trim_start().starts_with("<details") && !l.contains("</details>")
            }
            _ => false,
        };
        if open_details {
            let mut inner = Vec::new();
            while i < nodes.len() {
                let n = nodes[i];
                i += 1;
                l1 = n.data.borrow().sourcepos.end.line;
                let closes = matches!(&n.data.borrow().value, NodeValue::HtmlBlock(h) if h.literal.to_lowercase().contains("</details>"));
                if closes {
                    break;
                }
                if let Some(b) = block_of(n, l0) {
                    inner.push(b);
                }
            }
            made = match made {
                Some(Block::Details { summary, mut body }) => {
                    body.extend(inner);
                    Some(Block::Details { summary, body })
                }
                other => {
                    let summary = match other {
                        Some(Block::Para(inl)) => inl,
                        _ => vec![Inl::Text("Details".into(), Style::default())],
                    };
                    Some(Block::Details { summary, body: inner })
                }
            };
        }
        if let Some(mut block) = made {
            if let Block::Footnote { index, .. } = &mut block {
                foot += 1;
                *index = foot;
            }
            out.push(Top { block, line0: l0.max(1), line1: l1.max(l0).max(1) });
        }
    }
    out
}

/// Headings for the outline: `(level, text, id, source line)`.
pub fn outline(tops: &[Top]) -> Vec<(u8, String, String, usize)> {
    tops.iter()
        .filter_map(|t| match &t.block {
            Block::Heading { level, inl, id } => Some((*level, plain(inl), id.clone(), t.line0)),
            _ => None,
        })
        .collect()
}

// ----------------------------------------------------------------- layout

pub enum Draw {
    Text { pos: Pos2, galley: Arc<Galley>, links: Vec<(usize, usize, Arc<str>)> },
    Rect { rect: Rect, fill: Color32, stroke: Stroke, radius: u8 },
    Line { a: Pos2, b: Pos2, stroke: Stroke },
    Image { rect: Rect, key: u64, radius: u8, link: Option<Arc<str>>, zoom: bool, card: bool },
    Check { rect: Rect, checked: bool, rel_line: usize },
    Copy { rect: Rect, text: Arc<str> },
    Toggle { rect: Rect, key: u64 },
}

pub struct Laid {
    pub height: f32,
    pub items: Vec<Draw>,
    /// Still waiting on an image or diagram; lay out again when assets change.
    pub pending: bool,
    pub anchors: Vec<(String, f32)>,
}

pub struct Lay<'a> {
    pub ui: &'a egui::Ui,
    pub pal: &'a Palette,
    pub size: f32,
    pub assets: &'a mut Assets,
    pub base: Option<PathBuf>,
    pub ppp: f32,
    pub open: &'a HashSet<u64>,
    pub pending: bool,
    pub anchors: Vec<(String, f32)>,
    /// Column width, for sizing diagrams and images.
    pub column: f32,
}

struct Obj {
    at: usize,
    key: u64,
    size: egui::Vec2,
    depth: f32,
    link: Option<Arc<str>>,
    math: bool,
}

struct TextOut {
    galley: Arc<Galley>,
    links: Vec<(usize, usize, Arc<str>)>,
    objs: Vec<Obj>,
    kbds: Vec<(usize, usize)>,
}

/// Rectangles (one per row) covering chars `a..b` of a galley, in galley coordinates.
pub fn char_rects(galley: &Galley, a: usize, b: usize) -> Vec<Rect> {
    char_rects_of(galley, a, b, false)
}

/// As [`char_rects`]; `tight` uses the font's own height instead of the (taller) line height.
fn char_rects_of(galley: &Galley, a: usize, b: usize, tight: bool) -> Vec<Rect> {
    let mut out = Vec::new();
    let mut idx = 0usize;
    for row in &galley.rows {
        let n = row.row.glyphs.len();
        if idx + n > a && idx < b {
            let mut r: Option<Rect> = None;
            for (gi, g) in row.row.glyphs.iter().enumerate() {
                let ci = idx + gi;
                if ci >= a && ci < b {
                    let gr = if tight { Rect::from_min_size(g.pos - vec2(0.0, g.font_ascent), vec2(g.advance_width, g.font_height)) } else { g.logical_rect() };
                    let gr = gr.translate(row.pos.to_vec2());
                    r = Some(r.map_or(gr, |x| x.union(gr)));
                }
            }
            if let Some(r) = r {
                out.push(r);
            }
        }
        idx += n + usize::from(row.ends_with_newline);
        if idx >= b {
            break;
        }
    }
    out
}

/// Baseline y of the row holding char `at`, in galley coordinates.
fn baseline_at(galley: &Galley, at: usize) -> Option<(f32, f32, f32)> {
    let mut idx = 0usize;
    for row in &galley.rows {
        let n = row.row.glyphs.len();
        if at < idx + n {
            let g = &row.row.glyphs[at - idx];
            let r = g.logical_rect().translate(row.pos.to_vec2());
            return Some((r.max.x, r.min.y + g.font_ascent, row.pos.y + row.row.size.y / 2.0));
        }
        idx += n + usize::from(row.ends_with_newline);
    }
    None
}

impl<'a> Lay<'a> {
    fn base(&self, scale: f32, color: Color32, weight: f32) -> TextFormat {
        let size = (self.size * scale * 4.0).round() / 4.0;
        TextFormat { font_id: FontId::proportional(size), color, coords: theme::weight(weight), line_height: Some((size * 1.62).round()), ..Default::default() }
    }

    fn image_key(&mut self, url: &str, max_w: f32) -> (u64, State<'_>) {
        let key = hash64(&[&"img", &url, &self.base, &((max_w / 32.0) as u32), &((self.ppp * 100.0) as u32)]);
        let (u, base, ppp) = (url.to_string(), self.base.clone(), self.ppp);
        (key, self.assets.get(key, || assets::job_image(u, base, ppp, max_w)))
    }

    fn math_key(&mut self, tex: &str, display: bool, size: f32, color: Color32) -> (u64, State<'_>) {
        let key = hash64(&[&"math", &tex, &display, &((size * 8.0) as u32), &color.to_array(), &((self.ppp * 100.0) as u32)]);
        let (t, ppp) = (tex.to_string(), self.ppp);
        (key, self.assets.get(key, || assets::job_math(t, display, size, color, ppp)))
    }

    fn inline(&mut self, inl: &[Inl], base: &TextFormat, width: f32, halign: Align) -> TextOut {
        let mut job = LayoutJob::default();
        job.wrap.max_width = width.max(40.0);
        job.halign = halign;
        let mut links = Vec::new();
        let mut objs = Vec::new();
        let mut kbds = Vec::new();
        let mut n = 0usize; // chars so far
        let size = base.font_id.size;
        let pal = self.pal;
        for i in inl {
            match i {
                Inl::Text(t, st) => {
                    let mut f = base.clone();
                    if st.bold {
                        f.coords = theme::weight(700.0);
                    }
                    if st.italic {
                        f.font_id.family = theme::italic_family();
                    }
                    if st.code || st.kbd {
                        f.font_id = FontId::new((size * 0.88 * 4.0).round() / 4.0, FontFamily::Monospace);
                        f.coords = Default::default();
                        if st.code {
                            f.background = pal.doc_code;
                            f.expand_bg = 2.0;
                        }
                        if st.kbd {
                            f.font_id.size = (size * 0.8 * 4.0).round() / 4.0;
                            f.coords = theme::weight(600.0);
                        }
                    }
                    if st.mark {
                        f.background = pal.mark;
                        f.expand_bg = 2.0;
                    }
                    if st.strike {
                        f.strikethrough = Stroke::new(1.0, theme::with_alpha(f.color, 0.7));
                        f.color = theme::mix(f.color, pal.doc_bg, 0.35);
                    }
                    if st.underline {
                        f.underline = Stroke::new(1.0, pal.tip);
                    }
                    if st.sup || st.sub {
                        f.font_id.size = (size * 0.72 * 4.0).round() / 4.0;
                        f.valign = if st.sup { Align::TOP } else { Align::BOTTOM };
                    }
                    let len = t.chars().count();
                    if let Some(url) = &st.link {
                        f.color = pal.accent;
                        links.push((n, n + len, url.clone()));
                    }
                    if st.kbd {
                        // room for the key cap border
                        job.append("\u{a0}", 0.0, TextFormat { font_id: FontId::proportional(size * 0.3), ..base.clone() });
                        n += 1;
                        kbds.push((n, n + len));
                        job.append(t, 0.0, f);
                        job.append("\u{a0}", 0.0, TextFormat { font_id: FontId::proportional(size * 0.3), ..base.clone() });
                        n += len + 1;
                    } else {
                        job.append(t, 0.0, f);
                        n += len;
                    }
                }
                Inl::Break => {
                    job.append("\n", 0.0, base.clone());
                    n += 1;
                }
                Inl::FootRef(_, ix) => {
                    let mut f = base.clone();
                    f.font_id.size = (size * 0.72 * 4.0).round() / 4.0;
                    f.valign = Align::TOP;
                    f.color = pal.accent;
                    let t = format!("[{ix}]");
                    links.push((n, n + t.chars().count(), format!("#fn-{ix}").into()));
                    n += t.chars().count();
                    job.append(&t, 0.0, f);
                }
                Inl::Math(tex) => {
                    let color = base.color;
                    let (key, state) = self.math_key(tex, false, size * 1.08, color);
                    let (sz, depth, ok) = match state {
                        State::Ready(a) => (a.size, a.depth, true),
                        State::Pending => (vec2(tex.chars().count() as f32 * size * 0.5, size), 0.0, true),
                        State::Failed(_) => (egui::Vec2::ZERO, 0.0, false),
                    };
                    if matches!(self.assets.peek(key), None) && ok {
                        self.pending = true;
                    }
                    if !ok {
                        let mut f = base.clone();
                        f.font_id = FontId::monospace(size * 0.88);
                        f.color = pal.caution;
                        job.append(&format!("${tex}$"), 0.0, f);
                        n += tex.chars().count() + 2;
                        continue;
                    }
                    let mut f = base.clone();
                    f.font_id = FontId::proportional(1.0);
                    let need = sz.y + 2.0;
                    f.line_height = Some(base.line_height.unwrap_or(size * 1.6).max(need));
                    job.append("\u{a0}", sz.x.max(1.0), f);
                    objs.push(Obj { at: n, key, size: sz, depth, link: None, math: true });
                    n += 1;
                }
                Inl::Image { url, link, width, .. } => {
                    let max_w = width.map(|w| w as f32).unwrap_or(width_cap(self.column)).min(self.column);
                    let (key, state) = self.image_key(url, max_w);
                    let sz = match state {
                        State::Ready(a) => {
                            let s = a.size;
                            let scale = (max_w / s.x).min(1.0).min(if width.is_some() { max_w / s.x } else { 1.0 });
                            let scale = if width.is_some() { max_w / s.x } else { scale };
                            s * scale
                        }
                        State::Pending => {
                            self.pending = true;
                            vec2(size, size)
                        }
                        State::Failed(_) => vec2(size, size),
                    };
                    let mut f = base.clone();
                    f.font_id = FontId::proportional(1.0);
                    f.line_height = Some(base.line_height.unwrap_or(size * 1.6).max(sz.y + 4.0));
                    job.append("\u{a0}", sz.x.max(1.0), f);
                    objs.push(Obj { at: n, key, size: sz, depth: 0.0, link: link.clone(), math: false });
                    n += 1;
                }
            }
        }
        if job.sections.is_empty() {
            job.append("", 0.0, base.clone());
        }
        let galley = self.ui.fonts_mut(|f| f.layout_job(job));
        TextOut { galley, links, objs, kbds }
    }

    /// Lay out inline content at `(x, y)`; returns the height used.
    fn text(&mut self, inl: &[Inl], base: &TextFormat, x: f32, y: f32, w: f32, halign: Align, out: &mut Vec<Draw>) -> f32 {
        let t = self.inline(inl, base, w, halign);
        let origin = match halign {
            Align::Center => pos2(x + w / 2.0, y),
            Align::Max => pos2(x + w, y),
            _ => pos2(x, y),
        };
        let pal = self.pal;
        for (a, b) in &t.kbds {
            for r in char_rects_of(&t.galley, *a, *b, true) {
                let r = r.translate(origin.to_vec2()).expand2(vec2(4.5, 1.0));
                out.push(Draw::Rect { rect: r.translate(vec2(0.0, 1.5)), fill: pal.doc_border, stroke: Stroke::NONE, radius: 5 });
                out.push(Draw::Rect { rect: r, fill: pal.doc_subtle, stroke: Stroke::new(1.0, pal.doc_border), radius: 5 });
            }
        }
        for o in &t.objs {
            if let Some((right, baseline, mid)) = baseline_at(&t.galley, o.at) {
                let left = origin.x + right - o.size.x;
                let top = if o.math { origin.y + baseline - (o.size.y - o.depth) } else { origin.y + mid - o.size.y / 2.0 };
                out.push(Draw::Image { rect: Rect::from_min_size(pos2(left, top), o.size), key: o.key, radius: if o.math { 0 } else { 4 }, link: o.link.clone(), zoom: !o.math, card: false });
            }
        }
        let h = t.galley.size().y;
        out.push(Draw::Text { pos: origin, galley: t.galley, links: t.links });
        h
    }

    fn plain_text(&mut self, s: &str, f: TextFormat, x: f32, y: f32, w: f32, out: &mut Vec<Draw>) -> f32 {
        self.text(&[Inl::Text(s.to_string(), Style::default())], &f, x, y, w, Align::Min, out)
    }

    pub fn blocks(&mut self, blocks: &[Block], x: f32, y: f32, w: f32, color: Color32, out: &mut Vec<Draw>) -> f32 {
        let mut cy = y;
        for (i, b) in blocks.iter().enumerate() {
            if i > 0 {
                cy += self.gap(b);
            }
            cy += self.block(b, x, cy, w, color, out);
        }
        cy - y
    }

    pub fn gap(&self, b: &Block) -> f32 {
        match b {
            Block::Heading { level, .. } => self.size * if *level <= 2 { 1.7 } else { 1.35 },
            _ => self.size * 0.95,
        }
    }

    pub fn block(&mut self, b: &Block, x: f32, y: f32, w: f32, color: Color32, out: &mut Vec<Draw>) -> f32 {
        let pal = *self.pal;
        let em = self.size;
        match b {
            Block::Para(inl) => {
                let f = self.base(1.0, color, 400.0);
                self.text(inl, &f, x, y, w, Align::Min, out)
            }
            Block::Heading { level, inl, id } => {
                let (scale, weight) = match level {
                    1 => (2.05, 720.0),
                    2 => (1.55, 680.0),
                    3 => (1.27, 660.0),
                    4 => (1.1, 650.0),
                    5 => (1.0, 650.0),
                    _ => (0.88, 650.0),
                };
                let mut f = self.base(scale, if *level == 6 { pal.muted } else { color }, weight);
                f.line_height = Some((f.font_id.size * 1.3).round());
                if *level == 1 {
                    f.extra_letter_spacing = -0.4;
                }
                self.anchors.push((id.clone(), y));
                let mut h = self.text(inl, &f, x, y, w, Align::Min, out);
                if *level <= 2 {
                    h += em * 0.32;
                    out.push(Draw::Line { a: pos2(x, y + h), b: pos2(x + w, y + h), stroke: Stroke::new(1.0, pal.doc_border) });
                    h += 1.0;
                }
                h
            }
            Block::Code { lang, text } => self.code(lang, text, x, y, w, out),
            Block::Quote(body) => {
                let pad = em * 1.1;
                let h = self.blocks(body, x + pad, y + em * 0.15, w - pad, pal.muted, out) + em * 0.3;
                out.push(Draw::Rect { rect: Rect::from_min_size(pos2(x, y), vec2(3.0, h)), fill: pal.doc_border, stroke: Stroke::NONE, radius: 2 });
                h
            }
            Block::Alert { kind, title, body } => {
                let (c, icon, name) = match kind {
                    AlertKind::Note => (pal.note, icons::INFO, "Note"),
                    AlertKind::Tip => (pal.tip, icons::LIGHTBULB, "Tip"),
                    AlertKind::Important => (pal.important, icons::MESSAGE_SQUARE, "Important"),
                    AlertKind::Warning => (pal.warning, icons::TRIANGLE_ALERT, "Warning"),
                    AlertKind::Caution => (pal.caution, icons::OCTAGON_ALERT, "Caution"),
                };
                let pad = em * 1.0;
                let idx = out.len();
                let mut h = em * 0.75;
                let tf = self.base(0.95, c, 650.0);
                let label = format!("{icon}  {}", title.clone().unwrap_or_else(|| name.to_string()));
                h += self.plain_text(&label, tf, x + pad, y + h, w - pad * 2.0, out);
                h += em * 0.35;
                h += self.blocks(body, x + pad, y + h, w - pad * 2.0, color, out);
                h += em * 0.8;
                let r = Rect::from_min_size(pos2(x, y), vec2(w, h));
                out.insert(idx, Draw::Rect { rect: r, fill: theme::mix(pal.doc_bg, c, if pal.dark { 0.13 } else { 0.07 }), stroke: Stroke::NONE, radius: 8 });
                out.insert(idx + 1, Draw::Rect { rect: Rect::from_min_size(pos2(x, y + 6.0), vec2(3.0, h - 12.0)), fill: c, stroke: Stroke::NONE, radius: 2 });
                h
            }
            Block::List { ordered, start, tight, items } => {
                let f = self.base(1.0, color, 400.0);
                let last = start + items.len().saturating_sub(1);
                let gutter = if *ordered { (last.to_string().len() as f32 * 0.62 + 1.25) * em } else { em * 1.55 };
                let mut cy = y;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        cy += em * if *tight { 0.28 } else { 0.8 };
                    }
                    let line_h = f.line_height.unwrap_or(em * 1.6);
                    match item.task {
                        Some(checked) => {
                            let s = (em * 1.0).round();
                            let r = Rect::from_min_size(pos2(x + gutter - s - em * 0.5, cy + (line_h - s) / 2.0), vec2(s, s));
                            out.push(Draw::Check { rect: r, checked, rel_line: item.rel_line });
                        }
                        None => {
                            let marker = if *ordered { format!("{}.", start + i) } else { "•".to_string() };
                            let mf = self.base(1.0, pal.muted, if *ordered { 450.0 } else { 700.0 });
                            let g = self.ui.fonts_mut(|fonts| {
                                let mut job = LayoutJob::default();
                                job.append(&marker, 0.0, mf.clone());
                                fonts.layout_job(job)
                            });
                            let gx = x + gutter - g.size().x - em * 0.45;
                            out.push(Draw::Text { pos: pos2(gx, cy), galley: g, links: Vec::new() });
                        }
                    }
                    let item_color = if item.task == Some(true) { pal.muted } else { color };
                    let h = self.item_blocks(&item.blocks, x + gutter, cy, w - gutter, item_color, *tight, out);
                    cy += h.max(line_h);
                }
                cy - y
            }
            Block::Table { aligns, rows } => self.table(aligns, rows, x, y, w, color, out),
            Block::Rule => {
                out.push(Draw::Line { a: pos2(x, y + em * 0.6), b: pos2(x + w, y + em * 0.6), stroke: Stroke::new(1.0, pal.doc_border) });
                em * 1.2
            }
            Block::Math(tex) => {
                let (key, state) = self.math_key(tex, true, em * 1.2, color);
                match state {
                    State::Ready(a) => {
                        let s = a.size;
                        let scale = (w / s.x).min(1.0);
                        let sz = s * scale;
                        out.push(Draw::Image { rect: Rect::from_min_size(pos2(x + (w - sz.x) / 2.0, y + em * 0.3), sz), key, radius: 0, link: None, zoom: false, card: false });
                        sz.y + em * 0.6
                    }
                    State::Pending => {
                        self.pending = true;
                        em * 3.0
                    }
                    State::Failed(e) => {
                        let f = TextFormat { font_id: FontId::monospace(em * 0.86), color: pal.caution, ..Default::default() };
                        self.plain_text(&format!("{tex}\n{e}"), f, x, y, w, out)
                    }
                }
            }
            Block::Diagram { kind, src } => self.diagram(*kind, src, x, y, w, out),
            Block::Image { url, alt, title, link, width } => {
                let max_w = width.map(|v| v as f32).unwrap_or(w).min(w);
                let (key, state) = self.image_key(url, max_w);
                match state {
                    State::Ready(a) => {
                        let s = a.size;
                        let scale = if width.is_some() { max_w / s.x } else { (max_w / s.x).min(1.0) };
                        let sz = s * scale;
                        out.push(Draw::Image { rect: Rect::from_min_size(pos2(x, y), sz), key, radius: 6, link: link.clone(), zoom: true, card: false });
                        let mut h = sz.y;
                        if !title.is_empty() {
                            let f = self.base(0.88, pal.muted, 400.0);
                            h += em * 0.35;
                            h += self.plain_text(title, f, x, y + h, w, out);
                        }
                        h
                    }
                    State::Pending => {
                        self.pending = true;
                        let r = Rect::from_min_size(pos2(x, y), vec2(w.min(320.0), em * 6.0));
                        out.push(Draw::Rect { rect: r, fill: pal.doc_subtle, stroke: Stroke::NONE, radius: 6 });
                        em * 6.0
                    }
                    State::Failed(e) => {
                        let f = self.base(0.9, pal.muted, 400.0);
                        let label = format!("{}  {}", icons::IMAGE, if alt.is_empty() { url.as_str() } else { alt.as_str() });
                        let idx = out.len();
                        let mut h = em * 0.6;
                        h += self.plain_text(&label, f.clone(), x + em * 0.8, y + h, w - em * 1.6, out);
                        let ef = self.base(0.8, pal.faint, 400.0);
                        h += self.plain_text(&e, ef, x + em * 0.8, y + h, w - em * 1.6, out);
                        h += em * 0.6;
                        out.insert(idx, Draw::Rect { rect: Rect::from_min_size(pos2(x, y), vec2(w.min(460.0), h)), fill: pal.doc_subtle, stroke: Stroke::new(1.0, pal.doc_border), radius: 6 });
                        h
                    }
                }
            }
            Block::Details { summary, body } => {
                let key = hash64(&[&"details", summary, body]);
                let open = self.open.contains(&key);
                let f = self.base(1.0, color, 600.0);
                let chevron = self.base(0.9, pal.muted, 400.0);
                let ch = self.plain_text(if open { icons::CHEVRON_DOWN } else { icons::CHEVRON_RIGHT }, chevron, x, y, em * 1.4, out);
                let mut h = self.text(summary, &f, x + em * 1.5, y, w - em * 1.5, Align::Min, out).max(ch);
                out.push(Draw::Toggle { rect: Rect::from_min_size(pos2(x, y), vec2(w, h)), key });
                if open {
                    h += em * 0.6;
                    h += self.blocks(body, x + em * 1.5, y + h, w - em * 1.5, color, out);
                }
                h
            }
            Block::Footnote { index, body, .. } => {
                self.anchors.push((format!("fn-{index}"), y));
                let f = self.base(0.88, pal.muted, 500.0);
                self.plain_text(&format!("{index}."), f, x, y, em * 2.0, out);
                let saved = self.size;
                self.size = saved * 0.9;
                let h = self.blocks(body, x + em * 2.0, y, w - em * 2.0, pal.muted, out);
                self.size = saved;
                h.max(em * 1.4)
            }
            Block::DefList(items) => {
                let mut cy = y;
                for (i, (term, details)) in items.iter().enumerate() {
                    if i > 0 {
                        cy += em * 0.7;
                    }
                    let f = self.base(1.0, color, 650.0);
                    cy += self.text(term, &f, x, cy, w, Align::Min, out);
                    for d in details {
                        cy += em * 0.2;
                        cy += self.blocks(d, x + em * 1.5, cy, w - em * 1.5, pal.muted, out);
                    }
                }
                cy - y
            }
            Block::FrontMatter(rows) => {
                if rows.is_empty() {
                    return 0.0;
                }
                let idx = out.len();
                let pad = em * 0.85;
                let kf = TextFormat { font_id: FontId::proportional(em * 0.8), color: pal.muted, coords: theme::weight(600.0), ..Default::default() };
                let vf = TextFormat { font_id: FontId::proportional(em * 0.88), color, ..Default::default() };
                let key_w = rows.iter().map(|r| r.0.chars().count()).max().unwrap_or(4).min(18) as f32 * em * 0.5 + em * 1.2;
                let mut h = pad * 0.8;
                for (k, v) in rows {
                    let a = self.plain_text(&k.to_uppercase(), kf.clone(), x + pad, y + h + 1.5, key_w, out);
                    let b = self.plain_text(v, vf.clone(), x + pad + key_w, y + h, w - pad * 2.0 - key_w, out);
                    h += a.max(b) + em * 0.3;
                }
                h += pad * 0.5;
                out.insert(idx, Draw::Rect { rect: Rect::from_min_size(pos2(x, y), vec2(w, h)), fill: pal.doc_subtle, stroke: Stroke::new(1.0, pal.doc_border), radius: 8 });
                h
            }
            Block::Center(body) => {
                let mut cy = y;
                for (i, b) in body.iter().enumerate() {
                    if i > 0 {
                        cy += em * 0.6;
                    }
                    cy += match b {
                        Block::Para(inl) => {
                            let f = self.base(1.0, color, 400.0);
                            self.text(inl, &f, x, cy, w, Align::Center, out)
                        }
                        Block::Heading { level, inl, id } => {
                            let scale = [2.05, 1.55, 1.27, 1.1, 1.0, 0.88][(*level as usize).clamp(1, 6) - 1];
                            let mut f = self.base(scale, color, 700.0);
                            f.line_height = Some((f.font_id.size * 1.3).round());
                            self.anchors.push((id.clone(), cy));
                            self.text(inl, &f, x, cy, w, Align::Center, out)
                        }
                        Block::Image { .. } => {
                            let start = out.len();
                            let h = self.block(b, x, cy, w, color, out);
                            // centre what was just laid out
                            let right = out[start..].iter().filter_map(|d| if let Draw::Image { rect, .. } = d { Some(rect.max.x) } else { None }).fold(x, f32::max);
                            let shift = (w - (right - x)) / 2.0;
                            for d in &mut out[start..] {
                                if let Draw::Image { rect, .. } = d {
                                    *rect = rect.translate(vec2(shift, 0.0));
                                }
                            }
                            h
                        }
                        other => self.block(other, x, cy, w, color, out),
                    };
                }
                cy - y
            }
        }
    }

    fn item_blocks(&mut self, blocks: &[Block], x: f32, y: f32, w: f32, color: Color32, tight: bool, out: &mut Vec<Draw>) -> f32 {
        let mut cy = y;
        for (i, b) in blocks.iter().enumerate() {
            if i > 0 {
                cy += self.size * if tight || matches!(b, Block::List { .. }) { 0.28 } else { 0.7 };
            }
            cy += self.block(b, x, cy, w, color, out);
        }
        cy - y
    }

    fn code(&mut self, lang: &str, text: &str, x: f32, y: f32, w: f32, out: &mut Vec<Draw>) -> f32 {
        let pal = *self.pal;
        let em = self.size;
        let size = (em * 0.86 * 4.0).round() / 4.0;
        let pad = em * 0.95;
        let mut job = LayoutJob::default();
        job.wrap.max_width = (w - pad * 2.0).max(60.0);
        let spans = crate::syntax::highlight_snippet(lang, text);
        for (line, sp) in text.split_inclusive('\n').zip(spans.iter()) {
            let mut last = 0usize;
            for (end, tok) in sp.iter() {
                let end = (*end as usize).min(line.len());
                if end > last && line.is_char_boundary(last) && line.is_char_boundary(end) {
                    let mut f = TextFormat { font_id: FontId::monospace(size), color: pal.token(*tok), line_height: Some((size * 1.55).round()), ..Default::default() };
                    if *tok == theme::TOK_COMMENT {
                        f.font_id.family = FontFamily::Monospace;
                    }
                    job.append(&line[last..end].replace('\t', "    "), 0.0, f);
                    last = end;
                }
            }
        }
        if job.sections.is_empty() {
            job.append(" ", 0.0, TextFormat { font_id: FontId::monospace(size), color: pal.doc_text, ..Default::default() });
        }
        let galley = self.ui.fonts_mut(|f| f.layout_job(job));
        let h = galley.size().y + pad * 1.7;
        let rect = Rect::from_min_size(pos2(x, y), vec2(w, h));
        out.push(Draw::Rect { rect, fill: pal.doc_code, stroke: Stroke::new(1.0, theme::mix(pal.doc_code, pal.doc_border, 0.6)), radius: 8 });
        if !lang.is_empty() {
            let lf = TextFormat { font_id: FontId::proportional(em * 0.68), color: pal.faint, coords: theme::weight(600.0), extra_letter_spacing: 0.5, ..Default::default() };
            let g = self.ui.fonts_mut(|fonts| {
                let mut j = LayoutJob::default();
                j.append(&lang.to_uppercase(), 0.0, lf);
                fonts.layout_job(j)
            });
            out.push(Draw::Text { pos: pos2(x + w - g.size().x - pad * 0.8, y + em * 0.4), galley: g, links: Vec::new() });
        }
        out.push(Draw::Text { pos: pos2(x + pad, y + pad * 0.85), galley, links: Vec::new() });
        out.push(Draw::Copy { rect, text: text.into() });
        h
    }

    fn diagram(&mut self, kind: DiagramKind, src: &str, x: f32, y: f32, w: f32, out: &mut Vec<Draw>) -> f32 {
        let pal = *self.pal;
        let em = self.size;
        let dark = pal.dark;
        let ppp = self.ppp;
        let max_w = w - em * 1.6;
        let key = hash64(&[&"diagram", &(kind as u8), &src, &dark, &((max_w / 32.0) as u32), &((ppp * 100.0) as u32)]);
        let s = src.to_string();
        let base = self.base.clone();
        let state = self.assets.get(key, || match kind {
            DiagramKind::Mermaid => assets::job_mermaid(s, dark, ppp, max_w),
            DiagramKind::Dot => assets::job_dot(s, ppp, max_w),
            DiagramKind::Svg => assets::job_svg(s, ppp, max_w, base),
            DiagramKind::Chart => assets::job_chart(s, dark, ppp, max_w),
        });
        match state {
            State::Ready(a) => {
                let sz = a.size;
                let pad = em * 0.8;
                let h = sz.y + pad * 2.0;
                // Graphviz and raw SVG assume a light page
                let light_card = dark && matches!(kind, DiagramKind::Dot | DiagramKind::Svg);
                let card = Rect::from_min_size(pos2(x, y), vec2(w, h));
                out.push(Draw::Rect { rect: card, fill: if light_card { theme::rgb(0xf4f5f7) } else { pal.doc_subtle }, stroke: Stroke::new(1.0, theme::mix(pal.doc_subtle, pal.doc_border, 0.5)), radius: 10 });
                out.push(Draw::Image { rect: Rect::from_min_size(pos2(x + (w - sz.x) / 2.0, y + pad), sz), key, radius: 0, link: None, zoom: true, card: true });
                h
            }
            State::Pending => {
                self.pending = true;
                let h = em * 9.0;
                out.push(Draw::Rect { rect: Rect::from_min_size(pos2(x, y), vec2(w, h)), fill: pal.doc_subtle, stroke: Stroke::NONE, radius: 10 });
                let f = self.base(0.85, pal.faint, 400.0);
                self.text(&[Inl::Text("Rendering…".into(), Style::default())], &f, x, y + h / 2.0 - em * 0.7, w, Align::Center, out);
                h
            }
            State::Failed(e) => {
                let idx = out.len();
                let pad = em * 0.9;
                let mut h = pad * 0.8;
                let tf = self.base(0.9, pal.caution, 650.0);
                let name = match kind {
                    DiagramKind::Mermaid => "Mermaid",
                    DiagramKind::Dot => "Graphviz",
                    DiagramKind::Svg => "SVG",
                    DiagramKind::Chart => "Chart",
                };
                h += self.plain_text(&format!("{}  {name} could not be drawn", icons::TRIANGLE_ALERT), tf, x + pad, y + h, w - pad * 2.0, out);
                let ef = self.base(0.82, pal.muted, 400.0);
                h += em * 0.2;
                h += self.plain_text(e.lines().next().unwrap_or(""), ef, x + pad, y + h, w - pad * 2.0, out);
                let cf = TextFormat { font_id: FontId::monospace(em * 0.8), color: pal.muted, ..Default::default() };
                h += em * 0.5;
                let shown: String = src.lines().take(12).collect::<Vec<_>>().join("\n");
                h += self.plain_text(&shown, cf, x + pad, y + h, w - pad * 2.0, out);
                h += pad * 0.8;
                out.insert(idx, Draw::Rect { rect: Rect::from_min_size(pos2(x, y), vec2(w, h)), fill: theme::mix(pal.doc_bg, pal.caution, 0.07), stroke: Stroke::new(1.0, theme::mix(pal.doc_bg, pal.caution, 0.35)), radius: 8 });
                h
            }
        }
    }

    fn table(&mut self, aligns: &[CellAlign], rows: &[Vec<Vec<Inl>>], x: f32, y: f32, w: f32, color: Color32, out: &mut Vec<Draw>) -> f32 {
        let pal = *self.pal;
        let em = self.size;
        let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
        if cols == 0 {
            return 0.0;
        }
        let (px, py) = (em * 0.8, em * 0.42);
        let body = self.base(0.95, color, 400.0);
        let head = self.base(0.95, color, 650.0);
        // natural widths
        let mut natural = vec![em * 2.0; cols];
        for (ri, row) in rows.iter().enumerate().take(400) {
            for (ci, cell) in row.iter().enumerate() {
                let t = self.inline(cell, if ri == 0 { &head } else { &body }, 100_000.0, Align::Min);
                natural[ci] = natural[ci].max(t.galley.size().x + 1.0);
            }
        }
        let avail = w - cols as f32 * px * 2.0;
        let total: f32 = natural.iter().sum();
        let widths: Vec<f32> = if total <= avail {
            natural.clone()
        } else {
            // shrink the wide columns first; narrow ones keep their natural width
            let mut wd = natural.clone();
            let fair = avail / cols as f32;
            let small: f32 = wd.iter().filter(|v| **v <= fair).sum();
            let n_big = wd.iter().filter(|v| **v > fair).count().max(1);
            let big_total: f32 = wd.iter().filter(|v| **v > fair).sum();
            let room = (avail - small).max(n_big as f32 * em * 3.0);
            for v in &mut wd {
                if *v > fair {
                    *v = (*v / big_total * room).max(em * 3.0);
                }
            }
            wd
        };
        let table_w = widths.iter().sum::<f32>() + cols as f32 * px * 2.0;
        let idx = out.len();
        let mut cy = y;
        let mut row_tops = Vec::with_capacity(rows.len() + 1);
        for (ri, row) in rows.iter().enumerate() {
            row_tops.push(cy);
            let mut cx = x;
            let mut rh = em * 1.2;
            for ci in 0..cols {
                let cw = widths[ci];
                if let Some(cell) = row.get(ci) {
                    let align = match aligns.get(ci) {
                        Some(CellAlign::Center) => Align::Center,
                        Some(CellAlign::Right) => Align::Max,
                        _ => Align::Min,
                    };
                    let h = self.text(cell, if ri == 0 { &head } else { &body }, cx + px, cy + py, cw, align, out);
                    rh = rh.max(h);
                }
                cx += cw + px * 2.0;
            }
            cy += rh + py * 2.0;
        }
        row_tops.push(cy);
        let h = cy - y;
        // backgrounds and rules go behind the text
        let mut bg = Vec::new();
        let outer = Rect::from_min_size(pos2(x, y), vec2(table_w, h));
        bg.push(Draw::Rect { rect: outer, fill: pal.doc_bg, stroke: Stroke::NONE, radius: 8 });
        if rows.len() > 1 {
            bg.push(Draw::Rect { rect: Rect::from_min_max(pos2(x, y), pos2(x + table_w, row_tops[1])), fill: pal.doc_subtle, stroke: Stroke::NONE, radius: 8 });
        }
        for ri in 2..rows.len() {
            if ri % 2 == 0 {
                bg.push(Draw::Rect { rect: Rect::from_min_max(pos2(x, row_tops[ri]), pos2(x + table_w, row_tops[ri + 1])), fill: theme::mix(pal.doc_bg, pal.doc_subtle, 0.55), stroke: Stroke::NONE, radius: 0 });
            }
        }
        let rule = Stroke::new(1.0, theme::mix(pal.doc_bg, pal.doc_border, 0.7));
        for top in row_tops.iter().skip(1).take(rows.len().saturating_sub(1)) {
            bg.push(Draw::Line { a: pos2(x, *top), b: pos2(x + table_w, *top), stroke: rule });
        }
        let mut cx = x;
        for cw in widths.iter().take(cols - 1) {
            cx += cw + px * 2.0;
            bg.push(Draw::Line { a: pos2(cx, y), b: pos2(cx, y + h), stroke: rule });
        }
        bg.push(Draw::Rect { rect: outer, fill: Color32::TRANSPARENT, stroke: Stroke::new(1.0, pal.doc_border), radius: 8 });
        out.splice(idx..idx, bg);
        h
    }
}

fn width_cap(column: f32) -> f32 {
    column
}

/// Lay out one top-level block at width `w`.
pub fn layout_top(lay: &mut Lay<'_>, block: &Block, w: f32) -> Laid {
    lay.pending = false;
    lay.anchors.clear();
    let mut items = Vec::new();
    let color = lay.pal.doc_text;
    let height = lay.block(block, 0.0, 0.0, w, color, &mut items);
    Laid { height, items, pending: lay.pending, anchors: std::mem::take(&mut lay.anchors) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_blocks() {
        let tops = parse("---\ntitle: Hi\ntags: [a, b]\n---\n\n# Head *one*\n\nText with **bold**, `code`, <kbd>Ctrl</kbd> and $x^2$.\n\n- [x] done\n- [ ] todo\n\n| a | b |\n|:-:|--:|\n| 1 | 2 |\n\n> [!TIP]\n> Nice\n\n```mermaid\ngraph TD; A-->B\n```\n\n```rust\nfn main() {}\n```\n\n![alt](pic.png \"Title\")\n\nNote[^1].\n\n[^1]: The note.\n");
        let kinds: Vec<&str> = tops
            .iter()
            .map(|t| match &t.block {
                Block::FrontMatter(_) => "fm",
                Block::Heading { .. } => "h",
                Block::Para(_) => "p",
                Block::List { .. } => "list",
                Block::Table { .. } => "table",
                Block::Alert { .. } => "alert",
                Block::Diagram { .. } => "diagram",
                Block::Code { .. } => "code",
                Block::Image { .. } => "image",
                Block::Footnote { .. } => "foot",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, vec!["fm", "h", "p", "list", "table", "alert", "diagram", "code", "image", "p", "foot"]);
        match &tops[0].block {
            Block::FrontMatter(rows) => assert_eq!(rows[0], ("title".to_string(), "Hi".to_string())),
            _ => panic!(),
        }
        match &tops[1].block {
            Block::Heading { level, id, .. } => assert_eq!((*level, id.as_str()), (1, "head-one")),
            _ => panic!(),
        }
        match &tops[2].block {
            Block::Para(inl) => {
                assert!(inl.iter().any(|i| matches!(i, Inl::Text(t, s) if t == "bold" && s.bold)));
                assert!(inl.iter().any(|i| matches!(i, Inl::Text(t, s) if t == "Ctrl" && s.kbd)));
                assert!(inl.iter().any(|i| matches!(i, Inl::Math(m) if m == "x^2")));
            }
            _ => panic!(),
        }
        match &tops[3].block {
            Block::List { items, .. } => {
                assert_eq!(items[0].task, Some(true));
                assert_eq!(items[1].task, Some(false));
                assert_eq!(items[1].rel_line, 1);
            }
            _ => panic!(),
        }
        assert_eq!(tops[3].line0, 10);
        assert_eq!(outline(&tops).len(), 1);
    }

    #[test]
    fn html_fragments() {
        let tops = parse("<details><summary>More</summary>\n\nHidden **text**\n\n</details>\n\n<p align=\"center\"><img src=\"a.png\" width=\"120\"></p>\n\nA<br>B &amp; C<sup>2</sup>\n");
        assert!(matches!(&tops[0].block, Block::Details { .. }) || matches!(&tops[0].block, Block::Para(_)));
        assert!(tops.iter().any(|t| matches!(&t.block, Block::Center(b) if matches!(b.first(), Some(Block::Image { width: Some(120), .. })))));
        assert_eq!(decode_entities("a &amp; b &#65; &lt;"), "a & b A <");
        assert_eq!(slug("Hello, World 2!"), "hello-world-2");
    }
}
