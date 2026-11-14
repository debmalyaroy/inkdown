//! Export commands (HTML, PDF, Word, RTF, clipboard, diagram images, Google Docs)
//! and the Integrations dialog.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver};

use crate::app::InkApp;
use crate::assets;
use crate::doc::Kind;
use crate::docx;
use crate::html::{self, HtmlOpts};
use crate::icons;
use crate::md::{self, Block, DiagramKind, Inl};
use crate::theme;

pub enum Done {
    Toast(String),
    Open(String, String),
    Connected(Result<(), String>),
}

#[derive(Default)]
pub struct Integ {
    pub client_id: String,
    pub secret: String,
    pub loaded: bool,
    pub connected: bool,
    pub busy: bool,
    pub error: Option<String>,
    pub rx: Option<Receiver<Done>>,
}

impl Integ {
    /// Collect results of background exports. Returns messages for the app to show.
    pub fn poll(&mut self) -> Vec<Done> {
        let mut out = Vec::new();
        if let Some(rx) = &self.rx {
            loop {
                match rx.try_recv() {
                    Ok(d) => out.push(d),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(_) => {
                        self.rx = None;
                        self.busy = false;
                        break;
                    }
                }
            }
        }
        if !out.is_empty() {
            self.busy = false;
        }
        out
    }
    pub fn running(&self) -> bool {
        self.busy
    }
}

fn markdown_of(app: &InkApp) -> Option<(String, String, Option<PathBuf>)> {
    let d = app.docs.get(app.active)?;
    let text = match d.kind {
        Kind::Markdown => d.buf.text(),
        Kind::Asciidoc => crate::adoc::to_markdown(&d.buf.text()),
        _ => return None,
    };
    let stem = d.name.rsplit_once('.').map(|x| x.0.to_string()).unwrap_or_else(|| d.name.clone());
    Some((text, stem, d.dir()))
}

fn img_from_raster(r: &assets::Raster) -> docx::Img {
    docx::Img { jpeg: false, data: html::png_bytes(r), w: r.size.x, h: r.size.y, depth: r.depth }
}

fn diagram_svg(kind: DiagramKind, src: &str) -> Result<String, String> {
    match kind {
        DiagramKind::Mermaid => assets::mermaid_svg(src, false),
        DiagramKind::Dot => assets::dot_svg(src),
        DiagramKind::Svg => Ok(src.to_string()),
        DiagramKind::Chart => crate::chart::chart_svg(src, false, 720.0),
    }
}

fn math_img(tex: &str, display: bool) -> Option<docx::Img> {
    let (svg, w, h, depth) = assets::math_svg(tex, display, if display { 18.0 } else { 16.0 }, egui::Color32::from_rgb(0x1f, 0x23, 0x28)).ok()?;
    let tree = assets::svg_tree(&svg, None).ok()?;
    let k = 4.0;
    let scale = k * w / tree.size().width().max(0.01);
    let (pw, ph) = (((w * k).ceil() as u32).max(1), ((h * k).ceil() as u32).max(1));
    let mut pm = resvg::tiny_skia::Pixmap::new(pw, ph)?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pm.as_mut());
    let r = assets::Raster { rgba: pm.take(), w: pw as usize, h: ph as usize, size: egui::vec2(w, h), depth, svg: None };
    Some(img_from_raster(&r))
}

fn collect_inl(inl: &[Inl], base: Option<&Path>, a: &mut docx::Assets) {
    for i in inl {
        match i {
            Inl::Math(tex) => {
                let t = tex.trim();
                for display in [false, true] {
                    let key = format!("{}|{t}", display as u8);
                    if !a.math.contains_key(&key) {
                        if let Some(img) = math_img(t, display) {
                            a.math.insert(key, Rc::new(img));
                        }
                    }
                }
            }
            Inl::Image { url, .. } => image_asset(url, base, a),
            _ => {}
        }
    }
}

