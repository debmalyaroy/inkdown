//! Word (.docx) and Rich Text (.rtf) export.
//!
//! The Markdown AST is flattened into a small document model (`Doc`), which two
//! writers turn into WordprocessingML and RTF. Graphics that only the webview can
//! draw (diagrams, charts, math, decoded images) arrive pre-rendered in `Assets`.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Write as _;
use std::rc::Rc;

use comrak::nodes::{AlertType, AstNode, ListType, NodeValue, TableAlignment};
use comrak::{parse_document, Anchorizer, Arena};

#[derive(Default)]
pub struct Img {
    pub jpeg: bool,
    pub data: Vec<u8>,
    /// Display size in CSS pixels.
    pub w: f32,
    pub h: f32,
    /// Distance from the text baseline to the bottom of the image (inline math).
    pub depth: f32,
}

#[derive(Default)]
pub struct Assets {
    pub diagrams: HashMap<String, Rc<Img>>,
    pub math: HashMap<String, Rc<Img>>,
    pub images: HashMap<String, Rc<Img>>,
}

#[derive(Clone, Copy, Default, PartialEq)]
struct Fmt {
    b: bool,
    i: bool,
    strike: bool,
    u: bool,
    code: bool,
    sup: bool,
    sub: bool,
    mark: bool,
}

enum Inline {
    Text(String, Fmt),
    Break,
    Image(Rc<Img>, String),
    Link(String, Vec<Inline>),
}

#[derive(Clone, Default)]
struct Ctx {
    quote: u8,
    /// List nesting depth (0 = not in a list).
    level: u8,
    accent: Option<&'static str>,
}

enum Marker {
    None,
    Bullet,
    Number { list: usize, n: usize },
    Task(bool),
}

enum Block {
    Heading { level: u8, anchor: String, inl: Vec<Inline> },
    Para { inl: Vec<Inline>, ctx: Ctx, marker: Marker, center: bool },
    Code { text: String, ctx: Ctx },
    Rule,
    Table { aligns: Vec<TableAlignment>, rows: Vec<Vec<Vec<Inline>>>, ctx: Ctx },
}

pub struct Doc {
    title: String,
    blocks: Vec<Block>,
    /// Start number of each ordered list, indexed by `Marker::Number::list`.
    ordered: Vec<usize>,
}

const DIAGRAM_LANGS: &[&str] = &["mermaid", "mmd", "dot", "graphviz", "echarts", "chart", "svg"];
const PAGE_PX: f32 = 600.0; // usable width of A4 with 1" margins, in CSS px

// ------------------------------------------------------------------ builder

struct Builder<'a> {
    assets: &'a Assets,
    doc: Doc,
    anchors: Anchorizer,
    pending: Option<Marker>,
    bold_all: bool,
    notes_started: bool,
    note_no: usize,
}

fn plain_text<'a>(node: &'a AstNode<'a>, out: &mut String) {
    match &node.data.borrow().value {
        NodeValue::Text(t) => out.push_str(t),
        NodeValue::Code(c) => out.push_str(&c.literal),
        NodeValue::Math(m) => out.push_str(&m.literal),
        NodeValue::SoftBreak | NodeValue::LineBreak => out.push(' '),
        _ => {}
    }
    for c in node.children() {
        plain_text(c, out);
    }
}

fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let end = if tail.starts_with("<!--") {
            tail.find("-->").map(|e| e + 3)
        } else {
            tail.find('>').map(|e| e + 1)
        };
        match end {
            Some(e) => {
                let tag = tail[..e].to_ascii_lowercase();
                if tag.starts_with("<br") || tag.starts_with("</p") || tag.starts_with("</div") || tag.starts_with("</summary") || tag.starts_with("</li") {
                    out.push('\n');
                }
                rest = &tail[e..];
            }
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out.replace("&nbsp;", " ").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&amp;", "&")
}

impl<'a> Builder<'a> {
    fn flush_marker(&mut self, ctx: &Ctx) {
        if let Some(m) = self.pending.take() {
            self.doc.blocks.push(Block::Para { inl: vec![], ctx: ctx.clone(), marker: m, center: false });
        }
    }

    fn para(&mut self, inl: Vec<Inline>, ctx: &Ctx) {
        let marker = self.pending.take().unwrap_or(Marker::None);
        let center = matches!(marker, Marker::None)
            && inl.len() == 1
            && matches!(&inl[0], Inline::Image(img, _) if img.depth == 0.0 && img.w > 120.0);
        self.doc.blocks.push(Block::Para { inl, ctx: ctx.clone(), marker, center });
    }

    fn image(&self, img: &Rc<Img>, alt: String) -> Inline {
        Inline::Image(img.clone(), alt)
    }

