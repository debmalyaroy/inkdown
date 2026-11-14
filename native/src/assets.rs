//! Images, diagrams, math and charts: everything that becomes a texture.
//!
//! Work happens on background threads; the UI shows a placeholder until the result
//! arrives. Textures that have not been drawn for a while are dropped to keep memory low.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};

use egui::{ColorImage, TextureHandle, Vec2};
use resvg::{tiny_skia, usvg};

pub fn hash64(parts: &[&dyn HashDyn]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for p in parts {
        p.hash_dyn(&mut h);
    }
    h.finish()
}
pub trait HashDyn {
    fn hash_dyn(&self, h: &mut std::collections::hash_map::DefaultHasher);
}
impl<T: Hash + ?Sized> HashDyn for T {
    fn hash_dyn(&self, h: &mut std::collections::hash_map::DefaultHasher) {
        self.hash(h)
    }
}

pub struct Raster {
    pub rgba: Vec<u8>,
    pub w: usize,
    pub h: usize,
    /// Size in points at 100 % zoom.
    pub size: Vec2,
    /// Distance from the bottom edge up to the text baseline (math only).
    pub depth: f32,
    pub svg: Option<Arc<str>>,
}

pub struct Asset {
    pub tex: TextureHandle,
    pub size: Vec2,
    pub depth: f32,
    pub svg: Option<Arc<str>>,
    last_used: u64,
    bytes: usize,
}

enum Slot {
    Pending,
    Ready(Asset),
    Failed(Arc<str>),
}

pub enum State<'a> {
    Pending,
    Ready(&'a Asset),
    Failed(Arc<str>),
}

type Job = Box<dyn FnOnce() -> Result<Raster, String> + Send>;

pub struct Assets {
    slots: HashMap<u64, Slot>,
    jobs: Sender<(u64, Job)>,
    done: Receiver<(u64, Result<Raster, String>)>,
    /// Bumped whenever a pending asset resolves, so layouts waiting on it rebuild.
    pub epoch: u64,
    frame: u64,
}

impl Assets {
    pub fn new(ctx: &egui::Context) -> Self {
        let (jobs, job_rx) = channel::<(u64, Job)>();
        let (done_tx, done) = channel();
        let job_rx = Arc::new(Mutex::new(job_rx));
        for i in 0..2 {
            let rx = job_rx.clone();
            let tx = done_tx.clone();
            let ctx = ctx.clone();
            let _ = std::thread::Builder::new().name(format!("ink-assets-{i}")).stack_size(8 * 1024 * 1024).spawn(move || loop {
                let next = rx.lock().unwrap().recv();
                let Ok((key, job)) = next else { break };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).unwrap_or_else(|_| Err("The renderer failed on this input.".into()));
                if tx.send((key, result)).is_err() {
                    break;
                }
                ctx.request_repaint();
            });
        }
        Self { slots: HashMap::new(), jobs, done, epoch: 0, frame: 0 }
    }

    /// Collect finished work. Call once per frame.
    pub fn poll(&mut self, ctx: &egui::Context) {
        self.frame += 1;
        while let Ok((key, result)) = self.done.try_recv() {
            let slot = match result {
                Ok(r) => {
                    let image = ColorImage::from_rgba_premultiplied([r.w, r.h], &r.rgba);
                    let bytes = r.rgba.len();
                    let tex = ctx.load_texture(format!("asset-{key:x}"), image, egui::TextureOptions::LINEAR);
                    Slot::Ready(Asset { tex, size: r.size, depth: r.depth, svg: r.svg, last_used: self.frame, bytes })
                }
                Err(e) => Slot::Failed(e.into()),
            };
            self.slots.insert(key, slot);
            self.epoch += 1;
        }
        if self.frame % 120 == 0 {
            let total: usize = self.slots.values().map(|s| if let Slot::Ready(a) = s { a.bytes } else { 0 }).sum();
            let frame = self.frame;
            // unused for a while, or over budget: drop the oldest
            let limit = if total > 160 << 20 { 240 } else { 3600 };
            self.slots.retain(|_, s| match s {
                Slot::Ready(a) => frame - a.last_used < limit,
                _ => true,
            });
        }
    }

    pub fn get(&mut self, key: u64, make: impl FnOnce() -> Job) -> State<'_> {
        let frame = self.frame;
        if !self.slots.contains_key(&key) {
            self.slots.insert(key, Slot::Pending);
            let _ = self.jobs.send((key, make()));
        }
        match self.slots.get_mut(&key) {
            Some(Slot::Ready(a)) => {
                a.last_used = frame;
                State::Ready(a)
            }
            Some(Slot::Failed(e)) => State::Failed(e.clone()),
            _ => State::Pending,
        }
    }

    pub fn peek(&mut self, key: u64) -> Option<&Asset> {
        let frame = self.frame;
        match self.slots.get_mut(&key) {
            Some(Slot::Ready(a)) => {
                a.last_used = frame;
                Some(a)
            }
            _ => None,
        }
    }

    pub fn forget(&mut self, key: u64) {
        self.slots.remove(&key);
    }
}

