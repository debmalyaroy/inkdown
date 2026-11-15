//! Rendered views: the Markdown reader, the data tree, the CSV table and the
//! diagram / image canvas. Each keeps a small per-document state.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use egui::{pos2, vec2, Color32, CornerRadius, CursorIcon, Id, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use serde_json::Value;

use crate::assets::{self, hash64, Assets, State};
use crate::data;
use crate::icons;
use crate::md::{self, Draw, Laid, Lay, Top};
use crate::theme::{self, Palette};

#[derive(Default)]
pub struct ViewState {
    pub md: MdView,
    pub tree: TreeView,
    pub table: TableView,
    pub canvas: Canvas,
    pub pdf: crate::pdfview::PdfView,
    /// How much larger than normal the reader shows this document (0 means: not changed).
    pub zoom: f32,
    /// The zoom a pinch has reached; `zoom` follows it in small steps.
    zoom_raw: f32,
    /// When the zoom last changed, for the indicator.
    zoom_shown: f64,
    pub rich: Option<Box<crate::rich::Rich>>,
    pub design: Option<Box<crate::designer::Designer>>,
}

/// The least and most a reader can be zoomed.
pub const ZOOM_RANGE: (f32, f32) = (0.5, 4.0);

impl ViewState {
    /// The reader's zoom: 1 is the normal size.
    pub fn zoom(&self) -> f32 {
        if self.zoom > 0.0 { self.zoom } else { 1.0 }
    }

    /// Sets the zoom and returns by how much it changed. It moves in steps of 5 %, so that
    /// a pinch does not lay the document out again at every frame.
    pub fn set_zoom(&mut self, to: f32, now: f64) -> f32 {
        let before = self.zoom();
        self.zoom_raw = to.clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
        let stepped = (self.zoom_raw * 20.0).round() / 20.0;
        self.zoom = if (stepped - 1.0).abs() < 0.001 { 0.0 } else { stepped };
        if (self.zoom() - before).abs() > 0.0001 {
            self.zoom_shown = now;
            // the text keeps its place: what was at the top stays near the top
            self.md.goto_y = Some(self.md.scroll * self.zoom() / before);
        }
        self.zoom() / before
    }

    /// Pinch, Ctrl and the wheel, and a double tap, for the views that are laid out as text
    /// (Markdown, data trees, tables). Pictures and PDF pages zoom by themselves.
    pub fn zoom_gestures(&mut self, ui: &Ui, pane: egui::Rect, touch: bool) {
        let now = ui.input(|i| i.time);
        let (delta, double, pos) = ui.input(|i| (i.zoom_delta(), i.pointer.button_double_clicked(egui::PointerButton::Primary), i.pointer.interact_pos().or(i.pointer.hover_pos())));
        let inside = pos.is_some_and(|p| pane.contains(p));
        if delta != 1.0 && (inside || touch) {
            let raw = if self.zoom_raw > 0.0 { self.zoom_raw } else { self.zoom() };
            self.set_zoom(raw * delta, now);
        } else if touch && double && inside {
            // a double tap: larger, and back
            let to = if self.zoom() > 1.05 { 1.0 } else { 1.5 };
            self.set_zoom(to, now);
        }
    }

    /// Shows the zoom for a moment after it changed; a tap on it goes back to 100 %.
    pub fn zoom_indicator(&mut self, ui: &mut Ui, pane: egui::Rect, pal: &Palette) {
        let now = ui.input(|i| i.time);
        let fresh = now - self.zoom_shown < 1.6 && self.zoom_shown > 0.0;
        if !fresh && (self.zoom() - 1.0).abs() < 0.001 {
            return;
        }
        let label = format!("{:.0}%", self.zoom() * 100.0);
        let rect = egui::Rect::from_min_size(egui::pos2(pane.right() - 78.0, pane.bottom() - 44.0), egui::vec2(60.0, 28.0));
        let resp = ui.interact(rect, Id::new("read-zoom").with(pane.min.x as i32), Sense::click()).on_hover_text("Zoom. Click for 100 %.");
        ui.painter().rect(rect, egui::CornerRadius::same(14), pal.elev, egui::Stroke::new(1.0, pal.border), egui::StrokeKind::Inside);
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(12.5), pal.text);
        if resp.clicked() {
            self.set_zoom(1.0, now);
        }
        if fresh {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(400));
        }
    }
}

pub enum Action {
    Open(String),
    ToggleTask(usize),
    Zoom(u64),
    Copy(String),
    ErrorLine(Option<usize>),
    Replace(String),
    Toast(String),
}

pub struct ViewCtx<'a> {
    pub pal: &'a Palette,
    pub assets: &'a mut Assets,
    pub font_size: f32,
    pub doc_width: f32,
    pub base: Option<std::path::PathBuf>,
    pub actions: Vec<Action>,
}

// ------------------------------------------------------------- markdown

struct TopLaid {
    hash: u64,
    line0: usize,
    line1: usize,
    laid: Rc<Laid>,
    y: f32,
}