fn image_asset(url: &str, base: Option<&Path>, a: &mut docx::Assets) {
    if a.images.contains_key(url) {
        return;
    }
    let Ok(bytes) = assets::load_bytes(url, base) else { return };
    let is_svg = url.to_lowercase().ends_with(".svg") || bytes.starts_with(b"<svg") || bytes.starts_with(b"<?xml");
    let img = if is_svg {
        assets::raster_svg(&String::from_utf8_lossy(&bytes), 2.0, 1200.0, base).ok().map(|r| img_from_raster(&r))
    } else {
        match image::guess_format(&bytes) {
            Ok(fmt @ (image::ImageFormat::Png | image::ImageFormat::Jpeg)) => image::load_from_memory(&bytes).ok().map(|im| docx::Img { jpeg: fmt == image::ImageFormat::Jpeg, w: im.width() as f32, h: im.height() as f32, data: bytes.clone(), depth: 0.0 }),
            _ => assets::raster_image(&bytes, 2400).ok().map(|r| img_from_raster(&r)),
        }
    };
    if let Some(img) = img {
        a.images.insert(url.to_string(), Rc::new(img));
    }
}

fn collect_blocks(blocks: &[&Block], base: Option<&Path>, a: &mut docx::Assets) {
    for b in blocks {
        match b {
            Block::Heading { inl, .. } | Block::Para(inl) => collect_inl(inl, base, a),
            Block::Quote(body) | Block::Alert { body, .. } | Block::Footnote { body, .. } | Block::Center(body) => collect_blocks(&body.iter().collect::<Vec<_>>(), base, a),
            Block::Details { summary, body } => {
                collect_inl(summary, base, a);
                collect_blocks(&body.iter().collect::<Vec<_>>(), base, a);
            }
            Block::List { items, .. } => {
                for it in items {
                    collect_blocks(&it.blocks.iter().collect::<Vec<_>>(), base, a);
                }
            }
            Block::Table { rows, .. } => {
                for r in rows {
                    for c in r {
                        collect_inl(c, base, a);
                    }
                }
            }
            Block::DefList(items) => {
                for (t, defs) in items {
                    collect_inl(t, base, a);
                    for d in defs {
                        collect_blocks(&d.iter().collect::<Vec<_>>(), base, a);
                    }
                }
            }
            Block::Math(tex) => {
                if let Some(img) = math_img(tex.trim(), true) {
                    a.math.insert(format!("1|{}", tex.trim()), Rc::new(img));
                }
            }
            Block::Diagram { kind, src } => {
                if let Ok(r) = diagram_svg(*kind, src).and_then(|svg| assets::raster_svg(&svg, 2.0, 100_000.0, base)) {
                    a.diagrams.insert(src.trim().to_string(), Rc::new(img_from_raster(&r)));
                }
            }
            Block::Image { url, .. } => image_asset(url, base, a),
            _ => {}
        }
    }
}

/// Pre-render everything Word cannot draw itself.
pub fn collect_assets(markdown: &str, base: Option<&Path>) -> docx::Assets {
    let tops = md::parse(markdown);
    let mut a = docx::Assets::default();
    collect_blocks(&tops.iter().map(|t| &t.block).collect::<Vec<_>>(), base, &mut a);
    a
}

pub fn docx_bytes(markdown: &str, title: &str, base: Option<&Path>) -> Result<Vec<u8>, String> {
    let assets = collect_assets(markdown, base);
    docx::write_docx(&docx::build(markdown, &assets, title))
}

pub fn html_page(markdown: &str, stem: &str, base: Option<&Path>, png: bool) -> (String, String) {
    let pal = theme::palette("light", false);
    let tops = md::parse(markdown);
    let title = html::title_of(&tops, stem);
    (html::document(&tops, &title, &HtmlOpts { base, png, pal: &pal }), title)
}