    fn inlines<'n>(&mut self, node: &'n AstNode<'n>, base: Fmt, out: &mut Vec<Inline>) {
        let mut fmt = base;
        for child in node.children() {
            let value = child.data.borrow().value.clone();
            match value {
                NodeValue::Text(t) => out.push(Inline::Text(t.to_string(), fmt)),
                NodeValue::SoftBreak => out.push(Inline::Text(" ".into(), fmt)),
                NodeValue::LineBreak => out.push(Inline::Break),
                NodeValue::Code(c) => out.push(Inline::Text(c.literal, Fmt { code: true, ..fmt })),
                NodeValue::Emph => self.inlines(child, Fmt { i: true, ..fmt }, out),
                NodeValue::Strong => self.inlines(child, Fmt { b: true, ..fmt }, out),
                NodeValue::Strikethrough => self.inlines(child, Fmt { strike: true, ..fmt }, out),
                NodeValue::Highlight => self.inlines(child, Fmt { mark: true, ..fmt }, out),
                NodeValue::Insert | NodeValue::Underline => self.inlines(child, Fmt { u: true, ..fmt }, out),
                NodeValue::Superscript => self.inlines(child, Fmt { sup: true, ..fmt }, out),
                NodeValue::Subscript => self.inlines(child, Fmt { sub: true, ..fmt }, out),
                NodeValue::Link(l) => {
                    let mut kids = Vec::new();
                    self.inlines(child, fmt, &mut kids);
                    out.push(Inline::Link(l.url, kids));
                }
                NodeValue::WikiLink(w) => {
                    let mut kids = Vec::new();
                    self.inlines(child, fmt, &mut kids);
                    out.push(Inline::Link(w.url, kids));
                }
                NodeValue::Image(l) => {
                    let mut alt = String::new();
                    plain_text(child, &mut alt);
                    match self.assets.images.get(&l.url) {
                        Some(img) => out.push(self.image(img, alt)),
                        None => {
                            let label = if alt.is_empty() { l.url.clone() } else { alt };
                            out.push(Inline::Link(l.url, vec![Inline::Text(format!("[image: {label}]"), fmt)]));
                        }
                    }
                }
                NodeValue::Math(m) => {
                    let key = format!("{}|{}", m.display_math as u8, m.literal.trim());
                    match self.assets.math.get(&key) {
                        Some(img) => out.push(self.image(img, m.literal.trim().to_string())),
                        None => out.push(Inline::Text(m.literal, Fmt { code: true, ..fmt })),
                    }
                }
                NodeValue::FootnoteReference(r) => out.push(Inline::Text(r.ix.to_string(), Fmt { sup: true, ..fmt })),
                NodeValue::ShortCode(sc) => out.push(Inline::Text(sc.emoji, fmt)),
                NodeValue::HtmlInline(tag) => {
                    let t = tag.to_ascii_lowercase();
                    let t = t.trim_start_matches('<').trim_end_matches('>').trim_end_matches('/').trim();
                    let (close, name) = match t.strip_prefix('/') {
                        Some(n) => (true, n.trim()),
                        None => (false, t.split_whitespace().next().unwrap_or("")),
                    };
                    match name {
                        "br" => out.push(Inline::Break),
                        "kbd" | "code" | "samp" => fmt.code = !close || base.code,
                        "b" | "strong" => fmt.b = !close || base.b,
                        "i" | "em" => fmt.i = !close || base.i,
                        "u" | "ins" => fmt.u = !close || base.u,
                        "s" | "del" | "strike" => fmt.strike = !close || base.strike,
                        "sub" => fmt.sub = !close || base.sub,
                        "sup" => fmt.sup = !close || base.sup,
                        "mark" => fmt.mark = !close || base.mark,
                        _ => {}
                    }
                }
                _ => self.inlines(child, fmt, out),
            }
        }
    }

    fn inline_vec<'n>(&mut self, node: &'n AstNode<'n>) -> Vec<Inline> {
        let mut out = Vec::new();
        let base = Fmt { b: self.bold_all, ..Fmt::default() };
        self.inlines(node, base, &mut out);
        out
    }

    fn children<'n>(&mut self, node: &'n AstNode<'n>, ctx: &Ctx) {
        for c in node.children() {
            self.block(c, ctx);
        }
    }

    fn block<'n>(&mut self, node: &'n AstNode<'n>, ctx: &Ctx) {
        let value = node.data.borrow().value.clone();
        match value {
            NodeValue::FrontMatter(fm) => {
                for line in fm.lines() {
                    if let Some(t) = line.strip_prefix("title:") {
                        self.doc.title = t.trim().trim_matches(|c| c == '"' || c == '\'').to_string();
                    }
                }
            }
            NodeValue::Paragraph => {
                let inl = self.inline_vec(node);
                self.para(inl, ctx);
            }
            NodeValue::Heading(h) => {
                self.flush_marker(ctx);
                let mut text = String::new();
                plain_text(node, &mut text);
                let anchor = self.anchors.anchorize(&text);
                let inl = self.inline_vec(node);
                self.doc.blocks.push(Block::Heading { level: h.level.clamp(1, 6), anchor, inl });
            }
            NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) => {
                let inner = Ctx { quote: ctx.quote + 1, ..ctx.clone() };
                self.children(node, &inner);
            }
            NodeValue::Alert(a) => {
                let color = match a.alert_type {
                    AlertType::Note => "0969DA",
                    AlertType::Tip => "1A7F37",
                    AlertType::Important => "8250DF",
                    AlertType::Warning => "9A6700",
                    AlertType::Caution => "CF222E",
                };
                let inner = Ctx { quote: ctx.quote + 1, accent: Some(color), ..ctx.clone() };
                let title = a.title.clone().unwrap_or_else(|| a.alert_type.default_title().to_string());
                self.para(vec![Inline::Text(title, Fmt { b: true, ..Fmt::default() })], &inner);
                self.children(node, &inner);
            }
            NodeValue::List(list) => {
                let ordered = list.list_type == ListType::Ordered;
                let list_id = self.doc.ordered.len();
                if ordered {
                    self.doc.ordered.push(list.start.max(1));
                }
                let inner = Ctx { level: (ctx.level + 1).min(9), ..ctx.clone() };
                let mut n = list.start.max(1);
                for item in node.children() {
                    let task = match &item.data.borrow().value {
                        NodeValue::TaskItem(t) => Some(t.symbol.is_some()),
                        _ => None,
                    };
                    self.flush_marker(ctx);
                    self.pending = Some(match task {
                        Some(done) => Marker::Task(done),
                        None if ordered => Marker::Number { list: list_id, n },
                        None => Marker::Bullet,
                    });
                    self.children(item, &inner);
                    self.flush_marker(&inner);
                    n += 1;
                }
            }
            NodeValue::CodeBlock(cb) => {
                let lang = cb.info.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
                let body = cb.literal.trim();
                let asset = if DIAGRAM_LANGS.contains(&lang.as_str()) {
                    self.assets.diagrams.get(body)
                } else if lang == "math" {
                    self.assets.math.get(&format!("1|{body}"))
                } else {
                    None
                };
                match asset {
                    Some(img) => {
                        let inl = vec![self.image(img, lang)];
                        self.para(inl, ctx);
                    }
                    None => {
                        self.flush_marker(ctx);
                        self.doc.blocks.push(Block::Code { text: cb.literal.trim_end_matches('\n').to_string(), ctx: ctx.clone() });
                    }
                }
            }
            NodeValue::HtmlBlock(h) => {
                for chunk in strip_tags(&h.literal).split("\n") {
                    let chunk = chunk.trim();
                    if !chunk.is_empty() {
                        self.para(vec![Inline::Text(chunk.to_string(), Fmt::default())], ctx);
                    }
                }
            }
            NodeValue::ThematicBreak => {
                self.flush_marker(ctx);
                self.doc.blocks.push(Block::Rule);
            }
            NodeValue::Table(t) => {
                self.flush_marker(ctx);
                let mut rows = Vec::new();
                for row in node.children() {
                    let mut cells = Vec::new();
                    for cell in row.children() {
                        cells.push(self.inline_vec(cell));
                    }
                    rows.push(cells);
                }
                self.doc.blocks.push(Block::Table { aligns: t.alignments.clone(), rows, ctx: ctx.clone() });
            }
            NodeValue::FootnoteDefinition(_) => {
                if !self.notes_started {
                    self.notes_started = true;
                    self.doc.blocks.push(Block::Rule);
                }
                self.note_no += 1;
                let start = self.doc.blocks.len();
                self.children(node, ctx);
                let label = Inline::Text(format!("{}  ", self.note_no), Fmt { sup: true, ..Fmt::default() });
                match self.doc.blocks.get_mut(start) {
                    Some(Block::Para { inl, .. }) => inl.insert(0, label),
                    _ => self.doc.blocks.insert(start, Block::Para { inl: vec![label], ctx: ctx.clone(), marker: Marker::None, center: false }),
                }
            }
            NodeValue::DescriptionTerm => {
                self.bold_all = true;
                self.children(node, ctx);
                self.bold_all = false;
            }
            NodeValue::DescriptionDetails => {
                let inner = Ctx { level: (ctx.level + 1).min(9), ..ctx.clone() };
                self.children(node, &inner);
            }
            _ => self.children(node, ctx),
        }
    }
}

