//! Writing PDF files without a browser.
//!
//! A document is laid out here, on A4 pages, from the same block model the reader uses:
//! text is real text (selectable, searchable, in the bundled fonts), diagrams and formulas
//! are placed as pictures, links can be clicked and headings become bookmarks. The same
//! code runs on every platform, phones included.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use krilla::color::rgb;
use krilla::geom::{PathBuilder, Point, Rect, Size, Transform};
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{Fill, Stroke};
use krilla::text::{Font, TextDirection};
use krilla::Document;

use crate::md::{AlertKind, Block, CellAlign, DiagramKind, Inl, Item, Style};

const PAGE_W: f32 = 595.0;
const PAGE_H: f32 = 842.0;
const MARGIN_X: f32 = 56.0;
const MARGIN_TOP: f32 = 58.0;
const MARGIN_BOTTOM: f32 = 62.0;
const BODY: f32 = 10.5;
const LINE: f32 = 1.5;

type Rgb = [u8; 3];
const TEXT: Rgb = [28, 30, 36];
const MUTED: Rgb = [100, 106, 118];
const FAINT: Rgb = [150, 155, 165];
const LINK: Rgb = [38, 92, 200];
const RULE: Rgb = [216, 220, 228];
const CODE_BG: Rgb = [244, 246, 249];
const MARK_BG: Rgb = [255, 240, 150];

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Face {
    Sans,
    Bold,
    Italic,
    BoldItalic,
    Mono,
    MonoBold,
}

impl Face {
    fn of(style: &Style, strong: bool) -> Face {
        match (style.code || style.kbd, style.bold || strong, style.italic) {
            (true, true, _) => Face::MonoBold,
            (true, false, _) => Face::Mono,
            (false, true, true) => Face::BoldItalic,
            (false, true, false) => Face::Bold,
            (false, false, true) => Face::Italic,
            (false, false, false) => Face::Sans,
        }
    }
    fn index(self) -> usize {
        self as usize
    }
}

/// Which font draws a piece of text: one of ours, or one found on the system for
/// characters ours do not have.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum FontRef {
    Own(Face),
    Extra(usize),
}

struct Loaded {
    font: Font,
    data: Arc<Vec<u8>>,
    index: u32,
    weight: Option<f32>,
}

impl Loaded {
    fn face(&self) -> Option<ttf_parser::Face<'_>> {
        let mut f = ttf_parser::Face::parse(&self.data, self.index).ok()?;
        if let Some(w) = self.weight {
            let _ = f.set_variation(ttf_parser::Tag::from_bytes(b"wght"), w);
        }
        Some(f)
    }
}

struct Fonts {
    own: Vec<Loaded>,
    extra: Vec<Loaded>,
    /// Advance of a char in thousandths of the font size, per font.
    widths: HashMap<(FontRef, char), Option<f32>>,
    /// The system font that has a char ours lack.
    fallback: HashMap<char, Option<usize>>,
    by_id: HashMap<resvg::usvg::fontdb::ID, usize>,
}

impl Fonts {
    fn new() -> Result<Self, String> {
        let load = |bytes: &'static [u8], weight: f32| -> Result<Loaded, String> {
            let data = Arc::new(bytes.to_vec());
            let variable = ttf_parser::Face::parse(&data, 0).map(|f| f.is_variable()).unwrap_or(false);
            let font = if variable { Font::new_variable(data.clone().into(), 0, &[(krilla::text::Tag::new(b"wght"), weight)]) } else { None }.or_else(|| Font::new(data.clone().into(), 0)).ok_or("A bundled font could not be read.")?;
            Ok(Loaded { font, data, index: 0, weight: variable.then_some(weight) })
        };
        let sans: &'static [u8] = include_bytes!("../fonts/Inter.ttf");
        let italic: &'static [u8] = include_bytes!("../fonts/Inter-Italic.ttf");
        let mono: &'static [u8] = include_bytes!("../fonts/JetBrainsMono.ttf");
        // in the order of `Face`
        let own = vec![load(sans, 400.0)?, load(sans, 700.0)?, load(italic, 400.0)?, load(italic, 700.0)?, load(mono, 400.0)?, load(mono, 700.0)?];
        Ok(Self { own, extra: Vec::new(), widths: HashMap::new(), fallback: HashMap::new(), by_id: HashMap::new() })
    }

    fn loaded(&self, f: FontRef) -> &Loaded {
        match f {
            FontRef::Own(face) => &self.own[face.index()],
            FontRef::Extra(i) => &self.extra[i],
        }
    }

    /// Width of `c` at size 1, or `None` when the font has no such char.
    fn advance(&mut self, f: FontRef, c: char) -> Option<f32> {
        if let Some(w) = self.widths.get(&(f, c)) {
            return *w;
        }
        let w = self.loaded(f).face().and_then(|face| {
            let g = face.glyph_index(c)?;
            Some(face.glyph_hor_advance(g).unwrap_or(0) as f32 / face.units_per_em() as f32)
        });
        self.widths.insert((f, c), w);
        w
    }

    /// The font to draw `c` with when `wanted` is asked for.
    fn font_for(&mut self, wanted: Face, c: char) -> FontRef {
        if c.is_whitespace() || self.advance(FontRef::Own(wanted), c).is_some() {
            return FontRef::Own(wanted);
        }
        // the same char in the plain face (the mono font lacks many symbols)
        if self.advance(FontRef::Own(Face::Sans), c).is_some() {
            return FontRef::Own(Face::Sans);
        }
        if let Some(found) = self.fallback.get(&c) {
            return found.map(FontRef::Extra).unwrap_or(FontRef::Own(wanted));
        }
        let db = crate::assets::fontdb();
        let mut found = None;
        for info in db.faces() {
            let has = db.with_face_data(info.id, |data, index| ttf_parser::Face::parse(data, index).ok().is_some_and(|f| f.glyph_index(c).is_some())).unwrap_or(false);
            if !has {
                continue;
            }
            found = match self.by_id.get(&info.id) {
                Some(i) => Some(*i),
                None => db.with_face_data(info.id, |data, index| (Arc::new(data.to_vec()), index)).and_then(|(data, index)| {
                    let font = Font::new(data.clone().into(), index)?;
                    self.extra.push(Loaded { font, data, index, weight: None });
                    self.by_id.insert(info.id, self.extra.len() - 1);
                    Some(self.extra.len() - 1)
                }),
            };
            if found.is_some() {
                break;
            }
        }
        self.fallback.insert(c, found);
        found.map(FontRef::Extra).unwrap_or(FontRef::Own(wanted))
    }

    fn width(&mut self, f: FontRef, text: &str, size: f32) -> f32 {
        text.chars().map(|c| self.advance(f, c).unwrap_or(0.5)).sum::<f32>() * size
    }
}

