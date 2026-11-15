//! Reading PDF files: pages are drawn by a pure-Rust renderer on the background workers
//! and shown one under another, fitted to the width of the pane. The text of the pages is
//! read as well, so that it can be searched, selected and copied.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::sync::Arc;

use egui::{pos2, vec2, Color32, CornerRadius, Id, Rect, Sense, Ui, Vec2};
use hayro::hayro_interpret::{interpret_page, BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps, InterpreterCache, InterpreterSettings, SoftMask};
use hayro::hayro_interpret::font::GlyphRun;
use hayro::hayro_syntax::Pdf;
use hayro::kurbo::{Affine, BezPath, Point};
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings};

use crate::assets::{hash64, Job, Raster, State};
use crate::preview::ViewCtx;

/// Widest or tallest a rendered page may be, in pixels (a GPU texture limit on phones).
const MAX_PX: f32 = 4096.0;
const GAP: f32 = 14.0;
/// The error text that means: ask for the password.
pub const NEEDS_PASSWORD: &str = "This PDF is protected by a password.";

/// The text of one page with the place of every character, in page points from the top left.
#[derive(Default)]
pub struct PageText {
    pub text: String,
    /// One box per char of `text`; spaces and line ends that were added have empty boxes.
    pub boxes: Vec<Rect>,
}

#[derive(Default)]
pub struct PdfView {
    /// Page sizes in points, or why the file cannot be read.
    pages: Option<Result<Vec<Vec2>, String>>,
    /// 0 = fit the width of the pane; otherwise points on screen per PDF point.
    pub zoom: f32,
    /// The last picture shown for each page, kept on screen while a sharper one is made.
    shown: HashMap<usize, u64>,
    /// The page under the middle of the view, for the dump and the page indicator.
    pub current: usize,
    /// Scroll to this page, and this far down it (in page points).
    go_to: Option<(usize, f32)>,
    /// The password typed for a protected file (kept in memory only).
    password: String,
    password_draft: String,
    wrong_password: bool,
    /// Text of the pages read so far.
    texts: HashMap<usize, Arc<PageText>>,
    text_job: Option<Receiver<(usize, PageText)>>,
    pub find_open: bool,
    pub find_focus: bool,
    pub query: String,
    /// Matches as `(page, first char, one past the last char)`, in reading order.
    pub matches: Vec<(usize, usize, usize)>,
    match_key: (String, usize),
    pub current_match: Option<usize>,
    /// Selected text on one page: `(page, anchor char, head char)`.
    selection: Option<(usize, usize, usize)>,
    /// Where the first page was drawn last, and at what scale (for tests).
    pub first_page: Option<(f32, f32, f32)>,
    /// After a zoom by double tap: keep this point of a page (in page points) at this
    /// place of the view (in screen points from its top left corner).
    zoom_at: Option<(usize, egui::Pos2, Vec2)>,
}

/// Scroll offsets that put the point `at` of page `page` at `on_screen` in the view.
fn offsets_for(sizes: &[Vec2], scale: f32, content_w: f32, page: usize, at: egui::Pos2, on_screen: Vec2) -> Vec2 {
    let above: f32 = sizes.iter().take(page).map(|s| s.y * scale + GAP).sum::<f32>() + GAP;
    let page_w = sizes.get(page).map(|s| s.x).unwrap_or(0.0) * scale;
    let left = (content_w - page_w) / 2.0;
    vec2((left + at.x * scale - on_screen.x).max(0.0), (above + at.y * scale - on_screen.y).max(0.0))
}

impl PdfView {
    pub fn page_count(&self) -> usize {
        match &self.pages {
            Some(Ok(p)) => p.len(),
            _ => 0,
        }
    }

    pub fn error(&self) -> Option<&str> {
        match &self.pages {
            Some(Err(e)) => Some(e),
            _ => None,
        }
    }

    pub fn needs_password(&self) -> bool {
        self.error() == Some(NEEDS_PASSWORD)
    }

    /// Number of pages whose text has been read.
    pub fn text_pages(&self) -> usize {
        self.texts.len()
    }

    pub fn go_to(&mut self, page: usize) {
        self.go_to = Some((page.min(self.page_count().saturating_sub(1)), 0.0));
    }

    /// A double tap: zoom in around the tapped point, or back out to the full width.
    fn toggle_zoom(&mut self, fitted: f32, page: usize, at: egui::Pos2, on_screen: Vec2) {
        let zoomed = self.zoom > 0.0 && self.zoom > fitted * 1.05;
        self.zoom = if zoomed { 0.0 } else { (fitted * 2.2).clamp(0.2, 6.0) };
        self.zoom_at = Some((page, at, on_screen));
    }

    pub fn zoom_by(&mut self, factor: f32, fitted: f32) {
        let now = if self.zoom > 0.0 { self.zoom } else { fitted };
        self.zoom = (now * factor).clamp(0.2, 6.0);
    }

