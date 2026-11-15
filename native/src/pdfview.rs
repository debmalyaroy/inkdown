//! Reading PDF files: pages are drawn by a pure-Rust renderer on the background workers
//! and shown one under another, fitted to the width of the pane.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use egui::{pos2, vec2, Color32, CornerRadius, Id, Rect, Sense, Ui, Vec2};
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings};

use crate::assets::{hash64, Job, Raster, State};
use crate::preview::ViewCtx;

/// Widest or tallest a rendered page may be, in pixels (a GPU texture limit on phones).
const MAX_PX: f32 = 4096.0;
const GAP: f32 = 14.0;

#[derive(Default)]
pub struct PdfView {
    /// Page sizes in points, or why the file cannot be read.
    pages: Option<Result<Vec<Vec2>, String>>,
    /// 0 = fit the width of the pane; otherwise points on screen per PDF point.
    pub zoom: f32,
    /// The last picture shown for each page, kept on screen while a sharper one is made.
    shown: HashMap<usize, u64>,
    /// First page that is in view, for the dump and the page indicator.
    pub current: usize,
    go_to: Option<usize>,
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

    pub fn go_to(&mut self, page: usize) {
        self.go_to = Some(page.min(self.page_count().saturating_sub(1)));
    }

    pub fn zoom_by(&mut self, factor: f32, fitted: f32) {
        let now = if self.zoom > 0.0 { self.zoom } else { fitted };
        self.zoom = (now * factor).clamp(0.2, 6.0);
    }
}

thread_local! {
    /// The document each worker read last, so that pages of one file do not each parse it again.
    static OPEN: RefCell<Option<(usize, usize, Pdf)>> = const { RefCell::new(None) };
}

fn with_pdf<T>(bytes: &Arc<[u8]>, f: impl FnOnce(&Pdf) -> Result<T, String>) -> Result<T, String> {
    let id = (bytes.as_ptr() as usize, bytes.len());
    OPEN.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.as_ref().is_none_or(|(p, l, _)| (*p, *l) != id) {
            *slot = None;
            let pdf = Pdf::new(Arc::new(bytes.clone())).map_err(|e| match format!("{e:?}") {
                s if s.to_lowercase().contains("encrypt") => "This PDF is password-protected, which Inkdown cannot open yet.".to_string(),
                _ => "This file is not a PDF that Inkdown can read.".to_string(),
            })?;
            *slot = Some((id.0, id.1, pdf));
        }
        f(&slot.as_ref().unwrap().2)
    })
}

fn guarded<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| Err("This PDF could not be read.".into()))
}

/// Page sizes in points.
pub fn page_sizes(bytes: &Arc<[u8]>) -> Result<Vec<Vec2>, String> {
    guarded(|| {
        with_pdf(bytes, |pdf| {
            let sizes: Vec<Vec2> = pdf.pages().iter().map(|p| p.render_dimensions()).map(|(w, h)| vec2(w.max(1.0), h.max(1.0))).collect();
            if sizes.is_empty() {
                return Err("This PDF has no pages.".into());
            }
            Ok(sizes)
        })
    })
}

/// Draws one page `px_w` pixels wide.
pub fn render_page(bytes: &Arc<[u8]>, page: usize, px_w: f32) -> Result<Raster, String> {
    guarded(|| {
        with_pdf(bytes, |pdf| {
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

fn job_page(bytes: Arc<[u8]>, page: usize, px_w: f32) -> Job {
    Box::new(move || render_page(&bytes, page, px_w))
}

/// The pages of a PDF, scrollable and zoomable.
pub fn show(ui: &mut Ui, view: &mut PdfView, id: Id, bytes: &Arc<[u8]>, cx: &mut ViewCtx<'_>) {
    let pal = *cx.pal;
    let sizes = match view.pages.get_or_insert_with(|| page_sizes(bytes)) {
        Ok(s) => s.clone(),
        Err(e) => {
            let e = e.clone();
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new(e).size(14.0).color(pal.muted));
            });
            return;
        }
    };
    let full = ui.available_rect_before_wrap();
    let widest = sizes.iter().map(|s| s.x).fold(1.0, f32::max);
    let phone = full.width() < 560.0;
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
    let file_key = hash64(&[&"pdf", &bytes.len(), &(bytes.as_ptr() as usize)]);
    let total_h: f32 = sizes.iter().map(|s| s.y * scale + GAP).sum::<f32>() + GAP;
    let content_w = (widest * scale + 2.0 * margin).max(full.width() - 12.0);
    let mut area = egui::ScrollArea::both().id_salt(id).auto_shrink([false, false]);
    if let Some(p) = view.go_to.take() {
        let y: f32 = sizes.iter().take(p).map(|s| s.y * scale + GAP).sum();
        area = area.vertical_scroll_offset(y);
    }
    let mut first_visible = None;
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
            // the page under the middle of the view is the current one
            let middle = ui.clip_rect().center().y;
            if rect.top() - GAP <= middle && middle <= rect.bottom() {
                first_visible = Some(ix);
            }
            ui.painter().rect_filled(rect.translate(vec2(0.0, 1.5)).expand(1.0), CornerRadius::same(2), Color32::from_black_alpha(if pal.dark { 90 } else { 40 }));
            ui.painter().rect_filled(rect, CornerRadius::ZERO, Color32::WHITE);
            // sharp at the present zoom, in steps so that small zoom changes reuse a picture
            let want = (shown.x * ppp / 160.0).ceil().max(1.0) * 160.0;
            let key = hash64(&[&file_key, &ix, &(want as u32)]);
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            let ready = match cx.assets.get(key, || job_page(bytes.clone(), ix, want)) {
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
        }
    });
    if let Some(p) = first_visible {
        view.current = p;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-page PDF with correct offsets, built by hand.
    pub fn sample(pages: usize) -> Vec<u8> {
        let mut objs: Vec<String> = Vec::new();
        let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
        objs.push("<< /Type /Catalog /Pages 2 0 R >>".into());
        objs.push(format!("<< /Type /Pages /Kids [{}] /Count {pages} >>", kids.join(" ")));
        objs.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
        for i in 0..pages {
            let content = format!("BT /F1 36 Tf 72 700 Td (Page {}) Tj ET\n0 0 1 rg 72 600 200 40 re f", i + 1);
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
        let sizes = page_sizes(&bytes).unwrap();
        assert_eq!(sizes.len(), 3);
        assert_eq!((sizes[0].x, sizes[0].y), (612.0, 792.0));
        let r = render_page(&bytes, 1, 306.0).unwrap();
        assert_eq!((r.w, r.h), (306, 396));
        assert_eq!(r.rgba.len(), r.w * r.h * 4);
        // a white page with something blue and something dark on it
        let px = |x: usize, y: usize| &r.rgba[(y * r.w + x) * 4..(y * r.w + x) * 4 + 4];
        assert_eq!(px(5, 5), &[255, 255, 255, 255]);
        assert_eq!(px(80, 85), &[0, 0, 255, 255]);
        assert!(r.rgba.chunks(4).any(|p| p[0] < 60 && p[1] < 60 && p[2] < 60), "the text was drawn");
        assert!(render_page(&bytes, 9, 300.0).is_err());
        let junk: Arc<[u8]> = b"not a pdf at all".to_vec().into();
        assert!(page_sizes(&junk).is_err());
    }
}