/// One thing drawn on a page. Positions are in points from the top left corner.
enum Op {
    Text { x: f32, y: f32, font: FontRef, size: f32, color: Rgb, text: String },
    Fill { x: f32, y: f32, w: f32, h: f32, color: Rgb },
    Frame { x: f32, y: f32, w: f32, h: f32, color: Rgb, width: f32 },
    Line { x0: f32, y0: f32, x1: f32, y1: f32, color: Rgb, width: f32 },
    Image { x: f32, y: f32, w: f32, h: f32, image: usize },
    Link { x: f32, y: f32, w: f32, h: f32, url: String },
}

/// A run of inline content with one look.
#[derive(Clone)]
struct Span {
    text: String,
    face: Face,
    size: f32,
    color: Rgb,
    bg: Option<Rgb>,
    underline: bool,
    strike: bool,
    link: Option<Arc<str>>,
    /// Raised (positive) or lowered text, in points.
    rise: f32,
    /// A picture in the line: index, width, height and how far it hangs below the baseline.
    image: Option<(usize, f32, f32, f32)>,
    hard_break: bool,
}

impl Span {
    fn text(text: impl Into<String>, face: Face, size: f32, color: Rgb) -> Self {
        Span { text: text.into(), face, size, color, bg: None, underline: false, strike: false, link: None, rise: 0.0, image: None, hard_break: false }
    }
}

/// A piece of a line after breaking: text in one font, or a picture.
struct Piece {
    span: usize,
    font: FontRef,
    text: String,
    w: f32,
    space: bool,
}

struct LaidLine {
    pieces: Vec<Piece>,
    width: f32,
    height: f32,
    /// Distance from the top of the line to the baseline.
    ascent: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum Align {
    Left,
    Center,
    Right,
}

struct Writer<'a> {
    fonts: Fonts,
    pages: Vec<Vec<Op>>,
    images: Vec<krilla::image::Image>,
    y: f32,
    /// Left edge and width of the column being filled.
    x: f32,
    width: f32,
    /// Vertical bars at the left of quotes and callouts that are open: `(x, colour)`.
    bars: Vec<(f32, Rgb)>,
    align: Align,
    base: Option<&'a Path>,
    /// Headings for the bookmarks: level, text, page, y.
    outline: Vec<(u8, String, usize, f32)>,
    /// Bigger or smaller text inside some blocks.
    scale: f32,
}

impl<'a> Writer<'a> {
    fn new(base: Option<&'a Path>) -> Result<Self, String> {
        Ok(Self { fonts: Fonts::new()?, pages: vec![Vec::new()], images: Vec::new(), y: MARGIN_TOP, x: MARGIN_X, width: PAGE_W - 2.0 * MARGIN_X, bars: Vec::new(), align: Align::Left, base, outline: Vec::new(), scale: 1.0 })
    }

    fn page(&mut self) -> &mut Vec<Op> {
        self.pages.last_mut().unwrap()
    }

    fn bottom(&self) -> f32 {
        PAGE_H - MARGIN_BOTTOM
    }

    fn new_page(&mut self) {
        self.pages.push(Vec::new());
        self.y = MARGIN_TOP;
    }

    /// Makes sure `h` points fit on the page, starting a new one if they do not.
    fn need(&mut self, h: f32) {
        if self.y + h > self.bottom() && self.y > MARGIN_TOP + 0.5 {
            self.new_page();
        }
    }

    /// Moves down, keeping the bars of open quotes continuous.
    fn advance(&mut self, dy: f32) {
        if dy <= 0.0 {
            return;
        }
        let (y, room) = (self.y, self.bottom() - self.y);
        let h = dy.min(room.max(0.0));
        let bars = self.bars.clone();
        for (x, color) in bars {
            self.page().push(Op::Fill { x, y, w: 2.5, h, color });
        }
        self.y += dy;
    }

    /// Vertical space between blocks; none at the top of a page.
    fn gap(&mut self, dy: f32) {
        if self.y > MARGIN_TOP + 0.5 {
            self.advance(dy);
        }
    }

    // ------------------------------------------------------------ inline

    fn spans_of(&mut self, inl: &[Inl], face_bold: bool, size: f32, color: Rgb) -> Vec<Span> {
        let mut out = Vec::new();
        for i in inl {
            match i {
                Inl::Text(text, style) => {
                    let mut s = Span::text(text.clone(), Face::of(style, face_bold), size, color);
                    if style.code || style.kbd {
                        s.size = size * 0.9;
                        s.bg = Some(CODE_BG);
                    }
                    if style.mark {
                        s.bg = Some(MARK_BG);
                    }
                    if style.sup || style.sub {
                        s.size = size * 0.72;
                        s.rise = if style.sup { size * 0.36 } else { -size * 0.14 };
                    }
                    s.strike = style.strike;
                    s.underline = style.underline;
                    if let Some(url) = &style.link {
                        s.link = Some(url.clone());
                        s.color = LINK;
                        s.underline = true;
                    }
                    out.push(s);
                }
                Inl::Break => {
                    let mut s = Span::text("", Face::Sans, size, color);
                    s.hard_break = true;
                    out.push(s);
                }
                Inl::FootRef(_, n) => {
                    let mut s = Span::text(n.to_string(), Face::Sans, size * 0.72, LINK);
                    s.rise = size * 0.36;
                    out.push(s);
                }
                Inl::Math(tex) => match self.math_image(tex, false, size) {
                    Some((image, w, h, depth)) => {
                        let mut s = Span::text("", Face::Sans, size, color);
                        s.image = Some((image, w, h, depth));
                        out.push(s);
                    }
                    None => out.push(Span::text(tex.clone(), Face::Mono, size * 0.9, color)),
                },
                Inl::Image { url, alt, width, link, .. } => match self.picture(url, *width) {
                    Some((image, w, h)) => {
                        // a picture in a line of text is kept small enough to sit in it
                        let fit = (self.width / w).min(1.0).min(if h > 160.0 { 160.0 / h } else { 1.0 });
                        let mut s = Span::text("", Face::Sans, size, color);
                        s.image = Some((image, w * fit, h * fit, 0.0));
                        s.link = link.clone();
                        out.push(s);
                    }
                    None if !alt.is_empty() => out.push(Span::text(format!("[{alt}]"), Face::Italic, size, MUTED)),
                    None => {}
                },
            }
        }
        out
    }