    /// Tries a password on a protected file.
    pub fn unlock(&mut self, password: &str) {
        self.password = password.to_string();
        self.pages = None;
        self.wrong_password = true; // cleared when the file opens
    }

    pub fn open_find(&mut self) {
        self.find_open = true;
        self.find_focus = true;
    }

    /// Moves to the next or previous match.
    pub fn find_step(&mut self, forward: bool) {
        if self.matches.is_empty() {
            return;
        }
        let n = self.matches.len();
        let next = match self.current_match {
            Some(i) if forward => (i + 1) % n,
            Some(i) => (i + n - 1) % n,
            // start from the page in view
            None if forward => self.matches.iter().position(|m| m.0 >= self.current).unwrap_or(0),
            None => self.matches.iter().rposition(|m| m.0 <= self.current).unwrap_or(n - 1),
        };
        self.current_match = Some(next);
        let (page, a, _) = self.matches[next];
        let y = self.texts.get(&page).and_then(|t| t.boxes.get(a)).map(|b| b.top()).unwrap_or(0.0);
        self.go_to = Some((page, (y - 80.0).max(0.0)));
    }

    pub fn selected_text(&self) -> Option<String> {
        let (page, a, b) = self.selection?;
        let (a, b) = (a.min(b), a.max(b));
        let t = self.texts.get(&page)?;
        let s: String = t.text.chars().skip(a).take(b + 1 - a).collect();
        (!s.trim().is_empty()).then_some(s)
    }

    /// All the text read so far, page after page.
    pub fn all_text(&self) -> String {
        let mut pages: Vec<(&usize, &Arc<PageText>)> = self.texts.iter().collect();
        pages.sort_by_key(|p| *p.0);
        pages.iter().map(|p| p.1.text.trim_end()).collect::<Vec<_>>().join("\n\n")
    }

    fn collect_text(&mut self) {
        let Some(rx) = &self.text_job else { return };
        loop {
            match rx.try_recv() {
                Ok((page, text)) => {
                    self.texts.insert(page, Arc::new(text));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(_) => {
                    self.text_job = None;
                    break;
                }
            }
        }
    }

    fn update_matches(&mut self) {
        let key = (self.query.clone(), self.texts.len());
        if key == self.match_key {
            return;
        }
        let restart = key.0 != self.match_key.0;
        self.match_key = key;
        self.matches.clear();
        let needle: Vec<char> = self.query.to_lowercase().chars().collect();
        if needle.is_empty() {
            self.current_match = None;
            return;
        }
        let mut pages: Vec<usize> = self.texts.keys().copied().collect();
        pages.sort_unstable();
        for page in pages {
            let hay: Vec<char> = self.texts[&page].text.chars().flat_map(|c| c.to_lowercase().next()).collect();
            let mut i = 0;
            while i + needle.len() <= hay.len() {
                if hay[i..i + needle.len()] == needle[..] {
                    self.matches.push((page, i, i + needle.len()));
                    i += needle.len();
                } else {
                    i += 1;
                }
            }
        }
        if restart {
            self.current_match = None;
        } else if self.current_match.is_some_and(|i| i >= self.matches.len()) {
            self.current_match = None;
        }
    }
}

thread_local! {
    /// The document each worker read last, so that pages of one file do not each parse it again.
    static OPEN: RefCell<Option<((usize, usize, u64), Pdf)>> = const { RefCell::new(None) };
}

fn with_pdf<T>(bytes: &Arc<[u8]>, password: &str, f: impl FnOnce(&Pdf) -> Result<T, String>) -> Result<T, String> {
    let id = (bytes.as_ptr() as usize, bytes.len(), hash64(&[&password]));
    OPEN.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.as_ref().is_none_or(|(k, _)| *k != id) {
            *slot = None;
            let pdf = Pdf::new_with_password(Arc::new(bytes.clone()), password).map_err(|e| {
                let text = format!("{e:?}");
                if text.contains("PasswordProtected") {
                    NEEDS_PASSWORD.to_string()
                } else if text.contains("Decryption") {
                    "This PDF is encrypted in a way Inkdown cannot read.".to_string()
                } else {
                    "This file is not a PDF that Inkdown can read.".to_string()
                }
            })?;
            *slot = Some((id, pdf));
        }
        f(&slot.as_ref().unwrap().1)
    })
}

fn guarded<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| Err("This PDF could not be read.".into()))
}

/// Page sizes in points.
pub fn page_sizes(bytes: &Arc<[u8]>, password: &str) -> Result<Vec<Vec2>, String> {
    guarded(|| {
        with_pdf(bytes, password, |pdf| {
            let sizes: Vec<Vec2> = pdf.pages().iter().map(|p| p.render_dimensions()).map(|(w, h)| vec2(w.max(1.0), h.max(1.0))).collect();
            if sizes.is_empty() {
                return Err("This PDF has no pages.".into());
            }
            Ok(sizes)
        })
    })
}