/// A Chromium-family browser that can print to PDF without showing a window.
pub fn pdf_browser() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    // INKDOWN_BROWSER names the browser to use when it is installed somewhere unusual
    if let Some(p) = std::env::var_os("INKDOWN_BROWSER") {
        candidates.push(PathBuf::from(p));
    }
    #[cfg(windows)]
    for var in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"] {
        if let Some(root) = std::env::var_os(var) {
            let root = PathBuf::from(root);
            candidates.push(root.join(r"Microsoft\Edge\Application\msedge.exe"));
            candidates.push(root.join(r"Google\Chrome\Application\chrome.exe"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        // system-wide and per-user application folders
        let mut roots = vec![PathBuf::from("/Applications")];
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join("Applications"));
        }
        for root in roots {
            for app in ["Google Chrome.app/Contents/MacOS/Google Chrome", "Microsoft Edge.app/Contents/MacOS/Microsoft Edge", "Chromium.app/Contents/MacOS/Chromium"] {
                candidates.push(root.join(app));
            }
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    for name in ["chromium", "chromium-browser", "google-chrome", "google-chrome-stable", "microsoft-edge"] {
        for dir in std::env::var("PATH").unwrap_or_default().split(':') {
            candidates.push(Path::new(dir).join(name));
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn html_to_pdf(html: &str, out: &Path) -> Result<(), String> {
    let browser = pdf_browser().ok_or("PDF export needs Microsoft Edge or Chrome on this computer. Export HTML and print it instead.")?;
    let dir = std::env::temp_dir().join(format!("inkdown-pdf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let page = dir.join("page.html");
    std::fs::write(&page, html).map_err(|e| e.to_string())?;
    let url = format!("file:///{}", page.to_string_lossy().replace('\\', "/").trim_start_matches('/'));
    let _ = std::fs::remove_file(out);
    let made = |out: &Path| out.is_file() && std::fs::metadata(out).map(|m| m.len() > 500).unwrap_or(false);
    // browsers differ in which headless mode still prints, and some return before the file is written
    for mode in ["--headless=old", "--headless=new", "--headless"] {
        let mut c = std::process::Command::new(&browser);
        c.arg(mode).arg("--disable-gpu").arg("--no-first-run").arg("--disable-extensions").arg("--no-pdf-header-footer").arg(format!("--user-data-dir={}", dir.join("profile").display())).arg(format!("--print-to-pdf={}", out.display())).arg(&url);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000);
        }
        let _ = c.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(12);
        while !made(out) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        if made(out) {
            break;
        }
    }
    // the file may still be growing for a moment
    std::thread::sleep(std::time::Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&dir);
    if made(out) {
        Ok(())
    } else {
        Err("The browser did not produce a PDF. Use Export > HTML and print that instead.".into())
    }
}
fn save_as(app: &InkApp, name: &str) -> Option<PathBuf> {
    let dir = app.docs.get(app.active).and_then(|d| d.dir()).or_else(|| app.sidebar.folder.clone());
    crate::platform::save_dialog(name, dir)
}

fn background(app: &mut InkApp, ctx: &egui::Context, work: impl FnOnce() -> Done + Send + 'static) {
    let (tx, rx) = channel();
    let ctx = ctx.clone();
    app.integ.busy = true;
    app.integ.rx = Some(rx);
    std::thread::spawn(move || {
        let _ = tx.send(work());
        ctx.request_repaint();
    });
}

pub fn command(app: &mut InkApp, ctx: &egui::Context, name: &str) {
    // diagram documents
    if matches!(name, "export_png" | "export_svg") {
        let Some(d) = app.docs.get(app.active) else { return };
        let text = d.buf.text();
        let svg = match d.kind {
            Kind::Mermaid => assets::mermaid_svg(&text, false),
            Kind::Dot => assets::dot_svg(&text),
            Kind::Svg => Ok(text),
            _ => Err("Open a diagram (.mmd, .dot or .svg) to export it as an image.".to_string()),
        };
        let stem = d.name.rsplit_once('.').map(|x| x.0.to_string()).unwrap_or_else(|| d.name.clone());
        let base = d.dir();
        let result = svg.and_then(|svg| {
            let ext = if name == "export_png" { "png" } else { "svg" };
            let Some(path) = save_as(app, &format!("{stem}.{ext}")) else { return Ok(None) };
            let bytes = if ext == "png" { html::png_bytes(&assets::raster_svg(&svg, 3.0, 100_000.0, base.as_deref())?) } else { svg.into_bytes() };
            std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
            Ok(Some(path))
        });
        match result {
            Ok(Some(p)) => app.toast(format!("Saved {}", p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())),
            Ok(None) => {}
            Err(e) => app.toast(e),
        }
        return;
    }
    let Some((text, stem, base)) = markdown_of(app) else {
        app.toast("Open a Markdown or AsciiDoc document to export it.");
        return;
    };
    match name {
        "export_html" => {
            if let Some(path) = save_as(app, &format!("{stem}.html")) {
                let (page, _) = html_page(&text, &stem, base.as_deref(), false);
                match std::fs::write(&path, page) {
                    Ok(()) => app.toast("HTML saved"),
                    Err(e) => app.toast(format!("Could not save: {e}")),
                }
            }
        }
        "export_docx" | "export_rtf" => {
            let ext = if name == "export_docx" { "docx" } else { "rtf" };
            if let Some(path) = save_as(app, &format!("{stem}.{ext}")) {
                let a = collect_assets(&text, base.as_deref());
                let doc = docx::build(&text, &a, &stem);
                let bytes = if ext == "docx" { docx::write_docx(&doc) } else { Ok(docx::write_rtf(&doc)) };
                match bytes.and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string())) {
                    Ok(()) => app.toast(if ext == "docx" { "Word document saved" } else { "Rich text saved" }),
                    Err(e) => app.toast(format!("Could not export: {e}")),
                }
            }
        }
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        "export_pdf" => {
            if pdf_browser().is_none() {
                app.toast("PDF export needs Microsoft Edge or Chrome. Use Export > HTML and print that instead.");
                return;
            }
            if let Some(path) = save_as(app, &format!("{stem}.pdf")) {
                app.toast("Creating PDF…");
                background(app, ctx, move || {
                    let (page, _) = html_page(&text, &stem, base.as_deref(), false);
                    match html_to_pdf(&page, &path) {
                        Ok(()) => Done::Toast("PDF saved".into()),
                        Err(e) => Done::Toast(e),
                    }
                });
            }
        }
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        "print" => {
            let (page, _) = html_page(&text, &stem, base.as_deref(), false);
            let path = std::env::temp_dir().join(format!("{stem}-print.html"));
            match std::fs::write(&path, page.replace("</body>", "<script>window.onload=()=>window.print()</script></body>")) {
                Ok(()) => crate::platform::shell_open(&path.to_string_lossy()),
                Err(e) => app.toast(format!("Could not prepare the page: {e}")),
            }
        }
        "export_copy_html" => {
            let pal = theme::palette("light", false);
            let tops = md::parse(&text);
            ctx.copy_text(html::body(&tops, &HtmlOpts { base: base.as_deref(), png: false, pal: &pal }));
            app.toast("HTML copied");
        }
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        "export_copy_rich" => {
            let (page, _) = html_page(&text, &stem, base.as_deref(), true);
            let plain = md::parse(&text).iter().map(|t| plain_block(&t.block)).collect::<Vec<_>>().join("\n\n");
            match arboard::Clipboard::new().and_then(|mut c| c.set_html(page, Some(plain))) {
                Ok(()) => app.toast("Copied. Paste into Word, Google Docs or an email."),
                Err(e) => app.toast(format!("Could not copy: {e}")),
            }
        }
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        "export_gdoc" => {
            let status = crate::google::status();
            if status["connected"].as_bool() != Some(true) {
                app.dialog = Some(crate::app::Dialog::Integrations);
                app.toast("Connect your Google account first");
                return;
            }
            app.toast("Uploading to Google Docs…");
            background(app, ctx, move || {
                let (_, title) = html_page("", &stem, None, false);
                let title = md::parse(&text).first().map(|_| html::title_of(&md::parse(&text), &title)).unwrap_or(title);
                match docx_bytes(&text, &title, base.as_deref()).and_then(|b| crate::google::upload_docx(&b, &title)) {
                    Ok(v) => Done::Open(v["webViewLink"].as_str().unwrap_or("").to_string(), "Uploaded to Google Docs".into()),
                    Err(e) => Done::Toast(e),
                }
            });
        }
        other => app.toast(format!("{} is not available on this device", other.trim_start_matches("export_"))),
    }
}

fn plain_block(b: &Block) -> String {
    match b {
        Block::Heading { inl, .. } | Block::Para(inl) => md::plain(inl),
        Block::Code { text, .. } => text.clone(),
        Block::List { items, .. } => items.iter().map(|i| format!("- {}", i.blocks.iter().map(plain_block).collect::<Vec<_>>().join(" "))).collect::<Vec<_>>().join("\n"),
        Block::Quote(body) | Block::Alert { body, .. } | Block::Center(body) => body.iter().map(plain_block).collect::<Vec<_>>().join("\n"),
        Block::Table { rows, .. } => rows.iter().map(|r| r.iter().map(|c| md::plain(c)).collect::<Vec<_>>().join("\t")).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

/// Returns true when the dialog should close.
pub fn integrations_dialog(ctx: &egui::Context, app: &mut InkApp) -> bool {
    let pal = app.pal;
    let mut close = false;
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let _ = (ctx, &pal);
        app.toast("Integrations are set up on the desktop app");
        close = true;
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        if !app.integ.loaded {
            let s = crate::google::status();
            app.integ.client_id = s["clientId"].as_str().unwrap_or("").to_string();
            app.integ.connected = s["connected"].as_bool().unwrap_or(false);
            app.integ.loaded = true;
        }
        let mut connect = false;
        let mut disconnect = false;
        let modal = egui::Modal::new(egui::Id::new("dlg-integrations")).frame(egui::Frame::popup(&ctx.global_style()).fill(pal.elev).corner_radius(12).inner_margin(20));
        let r = modal.show(ctx, |ui| {
            ui.set_width(480.0f32.min(ui.ctx().content_rect().width() - 60.0));
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Integrations").size(16.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(icons::X).frame_when_inactive(false)).clicked() {
                        close = true;
                    }
                });
            });
            ui.add_space(10.0);
            egui::Frame::new().fill(pal.chrome).corner_radius(10).inner_margin(14).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(icons::CLOUD_UPLOAD).size(20.0).color(pal.accent));
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Google Drive").strong());
                        ui.label(egui::RichText::new("Export > Google Docs uploads the document to your own Drive as a Google Doc. Inkdown can only see files it created.").size(12.0).color(pal.muted));
                    });
                });
                ui.add_space(8.0);
                if app.integ.connected {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("{}  Connected", icons::CIRCLE_CHECK)).color(pal.tip));
                        if ui.button("Disconnect").clicked() {
                            disconnect = true;
                        }
                    });
                } else {
                    ui.label(egui::RichText::new("Sign-in uses your own Google OAuth client (type \"Desktop app\", Drive API enabled).").size(12.0).color(pal.muted));
                    ui.add_space(4.0);
                    ui.add(egui::TextEdit::singleline(&mut app.integ.client_id).hint_text("Client ID  (…apps.googleusercontent.com)").desired_width(f32::INFINITY));
                    ui.add(egui::TextEdit::singleline(&mut app.integ.secret).hint_text("Client secret").password(true).desired_width(f32::INFINITY));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let ready = !app.integ.client_id.trim().is_empty() && !app.integ.busy;
                        if ui.add_enabled(ready, egui::Button::new(egui::RichText::new(if app.integ.busy { "Waiting for the browser…" } else { "Connect Google account" }).color(pal.on_accent())).fill(pal.accent).min_size(egui::vec2(0.0, 30.0))).clicked() {
                            connect = true;
                        }
                        if ui.link("How to create a client").clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab("https://developers.google.com/workspace/guides/create-credentials#desktop-app"));
                        }
                    });
                }
                if let Some(e) = &app.integ.error {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(e).color(pal.danger).size(12.0));
                }
            });
        });
        if r.should_close() {
            close = true;
        }
        if connect {
            app.integ.error = None;
            let (id, secret) = (app.integ.client_id.clone(), app.integ.secret.clone());
            background(app, ctx, move || Done::Connected(crate::google::connect(&id, &secret).map(|_| ())));
        }
        if disconnect {
            match crate::google::disconnect() {
                Ok(()) => {
                    app.integ.connected = false;
                    app.toast("Google account disconnected");
                }
                Err(e) => app.integ.error = Some(e),
            }
        }
    }
    close
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_word_and_html() {
        let md = "# Report\n\nText with $a^2$ and **bold**.\n\n```mermaid\nflowchart LR\n  A --> B\n```\n\n| x | y |\n|---|---|\n| 1 | 2 |\n\n$$\n\\frac{1}{2}\n$$\n";
        let a = collect_assets(md, None);
        assert_eq!(a.diagrams.len(), 1, "diagram rendered for Word");
        assert!(a.math.contains_key("0|a^2") && a.math.contains_key("1|\\frac{1}{2}"));
        assert!(a.math["0|a^2"].data.starts_with(b"\x89PNG"));
        let bytes = docx_bytes(md, "Report", None).unwrap();
        assert!(bytes.starts_with(b"PK") && bytes.len() > 4000);
        // the package holds the document part and the embedded pictures
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let names: Vec<String> = (0..zip.len()).map(|i| zip.by_index(i).unwrap().name().map(|n| n.to_string()).unwrap_or_default()).collect();
        assert!(names.iter().any(|n| n == "word/document.xml"));
        assert!(names.iter().filter(|n| n.starts_with("word/media/")).count() >= 3, "{names:?}");
        let doc = docx::build(md, &a, "Report");
        assert!(String::from_utf8_lossy(&docx::write_rtf(&doc)).starts_with("{\\rtf1"));
        let (page, title) = html_page(md, "file", None, false);
        assert_eq!(title, "Report");
        assert!(page.contains("<table>") && page.contains("data:image/svg+xml"));
    }
}