    /// Breaks spans into lines no wider than `width`.
    fn lay(&mut self, spans: &[Span], width: f32) -> Vec<LaidLine> {
        // words, spaces and pictures, each in the font that has its chars
        let mut atoms: Vec<Piece> = Vec::new();
        let mut breaks: Vec<usize> = Vec::new(); // atom indices that start a new line by force
        for (si, s) in spans.iter().enumerate() {
            if s.hard_break {
                breaks.push(atoms.len());
                continue;
            }
            if let Some((_, w, _, _)) = s.image {
                atoms.push(Piece { span: si, font: FontRef::Own(s.face), text: String::new(), w, space: false });
                continue;
            }
            let mut cur = String::new();
            let mut cur_font = FontRef::Own(s.face);
            let mut cur_space = false;
            let mut flush = |fonts: &mut Fonts, cur: &mut String, font: FontRef, space: bool, atoms: &mut Vec<Piece>| {
                if !cur.is_empty() {
                    let w = fonts.width(font, cur, s.size);
                    atoms.push(Piece { span: si, font, text: std::mem::take(cur), w, space });
                }
            };
            for c in s.text.chars() {
                if c == '\n' {
                    flush(&mut self.fonts, &mut cur, cur_font, cur_space, &mut atoms);
                    breaks.push(atoms.len());
                    continue;
                }
                let c = if c == '\t' { ' ' } else { c };
                let space = c == ' ';
                let font = self.fonts.font_for(s.face, c);
                // a new atom at every change between word and space, at a change of font,
                // and after chars a long word may be broken at
                if !cur.is_empty() && (space != cur_space || font != cur_font) {
                    flush(&mut self.fonts, &mut cur, cur_font, cur_space, &mut atoms);
                }
                cur_font = font;
                cur_space = space;
                cur.push(c);
                if matches!(c, '/' | '-' | '_' | '.' | ',' | ';' | ':' | '?' | '&' | '=') && cur.chars().count() > 12 {
                    flush(&mut self.fonts, &mut cur, cur_font, cur_space, &mut atoms);
                }
            }
            flush(&mut self.fonts, &mut cur, cur_font, cur_space, &mut atoms);
        }
        // greedy filling
        let mut lines: Vec<Vec<Piece>> = vec![Vec::new()];
        let mut used = 0.0f32;
        for (i, atom) in atoms.into_iter().enumerate() {
            if breaks.contains(&i) {
                lines.push(Vec::new());
                used = 0.0;
            }
            if used + atom.w > width + 0.01 && used > 0.0 {
                if atom.space {
                    // a space at the end of a line is dropped
                    lines.push(Vec::new());
                    used = 0.0;
                    continue;
                }
                lines.push(Vec::new());
                used = 0.0;
            }
            if atom.space && used == 0.0 && !lines.last().unwrap().is_empty() {
                continue;
            }
            if atom.w > width && !atom.text.is_empty() {
                // a word wider than the line: cut it where it must be
                let size = spans[atom.span].size;
                let mut part = String::new();
                let mut part_w = 0.0;
                for c in atom.text.chars() {
                    let cw = self.fonts.advance(atom.font, c).unwrap_or(0.5) * size;
                    if used + part_w + cw > width && !(part.is_empty() && used == 0.0) {
                        if !part.is_empty() {
                            lines.last_mut().unwrap().push(Piece { span: atom.span, font: atom.font, text: std::mem::take(&mut part), w: part_w, space: false });
                        }
                        lines.push(Vec::new());
                        used = 0.0;
                        part_w = 0.0;
                    }
                    part.push(c);
                    part_w += cw;
                }
                used += part_w;
                lines.last_mut().unwrap().push(Piece { span: atom.span, font: atom.font, text: part, w: part_w, space: false });
                continue;
            }
            used += atom.w;
            lines.last_mut().unwrap().push(atom);
        }
        lines
            .into_iter()
            .map(|mut pieces| {
                while pieces.last().is_some_and(|p| p.space) {
                    pieces.pop();
                }
                let width = pieces.iter().map(|p| p.w).sum();
                let mut ascent = 0.0f32;
                let mut descent = 0.0f32;
                for p in &pieces {
                    let s = &spans[p.span];
                    match s.image {
                        Some((_, _, h, depth)) => {
                            ascent = ascent.max(h - depth + 2.0);
                            descent = descent.max(depth + 2.0);
                        }
                        None => {
                            ascent = ascent.max(s.size * 1.02 + s.rise.max(0.0));
                            descent = descent.max(s.size * (LINE - 1.02) - s.rise.min(0.0));
                        }
                    }
                }
                if pieces.is_empty() {
                    let size = spans.first().map(|s| s.size).unwrap_or(BODY);
                    ascent = size * 1.02;
                    descent = size * (LINE - 1.02);
                }
                LaidLine { pieces, width, height: ascent + descent, ascent }
            })
            .collect()
    }