/// Draws one page `px_w` pixels wide.
pub fn render_page(bytes: &Arc<[u8]>, password: &str, page: usize, px_w: f32) -> Result<Raster, String> {
    guarded(|| {
        with_pdf(bytes, password, |pdf| {
            let page = pdf.pages().get(page).ok_or("No such page.")?;
            let (w, h) = page.render_dimensions();
            let scale = (px_w / w.max(1.0)).min(MAX_PX / w.max(1.0)).min(MAX_PX / h.max(1.0)).max(0.05);
            let cache = RenderCache::new();
            let pix = hayro::render(page, &cache, &InterpreterSettings::default(), &RenderSettings::default(), &PixmapSettings { x_scale: scale, y_scale: scale, bg_color: WHITE });
            let (pw, ph) = (pix.width() as usize, pix.height() as usize);
            if pw == 0 || ph == 0 {
                return Err("Empty page.".into());
            }
            // the page was drawn on opaque white, so premultiplied and plain colours agree
            Ok(Raster { rgba: pix.data_as_u8_slice().to_vec(), w: pw, h: ph, size: vec2(w, h), depth: 0.0, svg: None })
        })
    })
}

fn job_page(bytes: Arc<[u8]>, password: String, page: usize, px_w: f32) -> Job {
    Box::new(move || render_page(&bytes, &password, page, px_w))
}

// ------------------------------------------------------------------- text

/// Receives what a page draws and keeps only its text: each glyph, where it stands and how tall it is.
#[derive(Default)]
struct TextCollector {
    glyphs: Vec<(String, f32, f32, f32)>,
}

impl<'a> Device<'a> for TextCollector {
    fn draw_path(&mut self, _: &BezPath, _: DrawProps<'a>, _: &DrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn draw_glyph_run(&mut self, run: &GlyphRun<'_, 'a>, props: DrawProps<'a>, _: &DrawMode) {
        for g in run.glyphs() {
            let Some(unicode) = g.as_unicode() else { continue };
            let text = match unicode {
                hayro::hayro_interpret::hayro_cmap::BfString::Char(c) => c.to_string(),
                hayro::hayro_interpret::hayro_cmap::BfString::String(s) => s,
            };
            if text.is_empty() || text.chars().all(|c| c.is_control()) {
                continue;
            }
            let t = props.transform * g.transform();
            let origin = t * Point::new(0.0, 0.0);
            let up = t * Point::new(0.0, 1.0);
            let size = ((up.x - origin.x).powi(2) + (up.y - origin.y).powi(2)).sqrt();
            // glyph space is a thousandth of the font size in some fonts and the size itself in others
            let size = if size < 0.5 { size * 1000.0 } else { size };
            self.glyphs.push((text, origin.x as f32, origin.y as f32, (size as f32).clamp(1.0, 400.0)));
        }
    }
    fn draw_image(&mut self, _: Image<'a, '_>, _: ImageDrawProps<'a>) {}
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}

/// Turns glyphs in drawing order into lines of text with a box for every character.
fn assemble(glyphs: Vec<(String, f32, f32, f32)>) -> PageText {
    let mut out = PageText::default();
    let push = |out: &mut PageText, s: &str, r: Rect| {
        for (k, c) in s.chars().enumerate() {
            out.text.push(c);
            // several chars from one glyph (a ligature) share its box
            out.boxes.push(if k == 0 { r } else { Rect::from_min_size(r.right_top(), vec2(0.0, r.height())) });
        }
    };
    let n = glyphs.len();
    for i in 0..n {
        let (text, x, y, size) = &glyphs[i];
        if i > 0 {
            let (ptext, px, py, psize) = &glyphs[i - 1];
            let line_height = size.max(*psize);
            if (y - py).abs() > line_height * 0.6 {
                push(&mut out, "\n", Rect::from_min_size(pos2(*px, *py), Vec2::ZERO));
            } else if !ptext.ends_with(' ') && !text.starts_with(' ') {
                // a gap wider than a letter, or a jump backwards: separate words
                let gap = x - px;
                if gap > psize * 1.15 || gap < -psize * 0.5 {
                    push(&mut out, " ", Rect::from_min_size(pos2(*px, *py), Vec2::ZERO));
                }
            }
        }
        // a glyph is as wide as the way to the next one on its line
        let width = match glyphs.get(i + 1) {
            Some((_, nx, ny, _)) if (ny - y).abs() < size * 0.6 && *nx > *x && nx - x < size * 1.6 => nx - x,
            _ => size * 0.55,
        };
        push(&mut out, text, Rect::from_min_max(pos2(*x, y - size * 0.82), pos2(x + width, y + size * 0.24)));
    }
    out
}

/// The text of one page.
pub fn page_text(bytes: &Arc<[u8]>, password: &str, page: usize) -> Result<PageText, String> {
    guarded(|| {
        with_pdf(bytes, password, |pdf| {
            let page = pdf.pages().get(page).ok_or("No such page.")?;
            let (w, h) = page.render_dimensions();
            let cache = InterpreterCache::new();
            let transform = Affine::new(page_transform(page));
            let mut context = Context::new(transform, hayro::kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), &cache, page.xref(), InterpreterSettings::default());
            let mut device = TextCollector::default();
            interpret_page(page, &mut context, &mut device);
            Ok(assemble(device.glyphs))
        })
    })
}

