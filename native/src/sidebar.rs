//! Left panel: the folder tree and the document outline. Also owns the background
//! file index used by Goto Anything and Find in Files.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

use egui::{vec2, Id, Sense, Ui};

use crate::app::{Dialog, InkApp};
use crate::doc::{Kind, Mode};
use crate::icons;
use crate::theme;

#[derive(Clone, Copy, PartialEq, Default)]
pub enum Tab {
    #[default]
    Files,
    Outline,
}

#[derive(Default)]
pub struct Sidebar {
    pub folder: Option<PathBuf>,
    expanded: HashSet<PathBuf>,
    cache: HashMap<PathBuf, Vec<(PathBuf, bool)>>,
    pub tab: Tab,
    index: Option<Arc<Vec<PathBuf>>>,
    index_rx: Option<Receiver<Vec<PathBuf>>>,
    symbols: (u64, u64, Vec<(usize, String)>),
}

const SKIP_DIRS: &[&str] = &[".git", "node_modules", "target", ".svn", ".hg", "__pycache__", ".idea", ".vs", "dist", "build", ".gradle", ".next", ".venv", "venv"];

fn walk(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let walker = ignore::WalkBuilder::new(root).hidden(false).git_ignore(true).git_exclude(true).parents(true).filter_entry(|e| !(e.file_type().is_some_and(|t| t.is_dir()) && e.file_name() == ".git")).build();
        for entry in walker.flatten() {
            if entry.file_type().is_some_and(|t| t.is_file()) {
                out.push(entry.into_path());
                if out.len() >= 300_000 {
                    break;
                }
            }
        }
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                let name = e.file_name().to_string_lossy().to_string();
                if p.is_dir() {
                    if !SKIP_DIRS.contains(&name.as_str()) {
                        stack.push(p);
                    }
                } else {
                    out.push(p);
                }
            }
            if out.len() >= 100_000 {
                break;
            }
        }
    }
    out.sort();
    out
}

impl Sidebar {
    pub fn set_folder(&mut self, folder: Option<PathBuf>) {
        self.cache.clear();
        self.expanded.clear();
        self.index = None;
        self.index_rx = None;
        if let Some(f) = &folder {
            self.expanded.insert(f.clone());
            let (tx, rx) = channel();
            let root = f.clone();
            let _ = std::thread::Builder::new().name("ink-index".into()).spawn(move || {
                let _ = tx.send(walk(&root));
            });
            self.index_rx = Some(rx);
        }
        self.folder = folder;
        self.tab = Tab::Files;
    }

    pub fn poll(&mut self) {
        if let Some(rx) = &self.index_rx {
            if let Ok(list) = rx.try_recv() {
                self.index = Some(Arc::new(list));
                self.index_rx = None;
            }
        }
    }

    pub fn indexing(&self) -> bool {
        self.index_rx.is_some()
    }

    pub fn index(&self) -> Option<Arc<Vec<PathBuf>>> {
        self.index.clone()
    }

    pub fn refresh(&mut self) {
        self.cache.clear();
        let f = self.folder.clone();
        let expanded = std::mem::take(&mut self.expanded);
        self.set_folder(f);
        self.expanded = expanded;
    }