    /// Draws one laid line with its top at `y`, inside the column `x..x + width`.
    fn draw_line(&mut self, spans: &[Span], line: &LaidLine, x: f32, y: f32, width: f32, align: Align) {
        let mut px = match align {
            Align::Left => x,
            Align::Center => x + (width - line.width).max(0.0) / 2.0,
            Align::Right => x + (width - line.width).max(0.0),
        };
        let baseline = y + line.ascent;
        for p in &line.pieces {
            let s = &spans[p.span];
            if let Some((image, w, h, depth)) = s.image {
                self.page().push(Op::Image { x: px, y: baseline + depth - h, w, h, image });
                if let Some(url) = &s.link {
                    self.page().push(Op::Link { x: px, y: baseline + depth - h, w, h, url: url.to_string() });
                }
                px += w;
                continue;
            }
            let (top, h) = (baseline - s.rise - s.size * 0.95, s.size * 1.3);
            if let Some(bg) = s.bg {
                self.page().push(Op::Fill { x: px - 1.0, y: top, w: p.w + 2.0, h, color: bg });
            }
            // spaces are written too: without them a viewer copies and searches words run together
            self.page().push(Op::Text { x: px, y: baseline - s.rise, font: p.font, size: s.size, color: s.color, text: p.text.clone() });
            if s.underline {
                self.page().push(Op::Line { x0: px, y0: baseline - s.rise + s.size * 0.14, x1: px + p.w, y1: baseline - s.rise + s.size * 0.14, color: s.color, width: s.size * 0.055 });
            }
            if s.strike {
                self.page().push(Op::Line { x0: px, y0: baseline - s.rise - s.size * 0.3, x1: px + p.w, y1: baseline - s.rise - s.size * 0.3, color: s.color, width: s.size * 0.06 });
            }
            if let Some(url) = &s.link {
                self.page().push(Op::Link { x: px, y: top, w: p.w, h, url: url.to_string() });
            }
            px += p.w;
        }
    }

    /// Lays out inline content in the current column and moves down past it.
    fn flow(&mut self, spans: &[Span]) {
        let (x, width, align) = (self.x, self.width, self.align);
        for line in self.lay(spans, width) {
            self.need(line.height);
            let y = self.y;
            self.draw_line(spans, &line, x, y, width, align);
            self.advance(line.height);
        }
    }

    // ---------------------------------------------------------- pictures

    fn add_image(&mut self, mut rgba: Vec<u8>, w: usize, h: usize) -> Option<usize> {
        if w == 0 || h == 0 || rgba.len() < w * h * 4 {
            return None;
        }
        // the pixels come with their colours already multiplied by their opacity; a PDF
        // page is white, so put them on white
        for px in rgba.chunks_exact_mut(4) {
            let a = px[3];
            if a < 255 {
                px[0] = px[0].saturating_add(255 - a);
                px[1] = px[1].saturating_add(255 - a);
                px[2] = px[2].saturating_add(255 - a);
                px[3] = 255;
            }
        }
        self.images.push(krilla::image::Image::from_rgba8(rgba, w as u32, h as u32));
        Some(self.images.len() - 1)
    }

    /// A picture file or address: `(image, width, height)` in points.
    fn picture(&mut self, url: &str, width: Option<u32>) -> Option<(usize, f32, f32)> {
        let bytes = crate::assets::load_bytes(url, self.base).ok()?;
        let r = crate::assets::raster_image(&bytes, 2200).ok()?;
        let (nw, nh) = (r.size.x.max(1.0), r.size.y.max(1.0));
        // a pixel of a picture is three quarters of a point, as in a browser
        let w = width.map(|w| w as f32 * 0.75).unwrap_or(nw * 0.75);
        let h = w * nh / nw;
        Some((self.add_image(r.rgba, r.w, r.h)?, w, h))
    }

    fn svg_image(&mut self, svg: &str, max_w: f32) -> Option<(usize, f32, f32)> {
        let r = crate::assets::raster_svg(svg, 2.6, max_w, self.base).ok()?;
        let (w, h) = (r.size.x * 0.75, r.size.y * 0.75);
        Some((self.add_image(r.rgba, r.w, r.h)?, w, h))
    }

    /// A formula as a picture: `(image, width, height, depth below the baseline)`.
    fn math_image(&mut self, tex: &str, display: bool, size: f32) -> Option<(usize, f32, f32, f32)> {
        let px = 18.0;
        let (svg, w, h, depth) = crate::assets::math_svg(tex, display, px, egui::Color32::from_rgb(TEXT[0], TEXT[1], TEXT[2])).ok()?;
        let r = crate::assets::raster_svg(&svg, 3.5, 4000.0, None).ok()?;
        // the formula was set at `px`; bring it to the size of the text around it
        let k = size * if display { 1.15 } else { 1.0 } / px;
        Some((self.add_image(r.rgba, r.w, r.h)?, w * k, h * k, depth * k))
    }

    /// Places a picture as a block of its own, scaled down to fit the column and the page.
    fn place(&mut self, image: usize, w: f32, h: f32, center: bool) {
        let room = PAGE_H - MARGIN_TOP - MARGIN_BOTTOM;
        let k = (self.width / w).min(room / h).min(1.0);
        let (w, h) = (w * k, h * k);
        self.need(h);
        let x = if center || self.align == Align::Center { self.x + (self.width - w) / 2.0 } else { self.x };
        let y = self.y;
        self.page().push(Op::Image { x, y, w, h, image });
        self.advance(h);
    }

    // ------------------------------------------------------------ blocks

    fn blocks(&mut self, blocks: &[Block]) {
        for (i, b) in blocks.iter().enumerate() {
            if i > 0 {
                self.gap(BODY * 0.65 * self.scale);
            }
            self.block(b);
        }
    }