/// The transform from the page's own coordinates to points from its top left corner.
fn page_transform(page: &hayro::hayro_syntax::page::Page<'_>) -> [f64; 6] {
    use hayro::hayro_interpret::util::TransformExt;
    page.initial_transform(true).to_kurbo().as_coeffs()
}

/// Reads the text of every page on a thread of its own, in order.
fn start_text_job(bytes: Arc<[u8]>, password: String, pages: usize, ctx: egui::Context) -> Option<Receiver<(usize, PageText)>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("ink-pdf-text".into())
        .spawn(move || {
            for page in 0..pages {
                let text = page_text(&bytes, &password, page).unwrap_or_default();
                if tx.send((page, text)).is_err() {
                    return;
                }
                if page % 8 == 0 {
                    ctx.request_repaint();
                }
            }
            ctx.request_repaint();
        })
        .ok()?;
    Some(rx)
}

/// Screen rectangles covering chars `a..b` of a page, one per line.
fn char_rects(text: &PageText, a: usize, b: usize, origin: egui::Pos2, scale: f32) -> Vec<Rect> {
    let mut out: Vec<Rect> = Vec::new();
    for r in text.boxes.iter().skip(a).take(b.saturating_sub(a)).filter(|r| r.width() > 0.0) {
        let s = Rect::from_min_max(origin + r.min.to_vec2() * scale, origin + r.max.to_vec2() * scale);
        match out.last_mut() {
            Some(last) if (last.center().y - s.center().y).abs() < s.height() * 0.5 && s.left() <= last.right() + s.height() => *last = last.union(s),
            _ => out.push(s),
        }
    }
    out
}

/// The char of a page nearest to a point given in page points.
fn char_at(text: &PageText, p: egui::Pos2) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (i, r) in text.boxes.iter().enumerate().filter(|(_, r)| r.width() > 0.0) {
        // being on the right line matters more than being near sideways
        let d = (r.center().y - p.y).abs() * 4.0 + if r.x_range().contains(p.x) { 0.0 } else { (r.center().x - p.x).abs() };
        if best.is_none_or(|b| d < b.1) {
            best = Some((i, d));
        }
    }
    best.map(|b| b.0)
}