pub fn build(markdown: &str, assets: &Assets, fallback_title: &str) -> Doc {
    let opts = crate::render::options();
    let arena = Arena::new();
    let root = parse_document(&arena, markdown, &opts);
    let mut b = Builder {
        assets,
        doc: Doc { title: String::new(), blocks: Vec::new(), ordered: Vec::new() },
        anchors: Anchorizer::new(),
        pending: None,
        bold_all: false,
        notes_started: false,
        note_no: 0,
    };
    b.children(root, &Ctx::default());
    if b.doc.title.is_empty() {
        b.doc.title = fallback_title.to_string();
    }
    b.doc
}

/// Scale an image to fit the available width, returning CSS px.
fn fit(img: &Img, level: u8, quote: u8) -> (f32, f32) {
    let max = (PAGE_PX - 48.0 * level as f32 - 20.0 * quote as f32).max(160.0);
    let (w, h) = (img.w.max(1.0), img.h.max(1.0));
    if w > max { (max, h * max / w) } else { (w, h) }
}

// --------------------------------------------------------------------- DOCX

fn xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            c => out.push(c),
        }
    }
    out
}

struct Docx<'d> {
    body: String,
    rels: String,
    next_rel: usize,
    media: Vec<(String, &'d Img)>,
    media_ids: HashMap<*const Img, String>,
    links: HashMap<String, String>,
    bookmarks: HashMap<String, String>,
    next_id: usize,
    force_bold: bool,
}