    fn block(&mut self, block: &Block) {
        let body = BODY * self.scale;
        match block {
            Block::Heading { level, inl, .. } => {
                let size = [22.0, 17.5, 14.5, 12.5, 11.5, 10.5][(*level as usize).clamp(1, 6) - 1] * self.scale;
                self.gap(size * 0.55);
                // a heading does not stay alone at the bottom of a page
                self.need(size * 1.5 + body * 3.0);
                self.outline.push((*level, crate::md::plain(inl), self.pages.len() - 1, self.y));
                let spans = self.spans_of(inl, true, size, TEXT);
                self.flow(&spans);
                if *level <= 2 {
                    self.advance(3.0);
                    let (x, y, w) = (self.x, self.y, self.width);
                    self.page().push(Op::Fill { x, y, w, h: 0.7, color: RULE });
                    self.advance(2.0);
                }
            }
            Block::Para(inl) => {
                let spans = self.spans_of(inl, false, body, TEXT);
                self.flow(&spans);
            }
            Block::Code { text, .. } => self.code(text),
            Block::Quote(body_blocks) => self.barred(RULE, |w| w.tinted(MUTED, body_blocks)),
            Block::Alert { kind, title, body: body_blocks } => {
                let (name, color): (&str, Rgb) = match kind {
                    AlertKind::Note => ("Note", [38, 110, 220]),
                    AlertKind::Tip => ("Tip", [30, 150, 80]),
                    AlertKind::Important => ("Important", [130, 80, 220]),
                    AlertKind::Warning => ("Warning", [190, 130, 0]),
                    AlertKind::Caution => ("Caution", [210, 60, 50]),
                };
                let title = title.clone().unwrap_or_else(|| name.to_string());
                self.barred(color, |w| {
                    let size = BODY * w.scale;
                    w.flow(&[Span::text(title, Face::Bold, size, color)]);
                    w.gap(size * 0.3);
                    w.blocks(body_blocks);
                });
            }
            Block::List { ordered, start, items, tight } => self.list(*ordered, *start, items, *tight),
            Block::Table { aligns, rows } => self.table(aligns, rows),
            Block::Rule => {
                self.need(10.0);
                self.advance(4.0);
                let (x, y, w) = (self.x, self.y, self.width);
                self.page().push(Op::Fill { x, y, w, h: 0.8, color: RULE });
                self.advance(5.0);
            }
            Block::Math(tex) => match self.math_image(tex, true, body) {
                Some((image, w, h, _)) => self.place(image, w, h, true),
                None => self.code(tex),
            },
            Block::Diagram { kind, src } => {
                let svg = match kind {
                    DiagramKind::Mermaid => crate::assets::mermaid_svg(src, false),
                    DiagramKind::Dot => crate::assets::dot_svg(src),
                    DiagramKind::Svg => Ok(src.clone()),
                    DiagramKind::Chart => crate::chart::chart_svg(src, false, 720.0),
                };
                match svg.ok().and_then(|svg| self.svg_image(&svg, 900.0)) {
                    Some((image, w, h)) => self.place(image, w, h, true),
                    // a diagram that cannot be drawn is still there, as its source
                    None => self.code(src),
                }
            }
            Block::Details { summary, body: body_blocks } => {
                let spans = self.spans_of(summary, true, body, TEXT);
                self.flow(&spans);
                self.gap(body * 0.4);
                self.indented(12.0, |w| w.blocks(body_blocks));
            }
            Block::Footnote { index, body: body_blocks, .. } => {
                let (x, y) = (self.x, self.y);
                self.need(body * LINE);
                let y = if self.y < y { self.y } else { y };
                self.page().push(Op::Text { x, y: y + body * 0.86 * 1.02, font: FontRef::Own(Face::Sans), size: body * 0.86, color: MUTED, text: format!("{index}.") });
                self.indented(18.0, |w| {
                    let before = w.scale;
                    w.scale *= 0.88;
                    w.tinted(MUTED, body_blocks);
                    w.scale = before;
                });
            }
            Block::DefList(defs) => {
                for (i, (term, descriptions)) in defs.iter().enumerate() {
                    if i > 0 {
                        self.gap(body * 0.5);
                    }
                    let spans = self.spans_of(term, true, body, TEXT);
                    self.flow(&spans);
                    for d in descriptions {
                        self.gap(body * 0.2);
                        self.indented(16.0, |w| w.blocks(d));
                    }
                }
            }
            Block::FrontMatter(pairs) => {
                let key_w = pairs.iter().map(|(k, _)| self.fonts.width(FontRef::Own(Face::Bold), &k.to_uppercase(), body * 0.8)).fold(0.0, f32::max) + 14.0;
                for (k, v) in pairs {
                    let key = [Span::text(k.to_uppercase(), Face::Bold, body * 0.8, FAINT)];
                    let value = [Span::text(v.clone(), Face::Sans, body * 0.95, MUTED)];
                    let (x, width) = (self.x, self.width);
                    let lines = self.lay(&value, (width - key_w).max(60.0));
                    for (n, line) in lines.iter().enumerate() {
                        self.need(line.height);
                        let y = self.y;
                        if n == 0 {
                            let kl = self.lay(&key, key_w);
                            if let Some(first) = kl.first() {
                                self.draw_line(&key, first, x, y + (line.ascent - first.ascent), key_w, Align::Left);
                            }
                        }
                        self.draw_line(&value, line, x + key_w, y, width - key_w, Align::Left);
                        self.advance(line.height);
                    }
                }
                self.advance(4.0);
                let (x, y, w) = (self.x, self.y, self.width);
                self.page().push(Op::Fill { x, y, w, h: 0.7, color: RULE });
                self.advance(2.0);
            }
            Block::Center(body_blocks) => {
                let before = self.align;
                self.align = Align::Center;
                self.blocks(body_blocks);
                self.align = before;
            }
            Block::Image { url, alt, width, link, .. } => match self.picture(url, *width) {
                Some((image, w, h)) => {
                    let (x0, y0, page) = (self.x, self.y, self.pages.len());
                    self.place(image, w, h, false);
                    if let (Some(url), true) = (link, page == self.pages.len()) {
                        let (w, h) = (w.min(self.width), self.y - y0);
                        self.page().push(Op::Link { x: x0, y: y0, w, h, url: url.to_string() });
                    }
                }
                None => self.flow(&[Span::text(format!("[{}]", if alt.is_empty() { url } else { alt }), Face::Italic, body, MUTED)]),
            },
        }
    }

    /// Blocks drawn in a quieter colour (quotes, footnotes).
    fn tinted(&mut self, color: Rgb, blocks: &[Block]) {
        for (i, b) in blocks.iter().enumerate() {
            if i > 0 {
                self.gap(BODY * 0.65 * self.scale);
            }
            match b {
                Block::Para(inl) => {
                    let size = BODY * self.scale;
                    let spans = self.spans_of(inl, false, size, color);
                    self.flow(&spans);
                }
                other => self.block(other),
            }
        }
    }