#[derive(Default)]
pub struct MdView {
    parsed_version: Option<u64>,
    parsed: Vec<Top>,
    key: (u64, u32, u64, u32, u64, u64),
    tops: Vec<TopLaid>,
    cache: HashMap<u64, Rc<Laid>>,
    pub total: f32,
    pub outline: Vec<(u8, String, String, usize)>,
    open: HashSet<u64>,
    open_epoch: u64,
    /// Scroll here on the next frame.
    pub goto_y: Option<f32>,
    pub goto_anchor: Option<String>,
    pub goto_line: Option<usize>,
    pub scroll: f32,
    stale: bool,
    pub words: usize,
}

impl MdView {
    pub fn invalidate(&mut self) {
        self.parsed_version = None;
        self.cache.clear();
    }

    /// Document y of a source line (1-based), for scroll sync.
    pub fn y_of_line(&self, line: usize) -> Option<f32> {
        let i = self.tops.partition_point(|t| t.line1 < line);
        let t = self.tops.get(i).or(self.tops.last())?;
        let span = (t.line1 - t.line0).max(1) as f32;
        let frac = (line.saturating_sub(t.line0) as f32 / span).clamp(0.0, 1.0);
        Some(t.y + t.laid.height * if line < t.line0 { 0.0 } else { frac })
    }

    pub fn line_at_y(&self, y: f32) -> usize {
        let i = self.tops.partition_point(|t| t.y + t.laid.height < y);
        match self.tops.get(i) {
            Some(t) => {
                let frac = ((y - t.y) / t.laid.height.max(1.0)).clamp(0.0, 1.0);
                t.line0 + ((t.line1 - t.line0) as f32 * frac) as usize
            }
            None => self.tops.last().map(|t| t.line1).unwrap_or(1),
        }
    }

    fn rebuild(&mut self, ui: &Ui, text: impl FnOnce() -> String, version: u64, width: f32, cx: &mut ViewCtx<'_>, pal_epoch: u64) {
        if self.parsed_version != Some(version) {
            let t = text();
            self.words = t.split_whitespace().count();
            self.parsed = md::parse(&t);
            self.outline = md::outline(&self.parsed);
            self.parsed_version = Some(version);
        }
        let key = (version, width as u32, pal_epoch, (cx.font_size * 8.0) as u32, cx.assets.epoch, self.open_epoch);
        if key == self.key && !self.stale {
            return;
        }
        let assets_changed = key.4 != self.key.4 || self.stale;
        self.key = key;
        self.stale = false;
        let ppp = ui.ctx().pixels_per_point();
        let mut lay = Lay { ui, pal: cx.pal, size: cx.font_size, assets: cx.assets, base: cx.base.clone(), ppp, open: &self.open, pending: false, anchors: Vec::new(), column: width };
        let mut next: HashMap<u64, Rc<Laid>> = HashMap::with_capacity(self.parsed.len());
        let mut tops = Vec::with_capacity(self.parsed.len());
        let mut y = 0.0f32;
        let gap_first = 0.0;
        for (i, top) in self.parsed.iter().enumerate() {
            let hash = hash64(&[&top.block, &(width as u32), &pal_epoch, &((cx.font_size * 8.0) as u32), &self.open_epoch]);
            let cached = self.cache.get(&hash).filter(|l| !(l.pending && assets_changed)).cloned();
            let laid = match cached.or_else(|| next.get(&hash).cloned()) {
                Some(l) => l,
                None => Rc::new(md::layout_top(&mut lay, &top.block, width)),
            };
            y += if i == 0 { gap_first } else { lay.gap(&top.block) };
            next.insert(hash, laid.clone());
            let h = laid.height;
            tops.push(TopLaid { hash, line0: top.line0, line1: top.line1, laid, y });
            y += h;
        }
        self.cache = next;
        self.tops = tops;
        self.total = y;
    }

    fn anchor_y(&self, id: &str) -> Option<f32> {
        let want = id.trim_start_matches('#');
        let want_slug = md::slug(want);
        for t in &self.tops {
            for (a, y) in &t.laid.anchors {
                if a == want || *a == want_slug {
                    return Some(t.y + y);
                }
            }
        }
        None
    }
}

fn paint_check(ui: &Ui, rect: Rect, checked: bool, pal: &Palette, hot: bool) {
    let p = ui.painter();
    if checked {
        p.rect_filled(rect, CornerRadius::same(5), pal.accent);
        let g = theme::text(ui, icons::CHECK, rect.height() * 0.78, pal.on_accent(), 700.0);
        p.galley(rect.center() - g.size() / 2.0, g, pal.on_accent());
    } else {
        p.rect_filled(rect, CornerRadius::same(5), pal.doc_bg);
        p.rect_stroke(rect, CornerRadius::same(5), Stroke::new(1.5, if hot { pal.accent } else { pal.doc_border }), egui::StrokeKind::Inside);
    }
}