// ------------------------------------------------------------ SVG raster

pub fn fontdb() -> Arc<usvg::fontdb::Database> {
    static DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_system_fonts();
        db.load_font_data(include_bytes!("../fonts/Inter.ttf").to_vec());
        db.load_font_data(include_bytes!("../fonts/Inter-Italic.ttf").to_vec());
        db.load_font_data(include_bytes!("../fonts/JetBrainsMono.ttf").to_vec());
        let (sans, serif, mono) = {
            let has = |name: &str| db.faces().any(|f| f.families.iter().any(|(n, _)| n == name));
            let sans = ["Segoe UI", "Inter", "Roboto", "Helvetica Neue", "Arial"].into_iter().find(|n| has(n)).unwrap_or("Inter");
            let mono = ["Cascadia Mono", "JetBrains Mono", "Consolas", "Menlo"].into_iter().find(|n| has(n)).unwrap_or("JetBrains Mono");
            (sans, if has("Georgia") { "Georgia" } else { sans }, mono)
        };
        db.set_sans_serif_family(sans);
        db.set_serif_family(serif);
        db.set_monospace_family(mono);
        db.set_cursive_family(sans);
        db.set_fantasy_family(sans);
        Arc::new(db)
    })
    .clone()
}

pub fn svg_tree(svg: &str, base: Option<&std::path::Path>) -> Result<usvg::Tree, String> {
    let mut opt = usvg::Options { fontdb: fontdb(), ..Default::default() };
    opt.font_family = "sans-serif".into();
    opt.resources_dir = base.map(|p| p.to_path_buf());
    usvg::Tree::from_str(svg, &opt).map_err(|e| format!("Invalid SVG: {e}"))
}

/// Rasterise an SVG. `max_w` is the widest it may be shown, in points.
pub fn raster_svg(svg: &str, ppp: f32, max_w: f32, base: Option<&std::path::Path>) -> Result<Raster, String> {
    let tree = svg_tree(svg, base)?;
    let size = tree.size();
    let (w, h) = (size.width().max(1.0), size.height().max(1.0));
    let fit = (max_w / w).min(1.0);
    let scale = (ppp * fit).min(4000.0 / w.max(h)).max(0.05);
    let (pw, ph) = (((w * scale).ceil() as u32).max(1), ((h * scale).ceil() as u32).max(1));
    let mut pixmap = tiny_skia::Pixmap::new(pw, ph).ok_or("Image is too large.")?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Ok(Raster { rgba: pixmap.take(), w: pw as usize, h: ph as usize, size: Vec2::new(w * fit, h * fit), depth: 0.0, svg: Some(svg.into()) })
}

pub fn raster_image(bytes: &[u8], max_px: u32) -> Result<Raster, String> {
    let head = &bytes[..bytes.len().min(512)];
    let text = String::from_utf8_lossy(head);
    if text.contains("<svg") || text.trim_start().starts_with("<?xml") {
        return raster_svg(std::str::from_utf8(bytes).map_err(|_| "Invalid SVG text.")?, 2.0, 4000.0, None);
    }
    let img = image::load_from_memory(bytes).map_err(|e| format!("Cannot decode image: {e}"))?;
    let (w0, h0) = (img.width(), img.height());
    let img = if w0.max(h0) > max_px { img.resize(max_px, max_px, image::imageops::FilterType::Triangle) } else { img };
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    let mut data = rgba.into_raw();
    for px in data.chunks_exact_mut(4) {
        let a = px[3] as u16;
        if a < 255 {
            px[0] = ((px[0] as u16 * a + 127) / 255) as u8;
            px[1] = ((px[1] as u16 * a + 127) / 255) as u8;
            px[2] = ((px[2] as u16 * a + 127) / 255) as u8;
        }
    }
    Ok(Raster { rgba: data, w, h, size: Vec2::new(w0 as f32, h0 as f32), depth: 0.0, svg: None })
}

// ------------------------------------------------------------ renderers