    fn children(&mut self, dir: &Path) -> &[(PathBuf, bool)] {
        if !self.cache.contains_key(dir) {
            let mut list: Vec<(PathBuf, bool)> = std::fs::read_dir(dir)
                .map(|rd| rd.flatten().map(|e| (e.path(), e.file_type().map(|t| t.is_dir()).unwrap_or(false))).filter(|(p, is_dir)| !(*is_dir && p.file_name().is_some_and(|n| n == ".git"))).collect())
                .unwrap_or_default();
            list.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.file_name().map(|n| n.to_string_lossy().to_lowercase()).cmp(&b.0.file_name().map(|n| n.to_string_lossy().to_lowercase()))));
            self.cache.insert(dir.to_path_buf(), list);
        }
        &self.cache[dir]
    }

    fn collect(&mut self, dir: &Path, depth: usize, out: &mut Vec<(PathBuf, bool, usize)>) {
        let kids: Vec<(PathBuf, bool)> = self.children(dir).to_vec();
        for (p, is_dir) in kids {
            out.push((p.clone(), is_dir, depth));
            if is_dir && out.len() < 50_000 && self.expanded.contains(&p) {
                self.collect(&p, depth + 1, out);
            }
        }
    }

    fn rows(&mut self) -> Vec<(PathBuf, bool, usize)> {
        let mut out = Vec::new();
        if let Some(root) = self.folder.clone() {
            self.collect(&root, 0, &mut out);
        }
        out
    }
}
pub fn file_icon(name: &str, is_dir: bool, open: bool) -> &'static str {
    if is_dir {
        return if open { icons::FOLDER_OPEN } else { icons::FOLDER };
    }
    match Kind::of(name, "") {
        Kind::Markdown | Kind::Asciidoc => icons::FILE_TEXT,
        Kind::Data(_) => icons::FILE_JSON,
        Kind::Table(_) => icons::FILE_SPREADSHEET,
        Kind::Image | Kind::Svg => icons::FILE_IMAGE,
        Kind::Pdf => icons::FILE_TEXT,
        Kind::Mermaid | Kind::Dot => icons::WORKFLOW,
        Kind::Code => {
            let l = name.to_lowercase();
            if l.ends_with(".txt") || l.ends_with(".log") || !l.contains('.') { icons::FILE } else { icons::FILE_CODE }
        }
    }
}

fn contents(ui: &mut Ui, app: &mut InkApp) {
    let pal = app.pal;
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.spacing_mut().item_spacing.x = 2.0;
        for (tab, label) in [(Tab::Files, "Files"), (Tab::Outline, "Outline")] {
            let on = app.sidebar.tab == tab;
            let b = egui::Button::new(egui::RichText::new(label).size(12.5).color(if on { pal.text } else { pal.muted })).fill(if on { pal.active() } else { egui::Color32::TRANSPARENT }).corner_radius(6);
            if ui.add(b).clicked() {
                app.sidebar.tab = tab;
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(6.0);
            if app.sidebar.tab == Tab::Files {
                if ui.add(egui::Button::new(egui::RichText::new(icons::REFRESH_CW).size(13.0).color(pal.muted)).frame_when_inactive(false)).on_hover_text("Refresh").clicked() {
                    app.sidebar.refresh();
                }
                if ui.add(egui::Button::new(egui::RichText::new(icons::FOLDER_OPEN).size(13.0).color(pal.muted)).frame_when_inactive(false)).on_hover_text("Open folder").clicked() {
                    app.pending.push("open_folder".into());
                }
            }
        });
    });
    ui.add_space(6.0);
    match app.sidebar.tab {
        Tab::Files => files(ui, app),
        Tab::Outline => outline(ui, app),
    }
}