    fn indented(&mut self, by: f32, f: impl FnOnce(&mut Self)) {
        self.x += by;
        self.width -= by;
        f(self);
        self.x -= by;
        self.width += by;
    }

    /// Content with a vertical bar at its left.
    fn barred(&mut self, color: Rgb, f: impl FnOnce(&mut Self)) {
        self.need(BODY * LINE * 2.0);
        self.bars.push((self.x, color));
        self.indented(12.0, f);
        self.bars.pop();
    }

    fn code(&mut self, text: &str) {
        let size = 8.8 * self.scale;
        let pad = 7.0;
        let (x, width) = (self.x, self.width);
        let lines: Vec<&str> = text.trim_end_matches('\n').split('\n').collect();
        let mut first = true;
        self.need(size * LINE + 2.0 * pad);
        for (n, raw) in lines.iter().enumerate() {
            let span = [Span::text(raw.replace('\t', "    "), Face::Mono, size, TEXT)];
            let laid = self.lay(&span, width - 2.0 * pad);
            for (k, line) in laid.iter().enumerate() {
                let last = n + 1 == lines.len() && k + 1 == laid.len();
                let before = self.pages.len();
                self.need(line.height + if last { pad } else { 0.0 });
                let top_pad = if first || self.pages.len() != before { pad } else { 0.0 };
                first = false;
                let y = self.y;
                let h = top_pad + line.height + if last { pad } else { 0.0 };
                self.page().push(Op::Fill { x, y, w: width, h: h + 0.3, color: CODE_BG });
                self.draw_line(&span, line, x + pad, y + top_pad, width - 2.0 * pad, Align::Left);
                self.advance(h);
            }
        }
    }