/// The Markdown reader. `text` is only called when the document changed.
#[allow(clippy::too_many_arguments)]
pub fn markdown(ui: &mut Ui, view: &mut MdView, id: Id, text: impl FnOnce() -> String, version: u64, cx: &mut ViewCtx<'_>, pal_epoch: u64) {
    let pal = *cx.pal;
    let full = ui.available_rect_before_wrap();
    let narrow = full.width() < 560.0;
    let margin = if narrow { 18.0 } else { 44.0 };
    let column = (full.width() - margin * 2.0).min(cx.doc_width).max(160.0);
    let paged = full.width() > column + margin * 2.0 + 140.0;
    ui.painter().rect_filled(full, CornerRadius::ZERO, if paged { pal.desk } else { pal.doc_bg });

    view.rebuild(ui, text, version, column, cx, pal_epoch);
    if let Some(a) = view.goto_anchor.take() {
        view.goto_y = view.anchor_y(&a).map(|y| y - 12.0);
    }
    if let Some(l) = view.goto_line.take() {
        view.goto_y = view.y_of_line(l).map(|y| y - 24.0);
    }

    let top_pad = if paged { 28.0 } else { 26.0 };
    let page_pad = if paged { 56.0 } else { 0.0 };
    let content_h = view.total + top_pad * 2.0 + page_pad * 2.0 + 80.0;
    let mut area = egui::ScrollArea::vertical().id_salt(id).auto_shrink([false, false]);
    if let Some(y) = view.goto_y.take() {
        area = area.vertical_scroll_offset((y + top_pad + page_pad).max(0.0));
    }
    let mut stale = false;
    let mut toggles: Vec<u64> = Vec::new();
    let out = area.show_viewport(ui, |ui, viewport| {
        ui.set_width(full.width());
        ui.set_height(content_h);
        let origin_y = ui.min_rect().min.y;
        let page_w = column + if paged { 128.0 } else { 0.0 };
        let page_x = full.min.x + (full.width() - page_w) / 2.0;
        let col_x = (full.min.x + (full.width() - column) / 2.0).round();
        if paged {
            let page = Rect::from_min_size(pos2(page_x, origin_y + top_pad), vec2(page_w, view.total + page_pad * 2.0));
            let shadow = egui::Shadow { offset: [0, 6], blur: 28, spread: 0, color: Color32::from_black_alpha(if pal.dark { 110 } else { 28 }) };
            ui.painter().add(shadow.as_shape(page, CornerRadius::same(10)));
            ui.painter().rect_filled(page, CornerRadius::same(10), pal.doc_bg);
            ui.painter().rect_stroke(page, CornerRadius::same(10), Stroke::new(1.0, pal.border_soft), egui::StrokeKind::Outside);
        }
        let doc_y = origin_y + top_pad + page_pad;
        let (vis0, vis1) = (viewport.min.y - top_pad - page_pad - 200.0, viewport.max.y - top_pad - page_pad + 200.0);
        let first = view.tops.partition_point(|t| t.y + t.laid.height < vis0);
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let touch = crate::platform::touch(ui.ctx());
        for (ti, top) in view.tops.iter().enumerate().skip(first) {
            if top.y > vis1 {
                break;
            }
            let off = vec2(col_x, doc_y + top.y);
            for (di, d) in top.laid.items.iter().enumerate() {
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
                    Draw::Text { pos, galley, links } => {
                        // galleys may be centred or right aligned around `pos`
                        let gp = *pos + off;
                        let rect = galley.rect.translate(gp.to_vec2());
                        let resp = ui.interact(rect, id.with((top.hash, di, ti)), if touch { Sense::click() } else { Sense::click_and_drag() });
                        if touch {
                            // a finger drag scrolls; selecting text by dragging would fight it
                            ui.painter().galley(gp, galley.clone(), pal.doc_text);
                        } else {
                            egui::text_selection::LabelSelectionState::label_text_selection(ui, &resp, gp, galley.clone(), pal.doc_text, Stroke::NONE);
                        }
                        if !links.is_empty() {
                            if let Some(p) = pointer.filter(|p| rect.contains(*p)) {
                                let local = p - gp;
                                let hit = links.iter().find(|(a, b, _)| md::char_rects(galley, *a, *b).iter().any(|r| r.expand(1.0).contains(local.to_pos2())));
                                if let Some((_, _, url)) = hit {
                                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                                    if resp.clicked() {
                                        cx.actions.push(Action::Open(url.to_string()));
                                    }
                                    let url = url.to_string();
                                    resp.on_hover_text_at_pointer(url);
                                }
                            }
                        }
                    }
                    Draw::Image { rect, key, radius, link, zoom, card: _ } => {
                        let r = rect.translate(off);
                        match cx.assets.peek(*key) {
                            Some(a) => {
                                egui::Image::from_texture((a.tex.id(), r.size())).corner_radius(CornerRadius::same(*radius)).paint_at(ui, r);
                                if *zoom || link.is_some() {
                                    let resp = ui.interact(r, id.with(("img", top.hash, di)), Sense::click());
                                    if resp.hovered() {
                                        ui.ctx().set_cursor_icon(if link.is_some() { CursorIcon::PointingHand } else { CursorIcon::ZoomIn });
                                    }
                                    if resp.clicked() {
                                        match link {
                                            Some(l) => cx.actions.push(Action::Open(l.to_string())),
                                            None => cx.actions.push(Action::Zoom(*key)),
                                        }
                                    }
                                }
                            }
                            None => stale = true,
                        }
                    }
                    Draw::Check { rect, checked, rel_line } => {
                        let r = rect.translate(off);
                        let resp = ui.interact(r.expand(3.0), id.with(("chk", top.hash, di, ti)), Sense::click());
                        paint_check(ui, r, *checked, &pal, resp.hovered());
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                        }
                        if resp.clicked() {
                            cx.actions.push(Action::ToggleTask(top.line0 + rel_line));
                        }
                    }
                    Draw::Copy { rect, text } => {
                        let r = rect.translate(off);
                        if pointer.is_some_and(|p| r.contains(p)) {
                            let b = Rect::from_min_size(pos2(r.max.x - 34.0, r.min.y + 6.0), vec2(28.0, 26.0));
                            let resp = ui.interact(b, id.with(("copy", top.hash, di)), Sense::click());
                            ui.painter().rect_filled(b, CornerRadius::same(6), if resp.hovered() { pal.active() } else { pal.doc_code });
                            ui.painter().rect_stroke(b, CornerRadius::same(6), Stroke::new(1.0, pal.doc_border), egui::StrokeKind::Inside);
                            let g = theme::text(ui, icons::COPY, 13.0, pal.muted, 400.0);
                            ui.painter().galley(b.center() - g.size() / 2.0, g, pal.muted);
                            if resp.clicked() {
                                cx.actions.push(Action::Copy(text.to_string()));
                            }
                            resp.on_hover_text("Copy code");
                        }
                    }
                    Draw::Toggle { rect, key } => {
                        let r = rect.translate(off);
                        let resp = ui.interact(r, id.with(("tog", top.hash, di)), Sense::click());
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                        }
                        if resp.clicked() {
                            toggles.push(*key);
                        }
                    }
                }
            }
        }
    });
    view.scroll = (out.state.offset().y - top_pad - page_pad).max(0.0);
    for k in toggles {
        if !view.open.remove(&k) {
            view.open.insert(k);
        }
        view.open_epoch += 1;
    }
    if stale {
        view.stale = true;
        // forget layouts that reference dropped textures
        view.cache.clear();
        ui.ctx().request_repaint();
    }
}