/// The pages of a PDF, scrollable and zoomable.
pub fn show(ui: &mut Ui, view: &mut PdfView, id: Id, bytes: &Arc<[u8]>, cx: &mut ViewCtx<'_>) {
    let pal = *cx.pal;
    let password = view.password.clone();
    let sizes = match view.pages.get_or_insert_with(|| page_sizes(bytes, &password)) {
        Ok(s) => s.clone(),
        Err(e) if e == NEEDS_PASSWORD => {
            password_form(ui, view, &pal);
            return;
        }
        Err(e) => {
            let e = e.clone();
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new(e).size(14.0).color(pal.muted));
            });
            return;
        }
    };
    view.wrong_password = false;
    if view.texts.is_empty() && view.text_job.is_none() {
        view.text_job = start_text_job(bytes.clone(), password.clone(), sizes.len(), ui.ctx().clone());
    }
    view.collect_text();
    view.update_matches();
    let full = ui.available_rect_before_wrap();
    let widest = sizes.iter().map(|s| s.x).fold(1.0, f32::max);
    let phone = full.width() < 560.0;
    let touch = crate::platform::touch(ui.ctx());
    let margin = if phone { 6.0 } else { 24.0 };
    let fitted = ((full.width() - 2.0 * margin - 12.0) / widest).clamp(0.05, 6.0);
    // pinch, or Ctrl and the wheel, zooms
    if ui.rect_contains_pointer(full) {
        let z = ui.input(|i| i.zoom_delta());
        if z != 1.0 {
            view.zoom_by(z, fitted);
        }
    }
    let scale = if view.zoom > 0.0 { view.zoom } else { fitted };
    let ppp = ui.ctx().pixels_per_point();
    let file_key = hash64(&[&"pdf", &bytes.len(), &(bytes.as_ptr() as usize), &password]);
    let total_h: f32 = sizes.iter().map(|s| s.y * scale + GAP).sum::<f32>() + GAP;
    let content_w = (widest * scale + 2.0 * margin).max(full.width() - 12.0);
    let mut area = egui::ScrollArea::both().id_salt(id).auto_shrink([false, false]);
    // with a mouse, dragging selects text; with a finger it scrolls
    area = area.scroll_source(egui::scroll_area::ScrollSource { drag: if touch { egui::scroll_area::DragScroll::Always } else { egui::scroll_area::DragScroll::Never }, ..egui::scroll_area::ScrollSource::ALL });
    if let Some((p, within)) = view.go_to.take() {
        let y: f32 = sizes.iter().take(p).map(|s| s.y * scale + GAP).sum::<f32>() + within * scale;
        area = area.vertical_scroll_offset(y);
    }
    if let Some((page, at, on_screen)) = view.zoom_at.take() {
        let to = offsets_for(&sizes, scale, content_w, page, at, on_screen);
        area = area.vertical_scroll_offset(to.y).horizontal_scroll_offset(to.x);
    }
    let mut double_tap: Option<(usize, egui::Pos2, Vec2)> = None;
    let mut middle_page = None;
    let mut copy_now = ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy))) && view.selection.is_some();
    area.show(ui, |ui| {
        let (outer, _) = ui.allocate_exact_size(vec2(content_w, total_h), Sense::hover());
        let mut y = outer.top() + GAP;
        for (ix, size) in sizes.iter().enumerate() {
            let shown = *size * scale;
            let rect = Rect::from_min_size(pos2(outer.center().x - shown.x / 2.0, y), shown);
            y += shown.y + GAP;
            if !ui.is_rect_visible(rect.expand(200.0)) {
                continue;
            }
            if ix == 0 {
                view.first_page = Some((rect.left(), rect.top(), scale));
            }
            // the page under the middle of the view is the current one
            let middle = ui.clip_rect().center().y;
            if rect.top() - GAP <= middle && middle <= rect.bottom() {
                middle_page = Some(ix);
            }
            ui.painter().rect_filled(rect.translate(vec2(0.0, 1.5)).expand(1.0), CornerRadius::same(2), Color32::from_black_alpha(if pal.dark { 90 } else { 40 }));
            ui.painter().rect_filled(rect, CornerRadius::ZERO, Color32::WHITE);
            // sharp at the present zoom, in steps so that small zoom changes reuse a picture
            let want = (shown.x * ppp / 160.0).ceil().max(1.0) * 160.0;
            let key = hash64(&[&file_key, &ix, &(want as u32)]);
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            let ready = match cx.assets.get(key, || job_page(bytes.clone(), password.clone(), ix, want)) {
                State::Ready(a) => Some(a.tex.id()),
                State::Failed(e) => {
                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, format!("Page {} could not be drawn: {e}", ix + 1), egui::FontId::proportional(13.0), Color32::from_gray(90));
                    continue;
                }
                State::Pending => None,
            };
            match ready {
                Some(tex) => {
                    view.shown.insert(ix, key);
                    ui.painter().image(tex, rect, uv, Color32::WHITE);
                }
                None => {
                    // the earlier, coarser picture until the new one is ready
                    if let Some(a) = view.shown.get(&ix).and_then(|k| cx.assets.peek(*k)) {
                        ui.painter().image(a.tex.id(), rect, uv, Color32::WHITE);
                    }
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
                }
            }
            let Some(text) = view.texts.get(&ix).cloned() else { continue };
            // search matches on this page
            if view.find_open {
                let first = view.matches.partition_point(|m| m.0 < ix);
                for (k, m) in view.matches.iter().enumerate().skip(first).take_while(|(_, m)| m.0 == ix) {
                    let current = view.current_match == Some(k);
                    for r in char_rects(&text, m.1, m.2, rect.min, scale) {
                        ui.painter().rect_filled(r.expand(1.0), CornerRadius::same(2), if current { Color32::from_rgba_unmultiplied(255, 140, 0, 110) } else { Color32::from_rgba_unmultiplied(255, 214, 0, 90) });
                    }
                }
            }
            // a double tap zooms in around the tapped point, and out again
            if touch {
                let tap = ui.interact(rect, id.with(("page-tap", ix)), Sense::click());
                if tap.double_clicked() {
                    if let Some(p) = tap.interact_pointer_pos() {
                        double_tap = Some((ix, pos2((p.x - rect.left()) / scale, (p.y - rect.top()) / scale), p - full.min));
                    }
                }
            }
            // selecting text with the mouse
            if !touch {
                let resp = ui.interact(rect, id.with(("page", ix)), Sense::click_and_drag());
                let to_page = |p: egui::Pos2| pos2((p.x - rect.left()) / scale, (p.y - rect.top()) / scale);
                if resp.hovered() && !text.boxes.is_empty() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
                }
                if resp.drag_started() {
                    // from where the button went down, not from where the drag was recognised
                    view.selection = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos()).and_then(|p| char_at(&text, to_page(p))).map(|c| (ix, c, c));
                } else if resp.dragged() {
                    if let (Some((page, anchor, _)), Some(p)) = (view.selection, resp.interact_pointer_pos()) {
                        if page == ix {
                            if let Some(c) = char_at(&text, to_page(p)) {
                                view.selection = Some((ix, anchor, c));
                            }
                        }
                    }
                } else if resp.clicked() {
                    view.selection = None;
                }
                if resp.double_clicked() {
                    // on a word: select it; anywhere else on the page: zoom, as a double tap does
                    let at = resp.interact_pointer_pos().map(to_page);
                    let on_text = at.and_then(|p| char_at(&text, p).filter(|c| text.boxes[*c].expand(2.0).contains(p)));
                    if let (None, Some(p), Some(screen)) = (on_text, at, resp.interact_pointer_pos()) {
                        double_tap = Some((ix, p, screen - full.min));
                    }
                    if let Some(c) = on_text {
                        let chars: Vec<char> = text.text.chars().collect();
                        let word = |ch: char| ch.is_alphanumeric() || ch == '_';
                        if chars.get(c).copied().is_some_and(word) {
                            let (mut a, mut b) = (c, c);
                            while a > 0 && word(chars[a - 1]) {
                                a -= 1;
                            }
                            while b + 1 < chars.len() && word(chars[b + 1]) {
                                b += 1;
                            }
                            view.selection = Some((ix, a, b));
                        }
                    }
                }
                resp.context_menu(|ui| {
                    if ui.add_enabled(view.selection.is_some(), egui::Button::new("Copy")).clicked() {
                        copy_now = true;
                        ui.close();
                    }
                    if ui.button("Copy the text of this page").clicked() {
                        ui.ctx().copy_text(text.text.clone());
                        ui.close();
                    }
                    if ui.button("Select all on this page").clicked() {
                        view.selection = Some((ix, 0, text.boxes.len().saturating_sub(1)));
                        ui.close();
                    }
                });
            }
            if let Some((page, a, b)) = view.selection.filter(|s| s.0 == ix) {
                let _ = page;
                for r in char_rects(&text, a.min(b), a.max(b) + 1, rect.min, scale) {
                    ui.painter().rect_filled(r, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(60, 120, 255, 80));
                }
            }
        }
    });
    if let Some((page, at, on_screen)) = double_tap {
        view.toggle_zoom(fitted, page, at, on_screen);
        ui.ctx().request_repaint();
    }
    if copy_now {
        if let Some(t) = view.selected_text() {
            ui.ctx().copy_text(t);
        }
    }
    if let Some(p) = middle_page {
        view.current = p;
    }
    if view.text_job.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(120));
    }
    if view.find_open {
        find_bar(ui, view, &pal, full, sizes.len());
    }
    // page indicator and zoom
    let n = sizes.len();
    let bar_w = if phone { 196.0 } else { 232.0 };
    let bar = Rect::from_min_size(pos2(full.center().x - bar_w / 2.0, full.bottom() - 52.0), vec2(bar_w, 38.0));
    let mut bar_ui = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
    egui::Frame::new().fill(pal.elev).stroke(egui::Stroke::new(1.0, pal.border)).corner_radius(19).inner_margin(egui::Margin::symmetric(8, 2)).show(&mut bar_ui, |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let button = |ui: &mut Ui, icon: &str, tip: &str| ui.add(egui::Button::new(egui::RichText::new(icon).size(16.0).color(pal.text)).frame(false).min_size(vec2(32.0, 32.0))).on_hover_text(tip);
        if button(ui, crate::icons::ZOOM_OUT, "Zoom out").clicked() {
            view.zoom_by(1.0 / 1.2, fitted);
        }
        if ui.add(egui::Button::new(egui::RichText::new(format!("{:.0}%", scale * 100.0)).size(12.5).color(pal.muted)).frame(false).min_size(vec2(48.0, 32.0))).on_hover_text("Fit the width").clicked() {
            view.zoom = 0.0;
        }
        if button(ui, crate::icons::ZOOM_IN, "Zoom in").clicked() {
            view.zoom_by(1.2, fitted);
        }
        ui.add_space(6.0);
        ui.label(egui::RichText::new(format!("{} / {n}", view.current + 1)).size(12.5).color(pal.text));
    });
}