    fn list(&mut self, ordered: bool, start: usize, items: &[Item], tight: bool) {
        let body = BODY * self.scale;
        let indent = if ordered { self.fonts.width(FontRef::Own(Face::Sans), &format!("{}.", start + items.len().saturating_sub(1)), body) + 7.0 } else { 14.0 };
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                self.gap(if tight { body * 0.22 } else { body * 0.6 });
            }
            self.need(body * LINE);
            let (x, y) = (self.x, self.y);
            let baseline = y + body * 1.02;
            match item.task {
                Some(done) => {
                    let s = body * 0.82;
                    let (bx, by) = (x + 1.0, baseline - s + 0.5);
                    if done {
                        self.page().push(Op::Fill { x: bx, y: by, w: s, h: s, color: LINK });
                        self.page().push(Op::Line { x0: bx + s * 0.2, y0: by + s * 0.52, x1: bx + s * 0.42, y1: by + s * 0.74, color: [255, 255, 255], width: 1.1 });
                        self.page().push(Op::Line { x0: bx + s * 0.42, y0: by + s * 0.74, x1: bx + s * 0.8, y1: by + s * 0.26, color: [255, 255, 255], width: 1.1 });
                    } else {
                        self.page().push(Op::Frame { x: bx, y: by, w: s, h: s, color: FAINT, width: 0.8 });
                    }
                }
                None if ordered => self.page().push(Op::Text { x, y: baseline, font: FontRef::Own(Face::Sans), size: body, color: MUTED, text: format!("{}.", start + i) }),
                None => self.page().push(Op::Fill { x: x + 3.0, y: baseline - body * 0.42, w: body * 0.26, h: body * 0.26, color: MUTED }),
            }
            self.indented(indent, |w| {
                for (k, b) in item.blocks.iter().enumerate() {
                    if k > 0 {
                        w.gap(if tight { body * 0.22 } else { body * 0.6 });
                    }
                    w.block(b);
                }
            });
            // an item with nothing in it still takes a line
            if self.y <= y + 0.1 && self.pages.len() > 0 && item.blocks.is_empty() {
                self.advance(body * LINE);
            }
        }
    }

    fn table(&mut self, aligns: &[CellAlign], rows: &[Vec<Vec<Inl>>]) {
        let size = BODY * 0.93 * self.scale;
        let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        if cols == 0 {
            return;
        }
        let pad = 5.0;
        // what every cell would like, and what it needs at least (its longest word)
        let mut want = vec![0.0f32; cols];
        let mut least = vec![0.0f32; cols];
        let mut cells: Vec<Vec<Vec<Span>>> = Vec::new();
        for (r, row) in rows.iter().enumerate() {
            let mut out = Vec::new();
            for c in 0..cols {
                let spans = row.get(c).map(|inl| self.spans_of(inl, r == 0, size, TEXT)).unwrap_or_default();
                let one = self.lay(&spans, 100_000.0);
                want[c] = want[c].max(one.iter().map(|l| l.width).fold(0.0, f32::max));
                let narrow = self.lay(&spans, 1.0);
                least[c] = least[c].max(narrow.iter().map(|l| l.width).fold(0.0, f32::max).min(120.0));
                out.push(spans);
            }
            cells.push(out);
        }
        let room = self.width - cols as f32 * 2.0 * pad;
        let total: f32 = want.iter().sum();
        let widths: Vec<f32> = if total <= room {
            want.clone()
        } else {
            // give every column its least, and share the rest by what each still wants
            let base: f32 = least.iter().sum();
            let spare = (room - base).max(0.0);
            let wish: f32 = want.iter().zip(&least).map(|(w, l)| (w - l).max(0.0)).sum();
            want.iter().zip(&least).map(|(w, l)| if base > room { room / cols as f32 } else { l + if wish > 0.0 { spare * (w - l).max(0.0) / wish } else { 0.0 } }).collect()
        };
        let table_w: f32 = widths.iter().sum::<f32>() + cols as f32 * 2.0 * pad;
        let x0 = if self.align == Align::Center { self.x + (self.width - table_w) / 2.0 } else { self.x };
        for (r, row) in cells.iter().enumerate() {
            let laid: Vec<Vec<LaidLine>> = row.iter().enumerate().map(|(c, spans)| self.lay(spans, widths[c])).collect();
            let h = laid.iter().map(|l| l.iter().map(|x| x.height).sum::<f32>()).fold(size * LINE, f32::max) + 2.0 * pad * 0.7;
            self.need(h);
            let y = self.y;
            if r == 0 {
                self.page().push(Op::Fill { x: x0, y, w: table_w, h, color: CODE_BG });
                self.page().push(Op::Fill { x: x0, y, w: table_w, h: 0.6, color: RULE });
            }
            let mut cx = x0;
            for (c, lines) in laid.iter().enumerate() {
                let align = match aligns.get(c) {
                    Some(CellAlign::Center) => Align::Center,
                    Some(CellAlign::Right) => Align::Right,
                    _ => Align::Left,
                };
                let mut ly = y + pad * 0.7;
                for line in lines {
                    self.draw_line(&row[c], line, cx + pad, ly, widths[c], align);
                    ly += line.height;
                }
                self.page().push(Op::Fill { x: cx, y, w: 0.6, h, color: RULE });
                cx += widths[c] + 2.0 * pad;
            }
            self.page().push(Op::Fill { x: x0 + table_w - 0.6, y, w: 0.6, h, color: RULE });
            self.page().push(Op::Fill { x: x0, y: y + h - 0.6, w: table_w, h: 0.6, color: RULE });
            self.advance(h);
        }
    }

    // -------------------------------------------------------------- output

    fn finish(mut self, title: &str) -> Result<Vec<u8>, String> {
        let pages = std::mem::take(&mut self.pages);
        let total = pages.len();
        let mut doc = Document::new();
        let paint = |c: Rgb| -> krilla::paint::Paint { rgb::Color::new(c[0], c[1], c[2]).into() };
        let fill = |c: Rgb| Fill { paint: paint(c), opacity: NormalizedF32::ONE, rule: Default::default() };
        for (n, ops) in pages.iter().enumerate() {
            let mut page = doc.start_page_with(PageSettings::from_wh(PAGE_W, PAGE_H).ok_or("Bad page size.")?);
            let mut links: Vec<(Rect, String)> = Vec::new();
            {
                let mut s = page.surface();
                // boxes and pictures first, then text, so that a background never covers a letter
                for op in ops {
                    match op {
                        Op::Fill { x, y, w, h, color } => {
                            if let Some(r) = Rect::from_xywh(*x, *y, w.max(0.01), h.max(0.01)) {
                                let mut pb = PathBuilder::new();
                                pb.push_rect(r);
                                if let Some(path) = pb.finish() {
                                    s.set_stroke(None);
                                    s.set_fill(Some(fill(*color)));
                                    s.draw_path(&path);
                                }
                            }
                        }
                        Op::Image { x, y, w, h, image } => {
                            if let (Some(size), Some(img)) = (Size::from_wh(*w, *h), self.images.get(*image)) {
                                s.push_transform(&Transform::from_translate(*x, *y));
                                s.draw_image(img.clone(), size);
                                s.pop();
                            }
                        }
                        _ => {}
                    }
                }
                for op in ops {
                    match op {
                        Op::Frame { x, y, w, h, color, width } => {
                            if let Some(r) = Rect::from_xywh(*x, *y, w.max(0.01), h.max(0.01)) {
                                let mut pb = PathBuilder::new();
                                pb.push_rect(r);
                                if let Some(path) = pb.finish() {
                                    s.set_fill(None);
                                    s.set_stroke(Some(Stroke { paint: paint(*color), width: *width, ..Default::default() }));
                                    s.draw_path(&path);
                                }
                            }
                        }
                        Op::Line { x0, y0, x1, y1, color, width } => {
                            let mut pb = PathBuilder::new();
                            pb.move_to(*x0, *y0);
                            pb.line_to(*x1, *y1);
                            if let Some(path) = pb.finish() {
                                s.set_fill(None);
                                s.set_stroke(Some(Stroke { paint: paint(*color), width: *width, ..Default::default() }));
                                s.draw_path(&path);
                            }
                        }
                        Op::Text { x, y, font, size, color, text } => {
                            s.set_stroke(None);
                            s.set_fill(Some(fill(*color)));
                            s.draw_text(Point::from_xy(*x, *y), self.fonts.loaded(*font).font.clone(), *size, text, false, TextDirection::Auto);
                        }
                        Op::Link { x, y, w, h, url } => {
                            if let Some(r) = Rect::from_xywh(*x, *y, w.max(0.5), h.max(0.5)) {
                                links.push((r, url.clone()));
                            }
                        }
                        _ => {}
                    }
                }
                // page number
                if total > 1 {
                    let label = format!("{} / {total}", n + 1);
                    let w = self.fonts.width(FontRef::Own(Face::Sans), &label, 8.0);
                    s.set_stroke(None);
                    s.set_fill(Some(fill(FAINT)));
                    s.draw_text(Point::from_xy((PAGE_W - w) / 2.0, PAGE_H - 30.0), self.fonts.own[0].font.clone(), 8.0, &label, false, TextDirection::Auto);
                }
                s.finish();
            }
            for (rect, url) in links {
                // links to the web and to mail; a link to a place in the document has no target here
                if url.contains("://") || url.starts_with("mailto:") {
                    let target = krilla::annotation::Target::Action(krilla::action::LinkAction::new(url).into());
                    page.add_annotation(krilla::annotation::Annotation::new_link(krilla::annotation::LinkAnnotation::new(rect, target), None));
                }
            }
            page.finish();
        }
        // bookmarks from the headings
        if !self.outline.is_empty() {
            let mut outline = krilla::outline::Outline::new();
            let mut stack: Vec<(u8, krilla::outline::OutlineNode)> = Vec::new();
            let close = |stack: &mut Vec<(u8, krilla::outline::OutlineNode)>, outline: &mut krilla::outline::Outline, down_to: u8| {
                while stack.last().is_some_and(|t| t.0 >= down_to) {
                    let (_, node) = stack.pop().unwrap();
                    match stack.last_mut() {
                        Some(parent) => parent.1.push_child(node),
                        None => outline.push_child(node),
                    }
                }
            };
            for (level, text, page, y) in &self.outline {
                close(&mut stack, &mut outline, *level);
                let dest = krilla::destination::XyzDestination::new(*page, Point::from_xy(0.0, (*y - 12.0).max(0.0)));
                stack.push((*level, krilla::outline::OutlineNode::new(text.clone(), dest)));
            }
            close(&mut stack, &mut outline, 0);
            doc.set_outline(outline);
        }
        doc.set_metadata(krilla::metadata::Metadata::new().title(title.to_string()).creator(format!("Inkdown {}", env!("CARGO_PKG_VERSION"))));
        doc.finish().map_err(|e| format!("The PDF could not be written ({e:?})."))
    }
}