// ------------------------------------------------------------ data tree

struct TreeRow {
    depth: u16,
    key: String,
    text: String,
    class: u8, // 0 container, 1 string, 2 number, 3 bool, 4 null
    path: u64,
    open: Option<bool>,
}

#[derive(Default)]
pub struct TreeView {
    version: Option<u64>,
    value: Option<Value>,
    pub error: Option<(String, Option<usize>)>,
    rows: Vec<TreeRow>,
    toggled: HashSet<u64>,
    dirty: bool,
    pub filter: String,
    filter_seen: String,
    pub count: usize,
}

fn flatten(v: &Value, key: &str, depth: u16, path: u64, toggled: &HashSet<u64>, filter: &str, out: &mut Vec<TreeRow>) {
    if out.len() > 300_000 {
        return;
    }
    let (class, text) = match v {
        Value::Null => (4, "null".to_string()),
        Value::Bool(b) => (3, b.to_string()),
        Value::Number(n) => (2, n.to_string()),
        Value::String(s) => (1, if s.chars().count() > 400 { format!("\"{}…\"", s.chars().take(400).collect::<String>()) } else { format!("{s:?}") }),
        Value::Array(a) => (0, format!("[{}]", a.len())),
        Value::Object(o) => (0, format!("{{{}}}", o.len())),
    };
    if class != 0 {
        if filter.is_empty() || key.to_lowercase().contains(filter) || text.to_lowercase().contains(filter) {
            out.push(TreeRow { depth, key: key.to_string(), text, class, path, open: None });
        }
        return;
    }
    let open = !filter.is_empty() || ((depth < 2) ^ toggled.contains(&path));
    let at = out.len();
    out.push(TreeRow { depth, key: key.to_string(), text, class, path, open: Some(open) });
    if open {
        match v {
            Value::Array(a) => {
                for (i, c) in a.iter().enumerate() {
                    flatten(c, &i.to_string(), depth + 1, hash64(&[&path, &i]), toggled, filter, out);
                }
            }
            Value::Object(o) => {
                for (k, c) in o {
                    flatten(c, k, depth + 1, hash64(&[&path, k]), toggled, filter, out);
                }
            }
            _ => {}
        }
    }
    if !filter.is_empty() && out.len() == at + 1 && !key.to_lowercase().contains(filter) && depth > 0 {
        out.pop(); // nothing below matched
    }
}