/// The search field at the top right of a PDF.
fn find_bar(ui: &mut Ui, view: &mut PdfView, pal: &crate::theme::Palette, full: Rect, pages: usize) {
    let width = (full.width() - 24.0).min(380.0);
    let bar = Rect::from_min_size(pos2(full.right() - width - 18.0, full.top() + 10.0), vec2(width, 40.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
    egui::Frame::new().fill(pal.elev).stroke(egui::Stroke::new(1.0, pal.border)).corner_radius(10).inner_margin(egui::Margin::symmetric(8, 4)).show(&mut child, |ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let field = ui.add(egui::TextEdit::singleline(&mut view.query).hint_text("Find in this PDF").desired_width((width - 190.0).max(80.0)));
        if std::mem::take(&mut view.find_focus) {
            field.request_focus();
        }
        let (enter, shift, escape) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.modifiers.shift, i.key_pressed(egui::Key::Escape)));
        if field.lost_focus() && enter {
            view.update_matches();
            view.find_step(!shift);
            field.request_focus();
        }
        let reading = view.texts.len() < pages;
        let count = match (view.query.is_empty(), view.matches.len(), view.current_match) {
            (true, _, _) => String::new(),
            (false, 0, _) if reading => "Reading…".to_string(),
            (false, 0, _) => "No results".to_string(),
            (false, n, Some(i)) => format!("{} of {n}{}", i + 1, if reading { "…" } else { "" }),
            (false, n, None) => format!("{n} found{}", if reading { "…" } else { "" }),
        };
        ui.label(egui::RichText::new(count).size(12.0).color(if !view.query.is_empty() && view.matches.is_empty() && !reading { pal.danger } else { pal.muted }));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(egui::Button::new(crate::icons::X).frame(false).min_size(vec2(26.0, 26.0))).clicked() || escape {
                view.find_open = false;
            }
            if ui.add(egui::Button::new(crate::icons::ARROW_DOWN).frame(false).min_size(vec2(26.0, 26.0))).on_hover_text("Next").clicked() {
                view.find_step(true);
            }
            if ui.add(egui::Button::new(crate::icons::ARROW_UP).frame(false).min_size(vec2(26.0, 26.0))).on_hover_text("Previous").clicked() {
                view.find_step(false);
            }
        });
    });
}