/// A Markdown document as a PDF file.
pub fn markdown_to_pdf(text: &str, title: &str, base: Option<&Path>) -> Result<Vec<u8>, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let tops = crate::md::parse(text);
        let mut w = Writer::new(base)?;
        let blocks: Vec<Block> = tops.into_iter().map(|t| t.block).collect();
        w.blocks(&blocks);
        w.finish(title)
    }))
    .unwrap_or_else(|_| Err("The PDF could not be written.".into()))
}

/// Plain text or source code as a PDF file, in the code font.
pub fn text_to_pdf(text: &str, title: &str) -> Result<Vec<u8>, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut w = Writer::new(None)?;
        let size = 8.6;
        for raw in text.trim_end_matches('\n').split('\n') {
            let span = [Span::text(raw.replace('\t', "    "), Face::Mono, size, TEXT)];
            w.flow(&span);
        }
        w.finish(title)
    }))
    .unwrap_or_else(|_| Err("The PDF could not be written.".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pages_of(pdf: &[u8]) -> Vec<String> {
        let bytes: Arc<[u8]> = pdf.to_vec().into();
        let n = crate::pdfview::page_sizes(&bytes, "").expect("our own reader opens what we wrote").len();
        (0..n).map(|p| crate::pdfview::page_text(&bytes, "", p).unwrap().text).collect()
    }

    #[test]
    fn a_document_becomes_pages_of_real_text() {
        let md = "---\ntitle: Report\n---\n\n# Trip plan\n\nPack **light** and bring a *map*. See [the site](https://example.com/x) and `code`.\n\n## Lists\n\n- one\n- two\n  - nested\n\n1. first\n2. second\n\n- [x] done\n- [ ] open\n\n> A quote\n> over two lines.\n\n> [!WARNING]\n> Careful.\n\n| Day | Place |\n| --- | ---: |\n| 1 | Rome |\n| 2 | Florence and the hills around it |\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n\n---\n\nH~2~O and E = mc^2^. ==marked== ~~gone~~.\n\nGrüße, naïve café — “quotes”.\n";
        let pdf = markdown_to_pdf(md, "Report", None).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        let pages = pages_of(&pdf);
        assert_eq!(pages.len(), 1);
        let text = &pages[0];
        for expected in ["Trip plan", "Pack light and bring a map.", "the site", "Lists", "nested", "1.", "second", "done", "A quote", "Warning", "Careful.", "Florence and the hills around it", "println!(\"hi\");", "marked", "Grüße, naïve café"] {
            assert!(text.contains(expected), "missing {expected:?} in:\n{text}");
        }
        // the page is A4 and the link can be followed
        let bytes: Arc<[u8]> = pdf.clone().into();
        let size = crate::pdfview::page_sizes(&bytes, "").unwrap()[0];
        assert_eq!((size.x.round(), size.y.round()), (595.0, 842.0));
        assert!(pdf.windows(21).any(|w| w == b"https://example.com/x") || String::from_utf8_lossy(&pdf).contains("example.com"), "the link target is in the file");
    }

    #[test]
    fn long_documents_break_into_pages_and_keep_every_line() {
        let mut md = String::from("# Long\n\n");
        for i in 0..400 {
            md.push_str(&format!("Paragraph number {i} has a few words in it so that it takes some room on the page.\n\n"));
        }
        let pdf = markdown_to_pdf(&md, "Long", None).unwrap();
        let pages = pages_of(&pdf);
        assert!(pages.len() > 5, "{} pages", pages.len());
        let all = pages.join("\n");
        for i in [0, 1, 199, 398, 399] {
            assert!(all.contains(&format!("Paragraph number {i} has")), "paragraph {i} is missing");
        }
        // every page says which one it is
        assert!(pages[0].contains(&format!("1 / {}", pages.len())) && pages[2].contains(&format!("3 / {}", pages.len())));
        // nothing is written below the bottom margin except the page number
        let bytes: Arc<[u8]> = pdf.into();
        let t = crate::pdfview::page_text(&bytes, "", 1).unwrap();
        let lowest = t.boxes.iter().zip(t.text.chars()).filter(|(b, _)| b.width() > 0.0 && b.top() < PAGE_H - 40.0).map(|(b, _)| b.bottom()).fold(0.0, f32::max);
        assert!(lowest <= PAGE_H - MARGIN_BOTTOM + 6.0, "text reaches {lowest}");
    }

    #[test]
    fn long_words_code_and_plain_text() {
        let long = "x".repeat(400);
        let md = format!("A {long} word.\n\n```\n{}\n```\n", "let very_long_line = 1; ".repeat(20));
        let pdf = markdown_to_pdf(&md, "Wide", None).unwrap();
        let bytes: Arc<[u8]> = pdf.into();
        let t = crate::pdfview::page_text(&bytes, "", 0).unwrap();
        let right = t.boxes.iter().filter(|b| b.width() > 0.0).map(|b| b.right()).fold(0.0, f32::max);
        assert!(right <= PAGE_W - MARGIN_X + 4.0, "text runs to {right}, past the right margin");
        assert_eq!(t.text.matches('x').count(), 400, "no letter of the long word is lost");
        // source code as a listing
        let code: String = (0..200).map(|i| format!("line {i}: let x = {i};\n")).collect();
        let pdf = text_to_pdf(&code, "code.rs").unwrap();
        let pages = pages_of(&pdf);
        assert!(pages.len() >= 2);
        assert!(pages[0].contains("line 0: let x = 0;") && pages.last().unwrap().contains("line 199: let x = 199;"));
        assert!(markdown_to_pdf("", "Empty", None).is_ok());
    }
}