pub fn data_tree(ui: &mut Ui, view: &mut TreeView, id: Id, format: &'static str, text: impl FnOnce() -> String, version: u64, cx: &mut ViewCtx<'_>) {
    let pal = *cx.pal;
    if view.version != Some(version) {
        view.version = Some(version);
        let t = text();
        match data::parse(format, &t) {
            Ok(v) => {
                view.value = Some(v);
                view.error = None;
                cx.actions.push(Action::ErrorLine(None));
            }
            Err(e) => {
                cx.actions.push(Action::ErrorLine(e.line.map(|l| l.saturating_sub(1))));
                view.error = Some((e.message, e.line));
            }
        }
        view.dirty = true;
    }
    if view.filter != view.filter_seen {
        view.filter_seen = view.filter.clone();
        view.dirty = true;
    }
    if view.dirty {
        view.dirty = false;
        view.rows.clear();
        if let Some(v) = &view.value {
            flatten(v, "", 0, 1, &view.toggled, &view.filter.to_lowercase(), &mut view.rows);
        }
        view.count = view.rows.len();
    }
    let full = ui.available_rect_before_wrap();
    ui.painter().rect_filled(full, CornerRadius::ZERO, pal.doc_bg);
    egui::Frame::new().inner_margin(egui::Margin::symmetric(14, 8)).show(ui, |ui| {
        // wraps onto a second row when the pane is narrow, instead of overlapping
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            let filter_w = (ui.available_width() * 0.45).clamp(110.0, 220.0);
            ui.add(egui::TextEdit::singleline(&mut view.filter).hint_text(format!("{}  Filter keys and values", icons::FILTER)).desired_width(filter_w));
            match &view.error {
                Some((msg, line)) => {
                    let t = match line {
                        Some(l) => format!("{}  Line {l}: {msg}", icons::CIRCLE_ALERT),
                        None => format!("{}  {msg}", icons::CIRCLE_ALERT),
                    };
                    ui.label(egui::RichText::new(t).color(pal.danger).size(12.5));
                }
                None => {
                    ui.label(egui::RichText::new(format!("{}  Valid {}", icons::CIRCLE_CHECK, format.to_uppercase())).color(pal.tip).size(12.5));
                }
            }
            {
                for target in ["json", "yaml", "toml"] {
                    if target != format && view.value.is_some() && ui.small_button(format!("Copy as {}", target.to_uppercase())).clicked() {
                        match data::emit(target, view.value.as_ref().unwrap(), false) {
                            Ok(t) => {
                                cx.actions.push(Action::Copy(t));
                                cx.actions.push(Action::Toast(format!("Copied as {}", target.to_uppercase())));
                            }
                            Err(e) => cx.actions.push(Action::Toast(e)),
                        }
                    }
                }
                if view.value.is_some() && ui.small_button("Minify").clicked() {
                    if let Ok(t) = data::emit(format, view.value.as_ref().unwrap(), true) {
                        cx.actions.push(Action::Replace(t));
                    }
                }
                if view.value.is_some() && ui.small_button("Format").clicked() {
                    if let Ok(t) = data::emit(format, view.value.as_ref().unwrap(), false) {
                        cx.actions.push(Action::Replace(t));
                    }
                }
            }
        });
    });
    let row_h = (cx.font_size * 1.55).round();
    let size = cx.font_size * 0.9;
    let mut toggle = None;
    let stale = view.error.is_some();
    egui::ScrollArea::both().id_salt(id).auto_shrink([false, false]).show_rows(ui, row_h, view.rows.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for r in &view.rows[range] {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().max(400.0), row_h), if r.open.is_some() { Sense::click() } else { Sense::hover() });
            if resp.hovered() {
                ui.painter().rect_filled(rect, CornerRadius::ZERO, pal.line_hl());
            }
            let mut x = rect.min.x + 16.0 + r.depth as f32 * 18.0;
            let cy = rect.center().y;
            if let Some(open) = r.open {
                let g = theme::text(ui, if open { icons::CHEVRON_DOWN } else { icons::CHEVRON_RIGHT }, size, pal.faint, 400.0);
                ui.painter().galley(pos2(x - 16.0, cy - g.size().y / 2.0), g, pal.faint);
                if resp.clicked() {
                    toggle = Some(r.path);
                }
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
            }
            let mut job = egui::text::LayoutJob::default();
            if !r.key.is_empty() {
                job.append(&r.key, 0.0, theme::mono(size, pal.syn.attr));
                job.append(": ", 0.0, theme::mono(size, pal.faint));
            }
            let c = match r.class {
                1 => pal.syn.string,
                2 => pal.syn.number,
                3 => pal.syn.keyword,
                4 => pal.faint,
                _ => pal.muted,
            };
            job.append(&r.text, 0.0, theme::mono(size, if stale { theme::mix(c, pal.doc_bg, 0.4) } else { c }));
            let g = ui.fonts_mut(|f| f.layout_job(job));
            x += 2.0;
            ui.painter().galley(pos2(x, cy - g.size().y / 2.0), g, pal.doc_text);
        }
    });
    if let Some(p) = toggle {
        if !view.toggled.remove(&p) {
            view.toggled.insert(p);
        }
        view.dirty = true;
    }
}

// ------------------------------------------------------------ CSV table

#[derive(Default)]
pub struct TableView {
    version: Option<u64>,
    rows: Vec<Vec<String>>,
    widths: Vec<f32>,
    order: Vec<u32>,
    sort: Option<(usize, bool)>,
    pub truncated: bool,
}

pub fn parse_csv(text: &str, delim: char, limit: usize) -> (Vec<Vec<String>>, bool) {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    cell.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                cell.push(c);
            }
        } else if c == '"' && cell.is_empty() {
            quoted = true;
        } else if c == delim {
            row.push(std::mem::take(&mut cell));
        } else if c == '\n' {
            row.push(std::mem::take(&mut cell));
            rows.push(std::mem::take(&mut row));
            if rows.len() >= limit {
                return (rows, true);
            }
        } else if c != '\r' {
            cell.push(c);
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    (rows, false)
}