pub fn mermaid_svg(src: &str, dark: bool) -> Result<String, String> {
    let src = src.to_string();
    std::panic::catch_unwind(move || {
        let mut opt = mermaid_rs_renderer::RenderOptions::default();
        if dark {
            opt.theme = mermaid_rs_renderer::Theme::dark();
        }
        mermaid_rs_renderer::render_with_options(&src, opt).map_err(|e| e.to_string())
    })
    .unwrap_or_else(|_| Err("This diagram type is not supported by the native renderer yet.".into()))
}

pub fn dot_svg(src: &str) -> Result<String, String> {
    use layout::backends::svg::SVGWriter;
    use layout::gv::{DotParser, GraphBuilder};
    let src = src.to_string();
    std::panic::catch_unwind(move || {
        let mut parser = DotParser::new(&src);
        let graph = parser.process()?;
        let mut builder = GraphBuilder::new();
        builder.visit_graph(&graph);
        let mut vg = builder.get();
        let mut svg = SVGWriter::new();
        vg.do_it(false, false, false, &mut svg);
        Ok(svg.finalize())
    })
    .unwrap_or_else(|_| Err("Graphviz layout failed.".into()))
}

pub fn math_font() -> Option<&'static latex_rust::MathFont> {
    static F: OnceLock<Option<latex_rust::MathFont>> = OnceLock::new();
    F.get_or_init(|| latex_rust::MathFont::stix_two_math().ok()).as_ref()
}

fn dim(d: &latex_rust::Dim) -> f32 {
    d.as_ratio().map(|(n, den)| if den == 0 { 0.0 } else { n as f64 / den as f64 }).unwrap_or(0.0) as f32
}

/// LaTeX math to SVG. Returns `(svg, width, height, depth)` in points.
pub fn math_svg(tex: &str, display: bool, size: f32, color: egui::Color32) -> Result<(String, f32, f32, f32), String> {
    let font = math_font().ok_or("Math font failed to load.")?;
    let tex = tex.trim().to_string();
    std::panic::catch_unwind(move || {
        // box dimensions come back in ems; the SVG itself is drawn at 12 pt and scaled when rasterised
        let em = latex_rust::Dim::from_i64(12);
        let mut opt = latex_rust::SvgOptions::new();
        opt.display = display;
        opt.color = latex_rust::Color::rgb(color.r(), color.g(), color.b());
        opt.font_size_pt = em.clone();
        let ast = latex_rust::parse(&tex).map_err(|e| e.to_string())?;
        let style = if display { latex_rust::MathStyle::Display } else { latex_rust::MathStyle::Text };
        let boxed = latex_rust::layout_with_em_size_pt(&ast, font, style, &em).map_err(|e| e.to_string())?;
        let svg = latex_rust::render_svg(&boxed, font, &opt).map_err(|e| e.to_string())?;
        let (w, h, d) = (dim(&boxed.width) * size, dim(&boxed.height) * size, dim(&boxed.depth) * size);
        Ok((svg, w, h + d, d))
    })
    .unwrap_or_else(|_| Err("Could not typeset this formula.".into()))
}

// ---------------------------------------------------------------- jobs

pub fn job_svg(svg: String, ppp: f32, max_w: f32, base: Option<PathBuf>) -> Job {
    Box::new(move || raster_svg(&svg, ppp, max_w, base.as_deref()))
}

pub fn job_mermaid(src: String, dark: bool, ppp: f32, max_w: f32) -> Job {
    Box::new(move || raster_svg(&without_backdrop(mermaid_svg(&src, dark)?), ppp, max_w, None))
}

/// Drop the full-size background rectangle the Mermaid renderer paints first, so a diagram
/// sits directly on the page in every theme (exports keep it).
fn without_backdrop(mut svg: String) -> String {
    if let Some(start) = svg.find("<rect ") {
        let first_shape = ["<g", "<path", "<text", "<circle", "<polygon", "<line", "<ellipse"].iter().filter_map(|t| svg.find(t)).min().unwrap_or(usize::MAX);
        if start < first_shape {
            if let Some(len) = svg[start..].find("/>") {
                svg.replace_range(start..start + len + 2, "");
            }
        }
    }
    svg
}

pub fn job_dot(src: String, ppp: f32, max_w: f32) -> Job {
    Box::new(move || raster_svg(&dot_svg(&src)?, ppp, max_w, None))
}

pub fn job_chart(src: String, dark: bool, ppp: f32, max_w: f32) -> Job {
    Box::new(move || raster_svg(&crate::chart::chart_svg(&src, dark, max_w.min(760.0))?, ppp, max_w, None))
}