fn rpr(f: Fmt, link: bool, color: Option<&str>, position: i32) -> String {
    let mut s = String::new();
    if f.code {
        s.push_str(r#"<w:rStyle w:val="InlineCode"/>"#);
    } else if link {
        s.push_str(r#"<w:rStyle w:val="Hyperlink"/>"#);
    }
    if f.b { s.push_str("<w:b/>"); }
    if f.i { s.push_str("<w:i/>"); }
    if f.strike { s.push_str("<w:strike/>"); }
    if let (Some(c), false) = (color, link) { let _ = write!(s, r#"<w:color w:val="{c}"/>"#); }
    if position != 0 { let _ = write!(s, r#"<w:position w:val="{position}"/>"#); }
    if f.mark { s.push_str(r#"<w:highlight w:val="yellow"/>"#); }
    if f.u { s.push_str(r#"<w:u w:val="single"/>"#); }
    if f.sup { s.push_str(r#"<w:vertAlign w:val="superscript"/>"#); }
    if f.sub { s.push_str(r#"<w:vertAlign w:val="subscript"/>"#); }
    if s.is_empty() { s } else { format!("<w:rPr>{s}</w:rPr>") }
}

fn text_run(out: &mut String, text: &str, props: &str) {
    if text.is_empty() {
        return;
    }
    let _ = write!(out, "<w:r>{props}");
    let mut buf = String::new();
    let flush = |buf: &mut String, out: &mut String| {
        if !buf.is_empty() {
            let _ = write!(out, r#"<w:t xml:space="preserve">{}</w:t>"#, xml(buf));
            buf.clear();
        }
    };
    for c in text.chars() {
        match c {
            '\n' => { flush(&mut buf, out); out.push_str("<w:br/>"); }
            '\t' => { flush(&mut buf, out); out.push_str("<w:tab/>"); }
            '\r' => {}
            c => buf.push(c),
        }
    }
    flush(&mut buf, out);
    out.push_str("</w:r>");
}

impl<'d> Docx<'d> {
    fn rel(&mut self, kind: &str, target: &str, external: bool) -> String {
        let id = format!("rId{}", self.next_rel);
        self.next_rel += 1;
        let _ = write!(
            self.rels,
            r#"<Relationship Id="{id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}" Target="{}"{}/>"#,
            xml(target),
            if external { r#" TargetMode="External""# } else { "" }
        );
        id
    }

    fn picture(&mut self, out: &mut String, img: &'d Rc<Img>, alt: &str, ctx: &Ctx) {
        let key = Rc::as_ptr(img);
        let rid = match self.media_ids.get(&key) {
            Some(r) => r.clone(),
            None => {
                let name = format!("image{}.{}", self.media.len() + 1, if img.jpeg { "jpeg" } else { "png" });
                let rid = self.rel("image", &format!("media/{name}"), false);
                self.media.push((name, img.as_ref()));
                self.media_ids.insert(key, rid.clone());
                rid
            }
        };
        let (w, h) = fit(img, ctx.level, ctx.quote);
        let (cx, cy) = ((w * 9525.0) as i64, (h * 9525.0) as i64);
        self.next_id += 1;
        let id = self.next_id;
        let position = -(img.depth * 1.5).round() as i32;
        let _ = write!(
            out,
            r#"<w:r>{}<w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="{cx}" cy="{cy}"/><wp:docPr id="{id}" name="Picture {id}" descr="{}"/><wp:cNvGraphicFramePr><a:graphicFrameLocks xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" noChangeAspect="1"/></wp:cNvGraphicFramePr><a:graphic xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:nvPicPr><pic:cNvPr id="{id}" name="Picture {id}"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="{rid}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#,
            if position != 0 { format!(r#"<w:rPr><w:position w:val="{position}"/></w:rPr>"#) } else { String::new() },
            xml(alt)
        );
    }

    fn runs(&mut self, out: &mut String, inl: &'d [Inline], ctx: &Ctx, link: bool, color: Option<&str>) {
        for item in inl {
            match item {
                Inline::Text(t, f) => text_run(out, t, &rpr(Fmt { b: f.b || self.force_bold, ..*f }, link, color, 0)),
                Inline::Break => out.push_str("<w:r><w:br/></w:r>"),
                Inline::Image(img, alt) => self.picture(out, img, alt, ctx),
                Inline::Link(url, kids) => {
                    if let Some(slug) = url.strip_prefix('#') {
                        match self.bookmarks.get(slug).cloned() {
                            Some(name) => {
                                let _ = write!(out, r#"<w:hyperlink w:anchor="{name}" w:history="1">"#);
                                self.runs(out, kids, ctx, true, color);
                                out.push_str("</w:hyperlink>");
                            }
                            None => self.runs(out, kids, ctx, false, color),
                        }
                    } else {
                        let rid = match self.links.get(url) {
                            Some(r) => r.clone(),
                            None => {
                                let r = self.rel("hyperlink", url, true);
                                self.links.insert(url.clone(), r.clone());
                                r
                            }
                        };
                        let _ = write!(out, r#"<w:hyperlink r:id="{rid}" w:history="1">"#);
                        self.runs(out, kids, ctx, true, color);
                        out.push_str("</w:hyperlink>");
                    }
                }
            }
        }
    }

    /// Paragraph properties shared by text, code and table-adjacent blocks.
    fn ppr(ctx: &Ctx, marker: &Marker, style: Option<&str>, center: bool, extra: &str) -> String {
        let mut s = String::new();
        if let Some(st) = style {
            let _ = write!(s, r#"<w:pStyle w:val="{st}"/>"#);
        }
        let lvl = ctx.level.saturating_sub(1).min(8);
        match marker {
            Marker::Bullet => { let _ = write!(s, r#"<w:numPr><w:ilvl w:val="{lvl}"/><w:numId w:val="1"/></w:numPr>"#); }
            Marker::Number { list, .. } => { let _ = write!(s, r#"<w:numPr><w:ilvl w:val="{lvl}"/><w:numId w:val="{}"/></w:numPr>"#, list + 2); }
            _ => {}
        }
        if ctx.quote > 0 {
            let _ = write!(s, r#"<w:pBdr><w:left w:val="single" w:sz="18" w:space="10" w:color="{}"/></w:pBdr>"#, ctx.accent.unwrap_or("D0D7DE"));
        }
        s.push_str(extra);
        let left = 720 * ctx.level as u32 + 300 * ctx.quote as u32;
        let hanging = if matches!(marker, Marker::None) { 0 } else { 360 };
        if left > 0 || ctx.level > 0 {
            let _ = write!(s, r#"<w:ind w:left="{left}" w:hanging="{hanging}"/>"#);
        }
        if ctx.level > 0 && !matches!(marker, Marker::None) {
            s.push_str("<w:contextualSpacing/>");
        }
        if center {
            s.push_str(r#"<w:jc w:val="center"/>"#);
        }
        if s.is_empty() { s } else { format!("<w:pPr>{s}</w:pPr>") }
    }

    fn block(&mut self, b: &'d Block) {
        let mut out = String::new();
        match b {
            Block::Heading { level, anchor, inl } => {
                let _ = write!(out, r#"<w:p><w:pPr><w:pStyle w:val="Heading{level}"/></w:pPr>"#);
                let ctx = Ctx::default();
                if let Some(name) = self.bookmarks.get(anchor).cloned() {
                    self.next_id += 1;
                    let id = self.next_id;
                    let _ = write!(out, r#"<w:bookmarkStart w:id="{id}" w:name="{name}"/>"#);
                    self.runs(&mut out, inl, &ctx, false, None);
                    let _ = write!(out, r#"<w:bookmarkEnd w:id="{id}"/>"#);
                } else {
                    self.runs(&mut out, inl, &ctx, false, None);
                }
                out.push_str("</w:p>");
            }
            Block::Para { inl, ctx, marker, center } => {
                let spacing = if ctx.level > 0 { r#"<w:spacing w:after="60"/>"# } else { "" };
                let _ = write!(out, "<w:p>{}", Self::ppr(ctx, marker, None, *center, spacing));
                if let Marker::Task(done) = marker {
                    let _ = write!(
                        out,
                        r#"<w:r><w:rPr><w:rFonts w:ascii="Segoe UI Symbol" w:hAnsi="Segoe UI Symbol" w:cs="Segoe UI Symbol"/></w:rPr><w:t>{}</w:t><w:tab/></w:r>"#,
                        if *done { "\u{2611}" } else { "\u{2610}" }
                    );
                }
                let color = if ctx.quote > 0 && ctx.accent.is_none() { Some("57606A") } else { None };
                self.runs(&mut out, inl, ctx, false, color);
                out.push_str("</w:p>");
            }
            Block::Code { text, ctx } => {
                let _ = write!(out, "<w:p>{}", Self::ppr(ctx, &Marker::None, Some("CodeBlock"), false, ""));
                text_run(&mut out, text, "");
                out.push_str("</w:p>");
            }
            Block::Rule => out.push_str(
                r#"<w:p><w:pPr><w:pBdr><w:bottom w:val="single" w:sz="6" w:space="1" w:color="D0D7DE"/></w:pBdr><w:spacing w:before="120" w:after="240"/></w:pPr></w:p>"#,
            ),
            Block::Table { aligns, rows, ctx } => {
                let cols = rows.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
                let indent = 720 * ctx.level as u32 + 300 * ctx.quote as u32;
                let total = 9026u32.saturating_sub(indent);
                let col_w = total / cols as u32;
                let border = |side: &str| format!(r#"<w:{side} w:val="single" w:sz="4" w:space="0" w:color="BFC5CC"/>"#);
                let _ = write!(
                    out,
                    r#"<w:tbl><w:tblPr><w:tblW w:w="{total}" w:type="dxa"/><w:tblInd w:w="{indent}" w:type="dxa"/><w:tblBorders>{}{}{}{}{}{}</w:tblBorders><w:tblLayout w:type="fixed"/><w:tblCellMar><w:top w:w="70" w:type="dxa"/><w:left w:w="115" w:type="dxa"/><w:bottom w:w="70" w:type="dxa"/><w:right w:w="115" w:type="dxa"/></w:tblCellMar><w:tblLook w:val="04A0" w:firstRow="1" w:lastRow="0" w:firstColumn="0" w:lastColumn="0" w:noHBand="0" w:noVBand="1"/></w:tblPr><w:tblGrid>"#,
                    border("top"), border("left"), border("bottom"), border("right"), border("insideH"), border("insideV")
                );
                for _ in 0..cols {
                    let _ = write!(out, r#"<w:gridCol w:w="{col_w}"/>"#);
                }
                out.push_str("</w:tblGrid>");
                let cell_ctx = Ctx { level: ctx.level.max(3), ..Ctx::default() }; // narrows images inside cells
                for (ri, row) in rows.iter().enumerate() {
                    out.push_str("<w:tr>");
                    if ri == 0 {
                        out.push_str("<w:trPr><w:tblHeader/></w:trPr>");
                    }
                    for ci in 0..cols {
                        let _ = write!(out, r#"<w:tc><w:tcPr><w:tcW w:w="{col_w}" w:type="dxa"/>{}</w:tcPr><w:p><w:pPr><w:spacing w:after="0"/>"#,
                            if ri == 0 { r#"<w:shd w:val="clear" w:color="auto" w:fill="F2F4F7"/>"# } else { "" });
                        match aligns.get(ci) {
                            Some(TableAlignment::Center) => out.push_str(r#"<w:jc w:val="center"/>"#),
                            Some(TableAlignment::Right) => out.push_str(r#"<w:jc w:val="right"/>"#),
                            _ => {}
                        }
                        out.push_str("</w:pPr>");
                        if let Some(cell) = row.get(ci) {
                            self.force_bold = ri == 0;
                            self.runs(&mut out, cell, &cell_ctx, false, None);
                            self.force_bold = false;
                        }
                        out.push_str("</w:p></w:tc>");
                    }
                    out.push_str("</w:tr>");
                }
                out.push_str(r#"</w:tbl><w:p><w:pPr><w:spacing w:after="0"/></w:pPr></w:p>"#);
            }
        }
        self.body.push_str(&out);
    }
}

const STYLES_HEAD: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="Calibri" w:cs="Calibri"/><w:sz w:val="22"/><w:szCs w:val="22"/><w:lang w:val="en-US" w:eastAsia="en-US" w:bidi="ar-SA"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="276" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style><w:style w:type="character" w:default="1" w:styleId="DefaultParagraphFont"><w:name w:val="Default Paragraph Font"/><w:uiPriority w:val="1"/><w:semiHidden/><w:unhideWhenUsed/></w:style><w:style w:type="table" w:default="1" w:styleId="TableNormal"><w:name w:val="Normal Table"/><w:uiPriority w:val="99"/><w:semiHidden/><w:unhideWhenUsed/><w:tblPr><w:tblInd w:w="0" w:type="dxa"/><w:tblCellMar><w:top w:w="0" w:type="dxa"/><w:left w:w="108" w:type="dxa"/><w:bottom w:w="0" w:type="dxa"/><w:right w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style><w:style w:type="character" w:styleId="Hyperlink"><w:name w:val="Hyperlink"/><w:basedOn w:val="DefaultParagraphFont"/><w:uiPriority w:val="99"/><w:unhideWhenUsed/><w:rPr><w:color w:val="0563C1"/><w:u w:val="single"/></w:rPr></w:style><w:style w:type="character" w:customStyle="1" w:styleId="InlineCode"><w:name w:val="Inline Code"/><w:basedOn w:val="DefaultParagraphFont"/><w:uiPriority w:val="1"/><w:qFormat/><w:rPr><w:rFonts w:ascii="Consolas" w:hAnsi="Consolas" w:cs="Consolas"/><w:sz w:val="20"/><w:szCs w:val="20"/><w:shd w:val="clear" w:color="auto" w:fill="EFF1F3"/></w:rPr></w:style><w:style w:type="paragraph" w:customStyle="1" w:styleId="CodeBlock"><w:name w:val="Code Block"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr><w:pBdr><w:top w:val="single" w:sz="4" w:space="6" w:color="D0D7DE"/><w:left w:val="single" w:sz="4" w:space="6" w:color="D0D7DE"/><w:bottom w:val="single" w:sz="4" w:space="6" w:color="D0D7DE"/><w:right w:val="single" w:sz="4" w:space="6" w:color="D0D7DE"/></w:pBdr><w:shd w:val="clear" w:color="auto" w:fill="F6F8FA"/><w:spacing w:before="80" w:after="200" w:line="252" w:lineRule="auto"/></w:pPr><w:rPr><w:rFonts w:ascii="Consolas" w:hAnsi="Consolas" w:cs="Consolas"/><w:noProof/><w:sz w:val="19"/><w:szCs w:val="19"/></w:rPr></w:style>"#;

fn styles_xml() -> String {
    let mut s = String::from(STYLES_HEAD);
    for (i, (size, before)) in [(40, 420), (32, 360), (27, 300), (24, 260), (22, 240), (22, 240)].iter().enumerate() {
        let n = i + 1;
        let _ = write!(
            s,
            r#"<w:style w:type="paragraph" w:styleId="Heading{n}"><w:name w:val="heading {n}"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before="{before}" w:after="120"/><w:outlineLvl w:val="{i}"/></w:pPr><w:rPr><w:rFonts w:ascii="Calibri Light" w:hAnsi="Calibri Light" w:cs="Calibri Light"/><w:b/><w:bCs/><w:color w:val="{}"/><w:sz w:val="{size}"/><w:szCs w:val="{size}"/></w:rPr></w:style>"#,
            if n == 6 { "57606A" } else { "1F2328" }
        );
    }
    s.push_str("</w:styles>");
    s
}

fn numbering_xml(doc: &Doc) -> String {
    let mut s = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
    );
    s.push_str(r#"<w:abstractNum w:abstractNumId="0"><w:multiLevelType w:val="hybridMultilevel"/>"#);
    for l in 0..9 {
        let (ch, font) = [("\u{2022}", "Calibri"), ("o", "Courier New"), ("\u{25AA}", "Segoe UI Symbol")][l % 3];
        let _ = write!(
            s,
            r#"<w:lvl w:ilvl="{l}"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="{ch}"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="{}" w:hanging="360"/></w:pPr><w:rPr><w:rFonts w:ascii="{font}" w:hAnsi="{font}" w:cs="{font}" w:hint="default"/></w:rPr></w:lvl>"#,
            720 * (l + 1)
        );
    }
    s.push_str(r#"</w:abstractNum><w:abstractNum w:abstractNumId="1"><w:multiLevelType w:val="hybridMultilevel"/>"#);
    for l in 0..9 {
        let fmt = "decimal"; // Markdown numbers every level 1, 2, 3
        let _ = write!(
            s,
            r#"<w:lvl w:ilvl="{l}"><w:start w:val="1"/><w:numFmt w:val="{fmt}"/><w:lvlText w:val="%{}."/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="{}" w:hanging="360"/></w:pPr></w:lvl>"#,
            l + 1,
            720 * (l + 1)
        );
    }
    s.push_str(r#"</w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>"#);
    for (i, start) in doc.ordered.iter().enumerate() {
        let _ = write!(s, r#"<w:num w:numId="{}"><w:abstractNumId w:val="1"/>"#, i + 2);
        for l in 0..9 {
            let _ = write!(s, r#"<w:lvlOverride w:ilvl="{l}"><w:startOverride w:val="{start}"/></w:lvlOverride>"#);
        }
        s.push_str("</w:num>");
    }
    s.push_str("</w:numbering>");
    s
}

pub fn write_docx(doc: &Doc) -> Result<Vec<u8>, String> {
    let mut d = Docx {
        body: String::new(),
        rels: String::new(),
        next_rel: 3,
        media: Vec::new(),
        media_ids: HashMap::new(),
        links: HashMap::new(),
        bookmarks: HashMap::new(),
        next_id: 0,
        force_bold: false,
    };
    for (i, b) in doc.blocks.iter().enumerate() {
        if let Block::Heading { anchor, .. } = b {
            d.bookmarks.entry(anchor.clone()).or_insert_with(|| format!("_ink{i}"));
        }
    }
    for b in &doc.blocks {
        d.block(b);
    }
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"><w:body>{}<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr></w:body></w:document>"#,
        d.body
    );
    let rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>{}</Relationships>"#,
        d.rels
    );
    let content_types = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Default Extension="jpeg" ContentType="image/jpeg"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/></Types>"#;
    let root_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>"#;
    let core = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>{}</dc:title><dc:creator>Inkdown</dc:creator></cp:coreProperties>"#,
        xml(&doc.title)
    );

    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let deflate = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut put = |name: &str, bytes: &[u8], compress: bool| -> Result<(), String> {
        zip.start_file(name, if compress { deflate } else { stored }).map_err(|e| e.to_string())?;
        zip.write_all(bytes).map_err(|e| e.to_string())
    };
    put("[Content_Types].xml", content_types.as_bytes(), true)?;
    put("_rels/.rels", root_rels.as_bytes(), true)?;
    put("docProps/core.xml", core.as_bytes(), true)?;
    put("word/document.xml", document.as_bytes(), true)?;
    put("word/_rels/document.xml.rels", rels.as_bytes(), true)?;
    put("word/styles.xml", styles_xml().as_bytes(), true)?;
    put("word/numbering.xml", numbering_xml(doc).as_bytes(), true)?;
    for (name, img) in &d.media {
        put(&format!("word/media/{name}"), &img.data, false)?;
    }
    zip.finish().map(|c| c.into_inner()).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------- RTF

fn rtf_text(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '\n' => out.push_str("\\line "),
            '\t' => out.push_str("\\tab "),
            '\r' => {}
            c if (c as u32) < 0x80 => out.push(c),
            c => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    let _ = write!(out, "\\u{}?", *unit as i16);
                }
            }
        }
    }
}

fn rtf_picture(out: &mut String, img: &Img, ctx: &Ctx) {
    let (w, h) = fit(img, ctx.level, ctx.quote);
    let _ = write!(
        out,
        "{{\\pict\\{}\\picw{}\\pich{}\\picwgoal{}\\pichgoal{} ",
        if img.jpeg { "jpegblip" } else { "pngblip" },
        w.round() as i32,
        h.round() as i32,
        (w * 15.0).round() as i32,
        (h * 15.0).round() as i32
    );
    out.reserve(img.data.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for (i, b) in img.data.iter().enumerate() {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 15) as usize] as char);
        if i % 64 == 63 {
            out.push('\n');
        }
    }
    out.push('}');
}

fn rtf_runs(out: &mut String, inl: &[Inline], ctx: &Ctx, link: bool) {
    for item in inl {
        match item {
            Inline::Text(t, f) => {
                out.push('{');
                let mark = out.len();
                if f.b { out.push_str("\\b"); }
                if f.i { out.push_str("\\i"); }
                if f.strike { out.push_str("\\strike"); }
                if f.u || link { out.push_str("\\ul"); }
                if link { out.push_str("\\cf1"); }
                if f.sup { out.push_str("\\super"); }
                if f.sub { out.push_str("\\sub"); }
                if f.mark { out.push_str("\\highlight4"); }
                if f.code { out.push_str("\\f1\\fs20\\chshdng0\\chcbpat3"); }
                if out.len() > mark {
                    out.push(' '); // delimiter after the last control word only
                }
                rtf_text(out, t);
                out.push('}');
            }
            Inline::Break => out.push_str("\\line "),
            Inline::Image(img, _) => rtf_picture(out, img, ctx),
            Inline::Link(url, kids) => {
                if url.starts_with('#') {
                    rtf_runs(out, kids, ctx, false);
                } else {
                    out.push_str("{\\field{\\*\\fldinst{HYPERLINK \"");
                    rtf_text(out, &url.replace('"', "%22"));
                    out.push_str("\"}}{\\fldrslt{");
                    rtf_runs(out, kids, ctx, true);
                    out.push_str("}}}");
                }
            }
        }
    }
}

fn rtf_accent(hex: Option<&str>) -> u8 {
    match hex {
        Some("0969DA") => 6,
        Some("1A7F37") => 7,
        Some("8250DF") => 8,
        Some("9A6700") => 9,
        Some("CF222E") => 10,
        _ => 5,
    }
}

pub fn write_rtf(doc: &Doc) -> Vec<u8> {
    let mut o = String::from(
        "{\\rtf1\\ansi\\ansicpg1252\\deff0\\uc1\n{\\fonttbl{\\f0\\fswiss\\fcharset0 Calibri;}{\\f1\\fmodern\\fcharset0 Consolas;}{\\f2\\fnil\\fcharset0 Segoe UI Symbol;}}\n{\\colortbl;\\red5\\green99\\blue193;\\red87\\green96\\blue106;\\red246\\green248\\blue250;\\red255\\green243\\blue176;\\red208\\green215\\blue222;\\red9\\green105\\blue218;\\red26\\green127\\blue55;\\red130\\green80\\blue223;\\red154\\green103\\blue0;\\red207\\green34\\blue46;}\n{\\info{\\title ",
    );
    rtf_text(&mut o, &doc.title);
    o.push_str("}{\\author Inkdown}}\n\\paperw11906\\paperh16838\\margl1440\\margr1440\\margt1440\\margb1440\\viewkind1\n");
    const BASE: &str = "\\pard\\plain\\f0\\fs22\\sl276\\slmult1";
    for b in &doc.blocks {
        match b {
            Block::Heading { level, inl, .. } => {
                let size = [40, 32, 27, 24, 22, 22][(*level as usize - 1).min(5)];
                let _ = write!(o, "\\pard\\plain\\f0\\b\\fs{size}\\sb360\\sa120\\keepn\\outlinelevel{} ", level - 1);
                rtf_runs(&mut o, inl, &Ctx::default(), false);
                o.push_str("\\par\n");
            }
            Block::Para { inl, ctx, marker, center } => {
                let left = 720 * ctx.level as u32 + 300 * ctx.quote as u32;
                let _ = write!(o, "{BASE}\\sa{}\\li{left}", if ctx.level > 0 { 60 } else { 160 });
                if !matches!(marker, Marker::None) {
                    o.push_str("\\fi-360");
                }
                if ctx.quote > 0 {
                    let _ = write!(o, "\\brdrl\\brdrs\\brdrw30\\brsp200\\brdrcf{}", rtf_accent(ctx.accent));
                    if ctx.accent.is_none() {
                        o.push_str("\\cf2");
                    }
                }
                if *center {
                    o.push_str("\\qc");
                }
                o.push(' ');
                match marker {
                    Marker::Bullet => o.push_str("\\bullet\\tab "),
                    Marker::Number { n, .. } => { let _ = write!(o, "{n}.\\tab "); }
                    Marker::Task(done) => { let _ = write!(o, "{{\\f2 \\u{}?}}\\tab ", if *done { 9745 } else { 9744 }); }
                    Marker::None => {}
                }
                rtf_runs(&mut o, inl, ctx, false);
                o.push_str("\\par\n");
            }
            Block::Code { text, ctx } => {
                let left = 720 * ctx.level as u32 + 300 * ctx.quote as u32;
                let _ = write!(o, "\\pard\\plain\\f1\\fs19\\sa200\\li{left}\\cbpat3\\box\\brdrs\\brdrw10\\brsp100\\brdrcf5 ");
                rtf_text(&mut o, text);
                o.push_str("\\par\n");
            }
            Block::Rule => o.push_str("\\pard\\plain\\fs8\\sa240\\brdrb\\brdrs\\brdrw10\\brsp20\\brdrcf5 \\par\n"),
            Block::Table { aligns, rows, ctx } => {
                let cols = rows.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
                let indent = 720 * ctx.level as u32 + 300 * ctx.quote as u32;
                let col_w = 9026u32.saturating_sub(indent) / cols as u32;
                let cell_ctx = Ctx { level: ctx.level.max(3), ..Ctx::default() };
                for (ri, row) in rows.iter().enumerate() {
                    let _ = write!(o, "\\trowd\\trgaph115\\trleft{indent}\\trpaddt70\\trpaddb70\\trpaddft3\\trpaddfb3");
                    if ri == 0 {
                        o.push_str("\\trhdr");
                    }
                    for ci in 0..cols {
                        for side in ["t", "l", "b", "r"] {
                            let _ = write!(o, "\\clbrdr{side}\\brdrs\\brdrw10\\brdrcf5");
                        }
                        if ri == 0 {
                            o.push_str("\\clcbpat3");
                        }
                        let _ = write!(o, "\\cellx{}", indent + col_w * (ci as u32 + 1));
                    }
                    o.push('\n');
                    for ci in 0..cols {
                        let _ = write!(o, "\\pard\\plain\\intbl\\f0\\fs22");
                        match aligns.get(ci) {
                            Some(TableAlignment::Center) => o.push_str("\\qc"),
                            Some(TableAlignment::Right) => o.push_str("\\qr"),
                            _ => {}
                        }
                        if ri == 0 {
                            o.push_str("\\b");
                        }
                        o.push(' ');
                        if let Some(cell) = row.get(ci) {
                            rtf_runs(&mut o, cell, &cell_ctx, false);
                        }
                        o.push_str("\\cell\n");
                    }
                    o.push_str("\\row\n");
                }
                o.push_str("\\pard\\plain\\fs10\\sa80 \\par\n");
            }
        }
    }
    o.push('}');
    o.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "---\ntitle: Sample\n---\n# Title\n\nPara with **bold**, *italic*, `code`, ==mark==, [link](https://example.com) and [jump](#section).\n\n- one\n- two\n    1. nested\n    2. numbers\n- [x] done\n\n> quote\n\n> [!WARNING]\n> careful\n\n| a | b |\n|:-:|--:|\n| 1 | 2 |\n\n```rust\nfn main() {}\n```\n\n## Section\n\nText[^1] with unicode \u{00e9}\u{4e2d}\u{1F600} and $x^2$.\n\n---\n\n[^1]: A note.\n";

    #[test]
    fn docx_is_a_valid_package() {
        let mut assets = Assets::default();
        assets.math.insert("0|x^2".into(), Rc::new(Img { jpeg: false, data: vec![1, 2, 3], w: 20.0, h: 16.0, depth: 3.0 }));
        let doc = build(SAMPLE, &assets, "fallback");
        assert_eq!(doc.title, "Sample");
        let bytes = write_docx(&doc).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let names: Vec<String> = zip.file_names().filter_map(|n| n.ok()).map(|n| n.to_string()).collect();
        assert!(names.contains(&"word/document.xml".to_string()));
        assert!(names.contains(&"word/media/image1.png".to_string()));
        let mut body = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").unwrap(), &mut body).unwrap();
        for needle in ["Heading1", "<w:b/>", "w:anchor=\"_ink", "<w:tbl>", "CodeBlock", "<w:numPr>", "\u{2611}", "w:position"] {
            assert!(body.contains(needle), "missing {needle}");
        }
    }

    #[test]
    fn rtf_is_balanced() {
        let doc = build(SAMPLE, &Assets::default(), "t");
        let rtf = String::from_utf8(write_rtf(&doc)).unwrap();
        assert!(rtf.starts_with("{\\rtf1"));
        assert!(rtf.is_ascii());
        let (mut depth, mut prev) = (0i32, ' ');
        for c in rtf.chars() {
            if prev != '\\' {
                if c == '{' { depth += 1 } else if c == '}' { depth -= 1 }
            }
            assert!(depth >= 0);
            prev = if prev == '\\' && c == '\\' { ' ' } else { c };
        }
        assert_eq!(depth, 0);
        assert!(rtf.contains("\\trowd") && rtf.contains("HYPERLINK") && rtf.contains("\\u233?"));
    }
}