fn files(ui: &mut Ui, app: &mut InkApp) {
    let pal = app.pal;
    let Some(root) = app.sidebar.folder.clone() else {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new(icons::FOLDER_OPEN).size(26.0).color(pal.faint));
            ui.add_space(6.0);
            ui.label(egui::RichText::new("No folder open").color(pal.muted).size(13.0));
            ui.add_space(8.0);
            if ui.button("Open Folder…").clicked() {
                app.pending.push("open_folder".into());
            }
        });
        // open tabs stay reachable
        return;
    };
    let name = root.file_name().map(|n| n.to_string_lossy().to_uppercase()).unwrap_or_else(|| root.to_string_lossy().to_string());
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(egui::RichText::new(name).size(11.0).color(pal.faint).strong());
    });
    ui.add_space(4.0);
    let rows = app.sidebar.rows();
    let active = app.docs.get(app.active).and_then(|d| d.path.clone());
    let row_h = 26.0;
    let mut open = None;
    let mut toggle = None;
    egui::ScrollArea::vertical().id_salt("side-files").auto_shrink([false, false]).show_rows(ui, row_h, rows.len(), |ui, range| {
        for (path, is_dir, depth) in &rows[range] {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
            let is_active = active.as_ref() == Some(path);
            if is_active || resp.hovered() {
                ui.painter().rect_filled(rect.shrink2(vec2(6.0, 1.0)), 6, if is_active { pal.accent_soft() } else { pal.hover() });
            }
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let x = rect.min.x + 14.0 + *depth as f32 * 14.0;
            let expanded = app.sidebar.expanded.contains(path);
            let icon = theme::text(ui, file_icon(&name, *is_dir, expanded), 13.5, if *is_dir { pal.accent } else { pal.muted }, 400.0);
            ui.painter().galley(egui::pos2(x, rect.center().y - icon.size().y / 2.0), icon, pal.muted);
            let mut job = egui::text::LayoutJob::single_section(name, theme::fmt(13.0, if is_active { pal.text } else { theme::mix(pal.text, pal.muted, 0.35) }, if is_active { 600.0 } else { 430.0 }));
            job.wrap = egui::text::TextWrapping { max_width: (rect.max.x - x - 30.0).max(20.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
            let g = ui.fonts_mut(|f| f.layout_job(job));
            ui.painter().galley(egui::pos2(x + 22.0, rect.center().y - g.size().y / 2.0), g, pal.text);
            if resp.clicked() {
                if *is_dir {
                    toggle = Some(path.clone());
                } else {
                    open = Some(path.clone());
                }
            }
            let (p, d) = (path.clone(), *is_dir);
            resp.context_menu(|ui| {
                ui.set_min_width(190.0);
                let dir = if d { p.clone() } else { p.parent().map(Path::to_path_buf).unwrap_or_default() };
                if ui.button(format!("{}  New File…", icons::FILE_PLUS)).clicked() {
                    app.dialog = Some(Dialog::NewFile(dir.clone(), String::new(), false));
                    ui.close();
                }
                if ui.button(format!("{}  New Folder…", icons::FOLDER_PLUS)).clicked() {
                    app.dialog = Some(Dialog::NewFile(dir.clone(), String::new(), true));
                    ui.close();
                }
                ui.separator();
                if ui.button(format!("{}  Rename…", icons::PENCIL)).clicked() {
                    app.dialog = Some(Dialog::Rename(p.clone(), p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
                    ui.close();
                }
                if ui.button(format!("{}  Delete…", icons::TRASH_2)).clicked() {
                    app.dialog = Some(Dialog::ConfirmDelete(p.clone()));
                    ui.close();
                }
                ui.separator();
                if ui.button(format!("{}  Copy Path", icons::COPY)).clicked() {
                    ui.ctx().copy_text(p.to_string_lossy().to_string());
                    ui.close();
                }
                if ui.button(format!("{}  Show in File Manager", icons::EXTERNAL_LINK)).clicked() {
                    crate::platform::reveal(&p);
                    ui.close();
                }
            });
        }
    });
    if let Some(p) = toggle {
        if !app.sidebar.expanded.remove(&p) {
            app.sidebar.expanded.insert(p);
        }
    }
    if let Some(p) = open {
        app.open_path(&p);
    }
}

fn outline(ui: &mut Ui, app: &mut InkApp) {
    let pal = app.pal;
    let Some(d) = app.docs.get(app.active) else { return };
    let rows: Vec<(usize, String, usize)> = if matches!(d.kind, Kind::Markdown | Kind::Asciidoc) {
        // parse on demand when the reader is not on screen
        if d.mode == Mode::Code || d.mode == Mode::Write {
            let key = (d.id, d.buf.version);
            if (app.sidebar.symbols.0, app.sidebar.symbols.1) != key {
                let text = d.buf.text();
                let text = if d.kind == Kind::Asciidoc { crate::adoc::to_markdown(&text) } else { text };
                let tops = crate::md::parse(&text);
                app.sidebar.symbols = (key.0, key.1, crate::md::outline(&tops).into_iter().map(|(l, t, _, line)| (line.saturating_sub(1), format!("{}{t}", "\u{1}".repeat(l as usize)))).collect());
            }
            app.sidebar.symbols.2.iter().map(|(line, t)| (t.matches('\u{1}').count(), t.trim_start_matches('\u{1}').to_string(), *line)).collect()
        } else {
            d.view.md.outline.iter().map(|(l, t, _, line)| (*l as usize, t.clone(), line.saturating_sub(1))).collect()
        }
    } else {
        let key = (d.id, d.buf.version);
        if (app.sidebar.symbols.0, app.sidebar.symbols.1) != key {
            app.sidebar.symbols = (key.0, key.1, d.hl.symbols(&d.buf, std::time::Duration::from_millis(250)));
        }
        app.sidebar.symbols.2.iter().map(|(line, t)| (1, t.clone(), *line)).collect()
    };
    if rows.is_empty() {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| ui.label(egui::RichText::new("No headings or symbols").color(pal.muted).size(13.0)));
        return;
    }
    let min_level = rows.iter().map(|r| r.0).min().unwrap_or(1);
    let mut jump = None;
    let row_h = 26.0;
    egui::ScrollArea::vertical().id_salt("side-outline").auto_shrink([false, false]).show_rows(ui, row_h, rows.len(), |ui, range| {
        for (level, text, line) in &rows[range] {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(rect.shrink2(vec2(6.0, 1.0)), 6, pal.hover());
            }
            let depth = level - min_level;
            let x = rect.min.x + 16.0 + depth as f32 * 13.0;
            let mut job = egui::text::LayoutJob::single_section(text.clone(), theme::fmt(13.0, if depth == 0 { pal.text } else { pal.muted }, if depth == 0 { 600.0 } else { 430.0 }));
            job.wrap = egui::text::TextWrapping { max_width: (rect.max.x - x - 10.0).max(20.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
            let g = ui.fonts_mut(|f| f.layout_job(job));
            ui.painter().galley(egui::pos2(x, rect.center().y - g.size().y / 2.0), g, pal.text);
            if resp.clicked() {
                jump = Some(*line);
            }
        }
    });
    if let (Some(line), Some(d)) = (jump, app.docs.get_mut(app.active)) {
        d.ed.goto(&mut d.buf, line, 0);
        d.view.md.goto_line = Some(line + 1);
        if let Some(r) = &mut d.view.rich {
            r.goto_line = Some(line);
        }
    }
}

pub fn show(ui: &mut Ui, app: &mut InkApp) {
    let pal = app.pal;
    egui::Panel::left("sidebar").resizable(true).default_size(250.0).size_range(170.0..=520.0).frame(egui::Frame::new().fill(pal.chrome)).show(ui, |ui| contents(ui, app));
}

/// On narrow windows the sidebar slides over the content instead of squeezing it.
pub fn drawer(ctx: &egui::Context, app: &mut InkApp) {
    let pal = app.pal;
    let screen = ctx.content_rect();
    let w = (screen.width() * 0.82).min(320.0);
    ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, Id::new("drawer-dim"))).rect_filled(screen, 0, egui::Color32::from_black_alpha(90));
    // stay clear of the system bars on a phone
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let (top, bottom) = {
        let (t, b) = crate::mobile::insets_px();
        (t / ctx.pixels_per_point(), b / ctx.pixels_per_point())
    };
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let (top, bottom) = (0.0, 0.0);
    let resp = egui::Area::new(Id::new("ink-drawer")).order(egui::Order::Foreground).fixed_pos(screen.min + egui::vec2(0.0, top)).show(ctx, |ui| {
        egui::Frame::new().fill(pal.chrome).show(ui, |ui| {
            ui.set_width(w);
            ui.set_height(screen.height() - top - bottom);
            contents(ui, app);
        });
    });
    let dismissed = ctx.input(|i| (i.pointer.any_pressed() && !resp.response.contains_pointer()) || i.key_pressed(egui::Key::Escape));
    if dismissed && app.overlay_opened != app.frame_count {
        app.set.show_sidebar = false;
    }
}