pub fn job_math(tex: String, display: bool, size: f32, color: egui::Color32, ppp: f32) -> Job {
    Box::new(move || {
        let (svg, w, h, depth) = math_svg(&tex, display, size, color)?;
        let tree = svg_tree(&svg, None)?;
        let ts = tree.size();
        let pad = 1.0f32;
        let scale = ppp * (w / ts.width().max(0.001));
        let (pw, ph) = ((((w + pad * 2.0) * ppp).ceil() as u32).max(1), (((h + pad * 2.0) * ppp).ceil() as u32).max(1));
        if pw > 8000 || ph > 8000 {
            return Err("Formula is too large.".into());
        }
        let mut pixmap = tiny_skia::Pixmap::new(pw, ph).ok_or("Formula is too large.")?;
        resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale).post_translate(pad * ppp, pad * ppp), &mut pixmap.as_mut());
        Ok(Raster { rgba: pixmap.take(), w: pw as usize, h: ph as usize, size: Vec2::new(w + pad * 2.0, h + pad * 2.0), depth: depth + pad, svg: Some(svg.into()) })
    })
}

/// A local file, a `data:` URI or an http(s) URL.
pub fn job_image(url: String, base: Option<PathBuf>, ppp: f32, max_w: f32) -> Job {
    Box::new(move || {
        let bytes = load_bytes(&url, base.as_deref())?;
        let is_svg = url.to_lowercase().split(['?', '#']).next().unwrap_or("").ends_with(".svg") || bytes.iter().take(256).collect::<Vec<_>>().windows(4).any(|w| w == [&b'<', &b's', &b'v', &b'g']);
        if is_svg {
            let text = String::from_utf8_lossy(&bytes).to_string();
            return raster_svg(&text, ppp, max_w, base.as_deref());
        }
        raster_image(&bytes, ((max_w * ppp) as u32).clamp(256, 4096))
    })
}

pub fn job_bytes(bytes: Arc<[u8]>) -> Job {
    Box::new(move || raster_image(&bytes, 8192))
}

pub fn resolve_path(url: &str, base: Option<&std::path::Path>) -> Option<PathBuf> {
    let clean = url.split(['?', '#']).next().unwrap_or(url);
    let clean = clean.strip_prefix("file:///").or_else(|| clean.strip_prefix("file://")).unwrap_or(clean);
    let decoded = percent_decode(clean);
    let p = PathBuf::from(&decoded);
    if p.is_absolute() {
        return Some(p);
    }
    base.map(|b| b.join(p))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 3 <= bytes.len() {
            if let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

pub fn load_bytes(url: &str, base: Option<&std::path::Path>) -> Result<Vec<u8>, String> {
    if url == "inkdown:icon.png" {
        return Ok(include_bytes!("../../assets/icon.png").to_vec());
    }
    if let Some(rest) = url.strip_prefix("data:") {
        let (meta, data) = rest.split_once(',').ok_or("Malformed data URI.")?;
        return if meta.ends_with(";base64") {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.decode(data.trim()).map_err(|e| e.to_string())
        } else {
            Ok(percent_decode(data).into_bytes())
        };
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        return fetch(url);
    }
    let path = resolve_path(url, base).ok_or("Relative image path in an unsaved document.")?;
    std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn fetch(url: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(20))).build().into();
    let mut resp = agent.get(url).call().map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    resp.body_mut().as_reader().take(40 << 20).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}
#[cfg(any(target_os = "android", target_os = "ios"))]
fn fetch(_url: &str) -> Result<Vec<u8>, String> {
    Err("Remote images are not loaded on this device.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_math_and_svg() {
        let (svg, w, h, d) = math_svg(r"\frac{a}{b} + \sqrt{x}", true, 16.0, egui::Color32::BLACK).unwrap();
        assert!(svg.contains("<path") && w > 20.0 && h > 10.0 && d > 0.0 && d < h);
        assert!(math_svg(r"\frac{a}{", false, 16.0, egui::Color32::BLACK).is_err());
        let r = raster_svg(r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="red"/></svg>"#, 2.0, 1000.0, None).unwrap();
        assert_eq!((r.w, r.h), (80, 40));
        assert_eq!(&r.rgba[..4], &[255, 0, 0, 255]);
        let m = mermaid_svg("flowchart LR\n  A[Start] --> B{Ok?}\n  B -->|yes| C[Done]", false).unwrap();
        assert!(m.contains("<svg"));
        let bare = without_backdrop(m.clone());
        assert!(bare.len() < m.len() && bare.matches("<rect ").count() + 1 == m.matches("<rect ").count());
        let d = dot_svg("digraph { a -> b; b -> c; }").unwrap();
        assert!(d.contains("<svg"));
        assert_eq!(percent_decode("a%20b.png"), "a b.png");
    }
}