pub fn table(ui: &mut Ui, view: &mut TableView, id: Id, delim: char, text: impl FnOnce() -> String, version: u64, cx: &mut ViewCtx<'_>) {
    let pal = *cx.pal;
    let size = cx.font_size * 0.92;
    if view.version != Some(version) {
        view.version = Some(version);
        let (rows, truncated) = parse_csv(&text(), delim, 500_000);
        view.rows = rows;
        view.truncated = truncated;
        let cols = view.rows.iter().take(500).map(Vec::len).max().unwrap_or(0);
        view.widths = (0..cols)
            .map(|c| {
                let longest = view.rows.iter().take(300).filter_map(|r| r.get(c)).map(|s| s.chars().count().min(60)).max().unwrap_or(4);
                (longest as f32 * size * 0.56 + 28.0).clamp(64.0, 460.0)
            })
            .collect();
        view.sort = None;
        view.order = (1..view.rows.len() as u32).collect();
    }
    let full = ui.available_rect_before_wrap();
    ui.painter().rect_filled(full, CornerRadius::ZERO, pal.doc_bg);
    if view.rows.is_empty() {
        ui.centered_and_justified(|ui| ui.label(egui::RichText::new("No rows").color(pal.muted)));
        return;
    }
    let row_h = (size * 2.0).round();
    let num_w = (view.rows.len().to_string().len() as f32 * size * 0.6 + 22.0).max(44.0);
    let total_w: f32 = num_w + view.widths.iter().sum::<f32>();
    let header = view.rows[0].clone();
    let mut clicked_col = None;
    egui::ScrollArea::both().id_salt(id).auto_shrink([false, false]).show_viewport(ui, |ui, viewport| {
        ui.set_width(total_w.max(full.width()));
        ui.set_height(row_h * (view.order.len() + 1) as f32 + 24.0);
        let origin = ui.min_rect().min;
        let first = ((viewport.min.y - row_h) / row_h).floor().max(0.0) as usize;
        let last = (((viewport.max.y) / row_h).ceil() as usize).min(view.order.len());
        let rule = Stroke::new(1.0, theme::mix(pal.doc_bg, pal.doc_border, 0.6));
        let p = ui.painter().clone();
        for vi in first..last {
            let ri = view.order[vi] as usize;
            let y = origin.y + row_h * (vi + 1) as f32;
            if vi % 2 == 1 {
                p.rect_filled(Rect::from_min_size(pos2(origin.x, y), vec2(total_w, row_h)), CornerRadius::ZERO, theme::mix(pal.doc_bg, pal.doc_subtle, 0.6));
            }
            let g = theme::text(ui, &ri.to_string(), size * 0.88, pal.faint, 400.0);
            p.galley(pos2(origin.x + num_w - g.size().x - 10.0, y + (row_h - g.size().y) / 2.0), g, pal.faint);
            let mut x = origin.x + num_w;
            for (ci, w) in view.widths.iter().enumerate() {
                if x + w > origin.x + viewport.min.x - 10.0 && x < origin.x + viewport.max.x + 10.0 {
                    if let Some(cell) = view.rows[ri].get(ci) {
                        let numeric = cell.trim().parse::<f64>().is_ok();
                        let mut job = egui::text::LayoutJob::single_section(cell.chars().take(200).collect(), theme::fmt(size, if numeric { pal.syn.number } else { pal.doc_text }, 400.0));
                        job.wrap = egui::text::TextWrapping { max_width: w - 18.0, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
                        let g = ui.fonts_mut(|f| f.layout_job(job));
                        let gx = if numeric { x + w - g.size().x - 9.0 } else { x + 9.0 };
                        p.galley(pos2(gx, y + (row_h - g.size().y) / 2.0), g, pal.doc_text);
                    }
                }
                x += w;
            }
            p.line_segment([pos2(origin.x, y + row_h), pos2(origin.x + total_w, y + row_h)], rule);
        }
        // sticky header
        let hy = origin.y + viewport.min.y;
        let hrect = Rect::from_min_size(pos2(origin.x, hy), vec2(total_w.max(full.width()), row_h));
        p.rect_filled(hrect, CornerRadius::ZERO, pal.doc_subtle);
        p.line_segment([hrect.left_bottom(), hrect.right_bottom()], Stroke::new(1.0, pal.doc_border));
        let mut x = origin.x + num_w;
        for (ci, w) in view.widths.iter().enumerate() {
            let r = Rect::from_min_size(pos2(x, hy), vec2(*w, row_h));
            let resp = ui.interact(r, id.with(("h", ci)), Sense::click());
            if resp.hovered() {
                p.rect_filled(r, CornerRadius::ZERO, pal.hover());
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            if resp.clicked() {
                clicked_col = Some(ci);
            }
            let name = header.get(ci).cloned().unwrap_or_default();
            let arrow = match view.sort {
                Some((c, asc)) if c == ci => if asc { format!("  {}", icons::ARROW_UP) } else { format!("  {}", icons::ARROW_DOWN) },
                _ => String::new(),
            };
            let mut job = egui::text::LayoutJob::single_section(format!("{name}{arrow}"), theme::fmt(size, pal.doc_text, 650.0));
            job.wrap = egui::text::TextWrapping { max_width: w - 16.0, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
            let g = ui.fonts_mut(|f| f.layout_job(job));
            p.galley(pos2(x + 9.0, hy + (row_h - g.size().y) / 2.0), g, pal.doc_text);
            p.line_segment([pos2(x, hy), pos2(x, origin.y + row_h * (view.order.len() + 1) as f32)], rule);
            x += w;
        }
    });
    if let Some(c) = clicked_col {
        let asc = !matches!(view.sort, Some((sc, true)) if sc == c);
        view.sort = Some((c, asc));
        let rows = &view.rows;
        view.order.sort_by(|a, b| {
            let (x, y) = (rows[*a as usize].get(c).map(String::as_str).unwrap_or(""), rows[*b as usize].get(c).map(String::as_str).unwrap_or(""));
            let ord = match (x.trim().parse::<f64>(), y.trim().parse::<f64>()) {
                (Ok(p), Ok(q)) => p.partial_cmp(&q).unwrap_or(std::cmp::Ordering::Equal),
                _ => x.to_lowercase().cmp(&y.to_lowercase()),
            };
            if asc { ord } else { ord.reverse() }
        });
    }
}

// ------------------------------------------------------- diagram canvas

#[derive(Default)]
pub struct Canvas {
    pub zoom: f32,
    pub pan: Vec2,
    pub key: u64,
    fitted: Option<u64>,
}

pub enum CanvasSource<'a> {
    Mermaid(&'a str),
    Dot(&'a str),
    Svg(&'a str),
    Chart(&'a str),
    Image(Arc<[u8]>),
}

/// Pan-and-zoom view of one diagram or image. Returns the asset key once it is ready.
pub fn canvas(ui: &mut Ui, view: &mut Canvas, id: Id, src: CanvasSource<'_>, cx: &mut ViewCtx<'_>) -> Option<u64> {
    let pal = *cx.pal;
    let full = ui.available_rect_before_wrap();
    let ppp = ui.ctx().pixels_per_point();
    let dark = pal.dark;
    let light_stage = dark && matches!(src, CanvasSource::Dot(_) | CanvasSource::Svg(_));
    ui.painter().rect_filled(full, CornerRadius::ZERO, if light_stage { theme::rgb(0xf1f2f5) } else { pal.doc_bg });
    // rasterise at up to 2x so zooming in stays sharp
    let scale = ppp * 2.0;
    let (key, state) = match &src {
        CanvasSource::Mermaid(s) => {
            let k = hash64(&[&"cv-mmd", s, &dark, &((scale * 10.0) as u32)]);
            let s = s.to_string();
            (k, cx.assets.get(k, || assets::job_mermaid(s, dark, scale, 100_000.0)))
        }
        CanvasSource::Dot(s) => {
            let k = hash64(&[&"cv-dot", s, &((scale * 10.0) as u32)]);
            let s = s.to_string();
            (k, cx.assets.get(k, || assets::job_dot(s, scale, 100_000.0)))
        }
        CanvasSource::Svg(s) => {
            let k = hash64(&[&"cv-svg", s, &((scale * 10.0) as u32)]);
            let (s, base) = (s.to_string(), cx.base.clone());
            (k, cx.assets.get(k, || assets::job_svg(s, scale, 100_000.0, base)))
        }
        CanvasSource::Chart(s) => {
            let k = hash64(&[&"cv-chart", s, &dark]);
            let s = s.to_string();
            (k, cx.assets.get(k, || assets::job_chart(s, dark, scale, 760.0)))
        }
        CanvasSource::Image(b) => {
            let k = hash64(&[&"cv-img", &b.len(), &&b[..b.len().min(4096)]]);
            let b = b.clone();
            (k, cx.assets.get(k, || assets::job_bytes(b)))
        }
    };
    let resp = ui.interact(full, id, Sense::click_and_drag());
    ui.advance_cursor_after_rect(full);
    match state {
        State::Pending => {
            // keep showing the previous render while the new one is prepared
            if let Some(a) = cx.assets.peek(view.key) {
                let size = a.size * view.zoom.max(0.01);
                let r = Rect::from_center_size(full.center() + view.pan, size);
                egui::Image::from_texture((a.tex.id(), size)).tint(Color32::from_white_alpha(150)).paint_at(&ui.new_child(egui::UiBuilder::new().max_rect(full)), r);
            } else {
                let g = theme::text(ui, "Rendering…", 13.0, pal.faint, 400.0);
                ui.painter().galley(full.center() - g.size() / 2.0, g, pal.faint);
            }
            None
        }
        State::Failed(e) => {
            cx.actions.push(Action::ErrorLine(error_line(&e)));
            let mut job = egui::text::LayoutJob::default();
            job.wrap.max_width = (full.width() - 80.0).max(120.0);
            job.halign = egui::Align::Center;
            job.append(&format!("{}  Could not draw this\n", icons::TRIANGLE_ALERT), 0.0, theme::fmt(15.0, pal.caution, 650.0));
            job.append(e.lines().take(6).collect::<Vec<_>>().join("\n").as_str(), 0.0, theme::fmt(12.5, pal.muted, 400.0));
            let g = ui.fonts_mut(|f| f.layout_job(job));
            ui.painter().galley(pos2(full.center().x, full.center().y - g.size().y / 2.0), g, pal.muted);
            None
        }
        State::Ready(a) => {
            let (tex, size) = (a.tex.id(), a.size);
            if view.fitted != Some(key) {
                // first sight of this render: keep zoom if only the content changed
                if view.zoom <= 0.0 || view.key == 0 {
                    let fit = ((full.width() - 48.0) / size.x).min((full.height() - 48.0) / size.y);
                    view.zoom = fit.clamp(0.05, 1.0);
                    view.pan = Vec2::ZERO;
                }
                view.fitted = Some(key);
                view.key = key;
                cx.actions.push(Action::ErrorLine(None));
            }
            if resp.dragged() {
                view.pan += resp.drag_delta();
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            } else if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::Grab);
            }
            if resp.hovered() {
                let (scroll, zoom, ptr) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.pointer.hover_pos()));
                let factor = if zoom != 1.0 { zoom } else if ui.input(|i| i.modifiers.command) { 1.0 } else { (scroll.y * 0.0022).exp() };
                if factor != 1.0 {
                    let old = view.zoom;
                    view.zoom = (view.zoom * factor).clamp(0.05, 8.0);
                    if let Some(p) = ptr {
                        // zoom about the pointer
                        let rel = p - full.center() - view.pan;
                        view.pan -= rel * (view.zoom / old - 1.0);
                    }
                }
            }
            if resp.double_clicked() {
                let fit = ((full.width() - 48.0) / size.x).min((full.height() - 48.0) / size.y);
                view.zoom = if (view.zoom - 1.0).abs() < 0.01 { fit.clamp(0.05, 1.0) } else { 1.0 };
                view.pan = Vec2::ZERO;
            }
            let shown = size * view.zoom;
            let r = Rect::from_center_size(full.center() + view.pan, shown);
            let child = ui.new_child(egui::UiBuilder::new().max_rect(full));
            let mut child = child;
            child.set_clip_rect(full);
            if matches!(src, CanvasSource::Image(_)) {
                checker(&child, r.intersect(full), &pal);
            }
            egui::Image::from_texture((tex, shown)).paint_at(&child, r);
            // controls
            let bar = Rect::from_min_size(pos2(full.max.x - 150.0, full.max.y - 46.0), vec2(136.0, 32.0));
            let mut bui = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
            egui::Frame::new().fill(pal.elev).stroke(Stroke::new(1.0, pal.border)).corner_radius(8).inner_margin(egui::Margin::symmetric(4, 3)).show(&mut bui, |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if ui.button(icons::ZOOM_OUT).clicked() {
                    view.zoom = (view.zoom / 1.25).max(0.05);
                }
                ui.label(egui::RichText::new(format!("{:>3.0}%", view.zoom * 100.0)).size(12.0).color(pal.muted));
                if ui.button(icons::ZOOM_IN).clicked() {
                    view.zoom = (view.zoom * 1.25).min(8.0);
                }
                if ui.button(icons::MAXIMIZE).on_hover_text("Fit").clicked() {
                    let fit = ((full.width() - 48.0) / size.x).min((full.height() - 48.0) / size.y);
                    view.zoom = fit.clamp(0.05, 1.0);
                    view.pan = Vec2::ZERO;
                }
            });
            Some(key)
        }
    }
}

fn checker(ui: &Ui, r: Rect, pal: &Palette) {
    if r.width() <= 0.0 || r.height() <= 0.0 {
        return;
    }
    let s = 12.0;
    let (a, b) = if pal.dark { (theme::rgb(0x2a2d34), theme::rgb(0x23262c)) } else { (theme::rgb(0xffffff), theme::rgb(0xeceef1)) };
    ui.painter().rect_filled(r, CornerRadius::ZERO, a);
    let (nx, ny) = ((r.width() / s).ceil() as usize, (r.height() / s).ceil() as usize);
    if nx * ny > 40_000 {
        return;
    }
    for j in 0..ny {
        for i in 0..nx {
            if (i + j) % 2 == 1 {
                let c = Rect::from_min_size(pos2(r.min.x + i as f32 * s, r.min.y + j as f32 * s), vec2(s, s)).intersect(r);
                ui.painter().rect_filled(c, CornerRadius::ZERO, b);
            }
        }
    }
}

fn error_line(msg: &str) -> Option<usize> {
    let lower = msg.to_lowercase();
    let i = lower.find("line ")?;
    let n: String = lower[i + 5..].chars().take_while(|c| c.is_ascii_digit()).collect();
    n.parse::<usize>().ok().map(|l| l.saturating_sub(1))
}

/// Point inside a rect, for callers placing overlays.
pub fn anchor(rect: Rect) -> Pos2 {
    rect.min
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_and_tree() {
        let (rows, _) = parse_csv("a,b\n\"x, y\",\"he said \"\"hi\"\"\"\n1,2", ',', 100);
        assert_eq!(rows, vec![vec!["a", "b"], vec!["x, y", "he said \"hi\""], vec!["1", "2"]]);
        let v: Value = serde_json::from_str(r#"{"a":{"b":[1,2,{"c":null}]},"d":"x"}"#).unwrap();
        let mut rows = Vec::new();
        flatten(&v, "", 0, 1, &HashSet::new(), "", &mut rows);
        // root and `a` are open by default (depth < 2), `b` is closed
        assert_eq!(rows.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), vec!["", "a", "b", "d"]);
        let mut rows = Vec::new();
        flatten(&v, "", 0, 1, &HashSet::new(), "c", &mut rows);
        assert!(rows.iter().any(|r| r.key == "c"));
        assert_eq!(error_line("Parse error on line 3: bad"), Some(2));
    }
}