/// Asks for the password of a protected file.
fn password_form(ui: &mut Ui, view: &mut PdfView, pal: &crate::theme::Palette) {
    let wrong = view.wrong_password;
    let mut open = false;
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() * 0.3).max(24.0));
        ui.label(egui::RichText::new(crate::icons::LOCK).size(30.0).color(pal.accent));
        ui.add_space(8.0);
        ui.label(egui::RichText::new("This PDF is protected by a password").size(16.0).strong());
        ui.add_space(10.0);
        let field = ui.add(egui::TextEdit::singleline(&mut view.password_draft).password(true).hint_text("Password").desired_width(240.0));
        if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            open = true;
        } else if !field.has_focus() && ui.memory(|m| m.focused().is_none()) {
            // the only thing to do here is to type the password
            field.request_focus();
        }
        if wrong {
            ui.add_space(4.0);
            ui.label(egui::RichText::new("That password does not open this file.").size(12.0).color(pal.danger));
        }
        ui.add_space(10.0);
        if ui.add(egui::Button::new(egui::RichText::new("Open").color(pal.on_accent())).fill(pal.accent).min_size(vec2(90.0, 30.0)).corner_radius(7)).clicked() {
            open = true;
        }
        ui.add_space(6.0);
        ui.label(egui::RichText::new("The password is used for this session only and is not stored.").size(11.5).color(pal.faint));
    });
    if open && !view.password_draft.is_empty() {
        let p = std::mem::take(&mut view.password_draft);
        view.unlock(&p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PDF with correct offsets, built by hand.
    pub fn sample(pages: usize) -> Vec<u8> {
        let mut objs: Vec<String> = Vec::new();
        let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
        objs.push("<< /Type /Catalog /Pages 2 0 R >>".into());
        objs.push(format!("<< /Type /Pages /Kids [{}] /Count {pages} >>", kids.join(" ")));
        objs.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
        for i in 0..pages {
            let content = format!("BT /F1 36 Tf 72 700 Td (Page {}) Tj ET\n0 0 1 rg 72 600 200 40 re f\nBT /F1 12 Tf 72 560 Td (Second line of text here) Tj ET", i + 1);
            objs.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>", 5 + 2 * i));
            objs.push(format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()));
        }
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
        for o in offsets {
            out.extend(format!("{o:010} 00000 n \n").bytes());
        }
        out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).bytes());
        out
    }

    #[test]
    fn pages_are_measured_and_drawn() {
        let bytes: Arc<[u8]> = sample(3).into();
        let sizes = page_sizes(&bytes, "").unwrap();
        assert_eq!(sizes.len(), 3);
        assert_eq!((sizes[0].x, sizes[0].y), (612.0, 792.0));
        let r = render_page(&bytes, "", 1, 306.0).unwrap();
        assert_eq!((r.w, r.h), (306, 396));
        assert_eq!(r.rgba.len(), r.w * r.h * 4);
        // a white page with something blue and something dark on it
        let px = |x: usize, y: usize| &r.rgba[(y * r.w + x) * 4..(y * r.w + x) * 4 + 4];
        assert_eq!(px(5, 5), &[255, 255, 255, 255]);
        assert_eq!(px(80, 85), &[0, 0, 255, 255]);
        assert!(r.rgba.chunks(4).any(|p| p[0] < 60 && p[1] < 60 && p[2] < 60), "the text was drawn");
        assert!(render_page(&bytes, "", 9, 300.0).is_err());
        let junk: Arc<[u8]> = b"not a pdf at all".to_vec().into();
        assert!(page_sizes(&junk, "").is_err());
    }

    #[test]
    fn text_is_read_with_its_place_on_the_page() {
        let bytes: Arc<[u8]> = sample(2).into();
        let t = page_text(&bytes, "", 1).unwrap();
        assert_eq!(t.text, "Page 2\nSecond line of text here");
        assert_eq!(t.boxes.len(), t.text.chars().count());
        // "Page 2" is 36 pt text whose baseline is 700 pt above the bottom of a 792 pt page
        let p = t.boxes[0];
        assert!((p.left() - 72.0).abs() < 1.0, "{p:?}");
        assert!(p.top() < 92.0 && p.bottom() > 92.0 && p.height() > 30.0 && p.height() < 45.0, "{p:?}");
        // letters follow each other to the right; the second line is lower and smaller
        assert!(t.boxes[1].left() > t.boxes[0].left() && t.boxes[1].left() < 110.0);
        let s = t.boxes[7];
        assert!(s.top() > 215.0 && s.bottom() < 240.0 && s.height() < 16.0, "{s:?}");
        // looking up a char by position, and the rectangles of a range
        assert_eq!(char_at(&t, pos2(74.0, 85.0)), Some(0));
        assert_eq!(t.text.chars().nth(char_at(&t, pos2(73.0, 228.0)).unwrap()), Some('S'));
        let rects = char_rects(&t, 0, t.boxes.len(), pos2(0.0, 0.0), 1.0);
        assert_eq!(rects.len(), 2, "one rectangle per line");

        // searching and selecting through the view
        let mut v = PdfView { pages: Some(Ok(vec![vec2(612.0, 792.0); 2])), ..Default::default() };
        for page in 0..2 {
            v.texts.insert(page, Arc::new(page_text(&bytes, "", page).unwrap()));
        }
        v.query = "PAGE".into();
        v.update_matches();
        assert_eq!(v.matches, vec![(0, 0, 4), (1, 0, 4)]);
        v.find_step(true);
        assert_eq!(v.current_match, Some(0));
        v.find_step(true);
        assert_eq!((v.current_match, v.go_to.map(|g| g.0)), (Some(1), Some(1)));
        v.find_step(true);
        assert_eq!(v.current_match, Some(0), "wraps around");
        v.find_step(false);
        assert_eq!(v.current_match, Some(1));
        v.query = "line of".into();
        v.update_matches();
        assert_eq!(v.matches.len(), 2);
        v.selection = Some((1, 12, 7));
        assert_eq!(v.selected_text().as_deref(), Some("Second"));
        assert_eq!(v.all_text(), "Page 1\nSecond line of text here\n\nPage 2\nSecond line of text here");

        // a double tap zooms in around the tapped point and a second one goes back
        let sizes = vec![vec2(612.0, 792.0); 2];
        let fitted = 0.5;
        v.zoom = 0.0;
        v.toggle_zoom(fitted, 1, pos2(306.0, 100.0), vec2(160.0, 300.0));
        assert!((v.zoom - 1.1).abs() < 1e-4 && v.zoom_at.is_some());
        // the tapped point of the second page lands where the finger was
        let content_w = 612.0 * v.zoom + 12.0;
        let to = offsets_for(&sizes, v.zoom, content_w, 1, pos2(306.0, 100.0), vec2(160.0, 300.0));
        assert!((to.x - (6.0 + 306.0 * 1.1 - 160.0)).abs() < 0.01, "{to:?}");
        assert!((to.y - (GAP + 792.0 * 1.1 + GAP + 100.0 * 1.1 - 300.0)).abs() < 0.01, "{to:?}");
        v.toggle_zoom(fitted, 1, pos2(306.0, 100.0), vec2(160.0, 300.0));
        assert_eq!(v.zoom, 0.0, "back to the full width");
        // near the top left corner the view cannot scroll past its start
        assert_eq!(offsets_for(&sizes, 1.0, 700.0, 0, pos2(0.0, 0.0), vec2(200.0, 200.0)), vec2(0.0, 0.0));
    }
}
