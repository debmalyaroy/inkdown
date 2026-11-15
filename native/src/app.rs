//! Application state, the frame loop and the command dispatcher.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use egui::{vec2, Id, Ui};

use crate::assets::Assets;
use crate::automation::Automation;
use crate::buffer::{Edit, EditKind};
use crate::doc::{Doc, Kind, Mode};
use crate::editor::{self, EdStyle};
use crate::keymap::{Combo, Keymap, Match, Scope};
use crate::picker::{PickKind, Picker};
use crate::preview::{self, Action, CanvasSource, ViewCtx};
use crate::settings::{Session, SessionTab, Settings};
use crate::theme::{self, Palette};

pub const WELCOME: &str = include_str!("../assets/welcome.md");

#[derive(Clone)]
pub enum MacroStep {
    Command(String),
    Text(String),
}

#[derive(Debug)]
pub enum Dialog {
    /// Unsaved changes in the tab at this index.
    ConfirmClose(usize),
    Shortcuts,
    About,
    /// Update check result, download progress and restart prompt.
    Update,
    Integrations,
    Message(String, String),
    ConfirmDelete(PathBuf),
    Rename(PathBuf, String),
    NewFile(PathBuf, String, bool),
    ReloadChanged(usize),
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Bottom {
    None,
    Search,
    Build,
}

pub struct InkApp {
    pub docs: Vec<Doc>,
    pub active: usize,
    pub set: Settings,
    saved_set: Settings,
    pub pal: Palette,
    pub pal_epoch: u64,
    pub system_dark: bool,
    pub keymap: Keymap,
    pub assets: Assets,
    pub picker: Option<Picker>,
    pub toasts: Vec<(String, f64)>,
    pub dialog: Option<Dialog>,
    pub sidebar: crate::sidebar::Sidebar,
    pub closed: Vec<PathBuf>,
    pub macro_rec: Option<Vec<MacroStep>>,
    pub macro_saved: Vec<MacroStep>,
    pub bottom: Bottom,
    pub search: crate::project::Search,
    pub build: crate::project::Build,
    pub lightbox: Option<(u64, preview::Canvas)>,
    pub integ: crate::export_ui::Integ,
    /// Two-column layout: the documents shown left and right.
    pub split: Option<[u64; 2]>,
    pub focus_pane: u8,
    last_active: u64,
    pub automation: Option<Automation>,
    pub untitled: usize,
    pub pending: Vec<String>,
    title: String,
    snippets: HashMap<String, Rc<Vec<(String, String)>>>,
    user_snippets: serde_json::Value,
    split_ratio: f32,
    last_disk_check: f64,
    pub fullscreen: bool,
    last_sync: (u64, usize, f32),
    pub frame_ms: f32,
    pub frame_max: f32,
    pub frame_sum: f32,
    pub frame_count: u32,
    /// What was happening when a frame took long (shown in test dumps).
    pub slow: Vec<String>,
    pub last_cmd: String,
    started: std::time::Instant,
    /// Last text-input request and how many frames ago it was made (see `ui`).
    ime_keep: Option<(egui::output::IMEOutput, u8)>,
    pub first_frame_ms: Option<f32>,
    pub quit: bool,
    pub updater: crate::update::Updater,
    /// Sum of whole-frame times (build, tessellate, paint) reported by the windowing layer.
    pub total_sum: f32,
    /// Times the glyph atlas was thrown away and every cached layout rebuilt.
    pub atlas_resets: u32,
    /// Document waiting for the answer of the phone's "save as" picker.
    pending_save: Option<u64>,
    /// When the phone glue was last asked for arrived documents.
    mobile_polled: f64,
    /// Input events to deliver at the start of the next frame (paste on phones).
    inject: Vec<egui::Event>,
    /// Glyph atlas state last frame; cached layouts must be rebuilt when egui resets the atlas.
    atlas: (f32, Option<egui::epaint::text::TextOptions>, f32),
}

impl InkApp {
    pub fn new(cc: &eframe::CreationContext<'_>, files: Vec<PathBuf>) -> Self {
        let started = std::time::Instant::now();
        let ctx = &cc.egui_ctx;
        theme::install_fonts(ctx);
        let set = Settings::load();
        let system_dark = ctx.system_theme().map(|t| t == egui::Theme::Dark).unwrap_or(false);
        let pal = theme::palette(&set.theme, system_dark);
        theme::apply(ctx, &pal);
        if (set.ui_scale - 1.0).abs() > 0.01 {
            ctx.set_zoom_factor(set.ui_scale.clamp(0.6, 2.5));
        }
        let user_snippets = std::fs::read_to_string(crate::settings::snippets_path()).ok().and_then(|t| serde_json::from_str(&crate::settings::strip_json_comments(&t)).ok()).unwrap_or(serde_json::Value::Null);
        let mut app = Self {
            docs: Vec::new(),
            active: 0,
            saved_set: set.clone(),
            set,
            pal,
            pal_epoch: 1,
            system_dark,
            keymap: Keymap::load(),
            assets: Assets::new(ctx),
            picker: None,
            toasts: Vec::new(),
            dialog: None,
            sidebar: Default::default(),
            closed: Vec::new(),
            macro_rec: None,
            macro_saved: Vec::new(),
            bottom: Bottom::None,
            search: Default::default(),
            build: Default::default(),
            lightbox: None,
            integ: Default::default(),
            split: None,
            focus_pane: 0,
            last_active: 0,
            automation: Automation::from_env(),
            untitled: 0,
            pending: Vec::new(),
            title: String::new(),
            snippets: HashMap::new(),
            user_snippets,
            split_ratio: 0.5,
            last_disk_check: 0.0,
            fullscreen: false,
            last_sync: (0, 0, 0.0),
            frame_ms: 0.0,
            frame_max: 0.0,
            frame_sum: 0.0,
            frame_count: 0,
            slow: Vec::new(),
            last_cmd: String::new(),
            started,
            ime_keep: None,
            updater: Default::default(),
            inject: Vec::new(),
            pending_save: None,
            mobile_polled: 0.0,
            total_sum: 0.0,
            atlas_resets: 0,
            first_frame_ms: None,
            quit: false,
            atlas: (0.0, None, 0.0),
        };
        if app.set.hot_exit && std::env::var_os("INKDOWN_NO_SESSION").is_none() {
            app.restore_session();
        }
        for f in files {
            app.open_path(&f);
        }
        if app.docs.is_empty() {
            app.open_welcome();
        }
        app
    }

    pub fn open_welcome(&mut self) {
        let mut d = Doc::new(None, "Welcome.md".into(), WELCOME, &self.set);
        d.mode = Mode::Read;
        d.scratch = true;
        self.docs.push(d);
        self.active = self.docs.len() - 1;
    }

    pub fn toast(&mut self, msg: impl Into<String>) {
        self.toasts.push((msg.into(), -1.0));
    }

    pub fn doc(&mut self) -> Option<&mut Doc> {
        self.docs.get_mut(self.active)
    }

    pub fn open_path(&mut self, path: &Path) -> bool {
        let path = std::fs::canonicalize(path).map(strip_unc).unwrap_or_else(|_| path.to_path_buf());
        if path.is_dir() {
            self.sidebar.set_folder(Some(path.clone()));
            // on phones the folder lives in a drawer that starts closed
            self.set.show_sidebar = !cfg!(any(target_os = "android", target_os = "ios"));
            let p = path.to_string_lossy().to_string();
            self.set.recent_folders.retain(|x| x != &p);
            self.set.recent_folders.insert(0, p);
            self.set.recent_folders.truncate(12);
            return true;
        }
        if let Some(i) = self.docs.iter().position(|d| d.path.as_deref() == Some(path.as_path())) {
            self.active = i;
            return true;
        }
        match Doc::open(&path, &self.set) {
            Ok(d) => {
                // a pristine welcome tab gives way to the first real file
                if self.docs.len() == 1 && self.docs[0].scratch && self.docs[0].name == "Welcome.md" {
                    self.docs.clear();
                }
                self.set.add_recent(&path);
                self.docs.push(d);
                self.active = self.docs.len() - 1;
                true
            }
            Err(e) => {
                self.toast(e);
                false
            }
        }
    }

    pub fn open_at(&mut self, path: &Path, line: usize, col: usize) {
        if self.open_path(path) {
            if let Some(d) = self.doc() {
                if d.mode == Mode::Read || d.mode == Mode::Write {
                    d.mode = if d.kind == Kind::Code { Mode::Code } else { Mode::Split };
                }
                d.ed.goto(&mut d.buf, line, col);
                d.ed.focus = true;
            }
        }
    }

    pub fn new_file(&mut self, name: Option<String>, text: &str) {
        self.untitled += 1;
        let name = name.unwrap_or_else(|| format!("Untitled-{}.md", self.untitled));
        let mut d = Doc::new(None, name, text, &self.set);
        if d.kind == Kind::Markdown {
            d.mode = if text.is_empty() { Mode::Write } else { Mode::Split };
        }
        if !text.is_empty() {
            // templates start out unsaved
            d.buf.set_text(text);
        }
        self.docs.push(d);
        self.active = self.docs.len() - 1;
    }

    pub fn snippets_for(&mut self, doc_ix: usize) -> Rc<Vec<(String, String)>> {
        let Some(d) = self.docs.get(doc_ix) else { return Rc::new(Vec::new()) };
        let ext = d.name.rsplit('.').next().unwrap_or("").to_lowercase();
        let key = format!("{ext}|{}", d.hl.name());
        if let Some(s) = self.snippets.get(&key) {
            return s.clone();
        }
        let list = Rc::new(crate::snippets::for_language(&ext, d.hl.name(), &self.user_snippets));
        self.snippets.insert(key, list.clone());
        list
    }

    /// Phones: documents picked in the system file picker or sent by another app, and the
    /// answer of the "save as" picker.
    #[cfg(any(target_os = "android", target_os = "ios"))]
    fn poll_mobile(&mut self, ctx: &egui::Context, now: f64) {
        if now - self.mobile_polled < 0.25 {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
            return;
        }
        self.mobile_polled = now;
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        while let Some(path) = crate::mobile::take_opened() {
            self.open_path(&path);
        }
        let Some(id) = self.pending_save else { return };
        let Some(answer) = crate::mobile::take_saved() else { return };
        self.pending_save = None;
        let Some(place) = answer else { return };
        let Some(ix) = self.docs.iter().position(|d| d.id == id) else { return };
        // keep a working copy in Inkdown's own folder, tied to the chosen document
        if self.docs[ix].path.is_none() {
            let dir = crate::mobile::documents_dir();
            let _ = std::fs::create_dir_all(&dir);
            let name = self.docs[ix].name.clone();
            let set = self.set.clone();
            self.docs[ix].set_path(dir.join(name), &set);
            self.docs[ix].scratch = false;
        }
        let set = self.set.clone();
        let text = self.docs[ix].text_for_disk(&set);
        let path = self.docs[ix].path.clone().unwrap_or_default();
        match crate::mobile::save_to(&place, &path, text.as_bytes()) {
            Ok(name) => {
                // the working copy follows; from now on Save writes to both
                let _ = self.docs[ix].save(&set);
                self.toast(format!("Saved as {name}"));
            }
            Err(e) => self.toast(e),
        }
    }

    // ------------------------------------------------------------ saving

    pub fn save_doc(&mut self, ix: usize, force_dialog: bool) -> bool {
        let Some(d) = self.docs.get_mut(ix) else { return false };
        if d.read_only {
            self.toast("This file is read-only.");
            return false;
        }
        let mut named = false;
        // phones: "Save As" goes through the system picker; the answer arrives in `tick`
        #[cfg(any(target_os = "android", target_os = "ios"))]
        if force_dialog && crate::mobile::pick_save(&d.name) {
            self.pending_save = Some(d.id);
            return false;
        }
        if d.path.is_none() || force_dialog {
            let Some(p) = crate::platform::save_dialog(&d.name, d.dir().or_else(|| self.sidebar.folder.clone())) else { return false };
            d.set_path(p, &self.set);
            d.scratch = false;
            named = true;
        }
        let set = self.set.clone();
        let d = &mut self.docs[ix];
        match d.save(&set) {
            Ok(()) => {
                if let Some(p) = d.path.clone() {
                    self.set.add_recent(&p);
                    self.after_save(&p);
                    // a phone has no save dialog, so say where the file went
                    if cfg!(any(target_os = "android", target_os = "ios")) {
                        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                        self.toast(if named { format!("Saved in Inkdown's documents as {name}") } else { format!("Saved {name}") });
                    }
                }
                true
            }
            Err(e) => {
                self.toast(e);
                false
            }
        }
    }

    /// Saving one of our own config files applies it immediately.
    fn after_save(&mut self, path: &Path) {
        if path == crate::settings::settings_path() {
            if let Ok(text) = std::fs::read_to_string(path) {
                match Settings::parse(&text) {
                    Ok(s) => {
                        self.set = s;
                        self.saved_set = self.set.clone();
                        self.apply_settings();
                        self.toast("Settings applied");
                    }
                    Err(e) => self.toast(format!("Settings not applied: {e}")),
                }
            }
        } else if path == crate::settings::keymap_path() {
            self.keymap = Keymap::load();
            self.toast("Key bindings applied");
        } else if path == crate::settings::snippets_path() {
            self.user_snippets = std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&crate::settings::strip_json_comments(&t)).ok()).unwrap_or(serde_json::Value::Null);
            self.snippets.clear();
            self.toast("Snippets applied");
        }
    }

    pub fn apply_settings(&mut self) {
        self.pal = theme::palette(&self.set.theme, self.system_dark);
        self.pal_epoch += 1;
        let set = self.set.clone();
        for d in &mut self.docs {
            let sample: String = d.buf.rope.slice(..d.buf.len().min(20_000)).to_string();
            d.apply_settings(&set, &sample);
        }
    }

    pub fn set_theme(&mut self, id: &str) {
        self.set.theme = id.to_string();
        self.pal = theme::palette(id, self.system_dark);
        self.pal_epoch += 1;
    }

    fn close_now(&mut self, ix: usize) {
        if ix >= self.docs.len() {
            return;
        }
        let d = self.docs.remove(ix);
        if let Some(p) = d.path {
            self.closed.push(p);
        }
        if self.active > ix || self.active >= self.docs.len() {
            self.active = self.active.saturating_sub(1);
        }
        if self.docs.is_empty() {
            self.active = 0;
        }
    }

    pub fn close_tab(&mut self, ix: usize) {
        match self.docs.get(ix) {
            Some(d) if d.dirty() => {
                self.active = ix;
                self.dialog = Some(Dialog::ConfirmClose(ix));
            }
            Some(_) => self.close_now(ix),
            None => {}
        }
    }

    // ----------------------------------------------------------- session

    fn restore_session(&mut self) {
        let s = Session::load();
        if let Some(f) = s.folder.as_deref().map(PathBuf::from).filter(|p| p.is_dir()) {
            self.sidebar.set_folder(Some(f));
        }
        for t in &s.tabs {
            let before = self.docs.len();
            match &t.path {
                Some(p) if Path::new(p).is_file() => {
                    self.open_path(Path::new(p));
                }
                Some(_) => continue,
                None => {
                    let Some(text) = &t.unsaved else { continue };
                    let d = Doc::new(None, t.name.clone(), "", &self.set);
                    self.docs.push(d);
                    self.docs.last_mut().unwrap().buf.set_text(text);
                }
            }
            if self.docs.len() > before {
                let set = self.set.clone();
                let d = self.docs.last_mut().unwrap();
                if let (Some(text), Some(_)) = (&t.unsaved, &t.path) {
                    d.buf.set_text(text);
                }
                if let Some(m) = Mode::from_id(&t.mode).filter(|m| d.kind.modes().contains(m)) {
                    d.mode = m;
                }
                let _ = set;
                d.buf.set_caret(t.caret.min(d.buf.len()));
                d.ed.reveal = true;
                d.ed.center = true;
            }
        }
        self.untitled = self.docs.iter().filter(|d| d.path.is_none()).count();
        if !self.docs.is_empty() {
            self.active = s.active.min(self.docs.len() - 1);
        }
    }

    pub fn save_session(&mut self) {
        if std::env::var_os("INKDOWN_NO_SESSION").is_some() {
            return;
        }
        let tabs = self
            .docs
            .iter()
            .filter(|d| !d.scratch && (d.path.is_some() || d.buf.len() > 0))
            .map(|d| SessionTab {
                path: d.path.as_ref().map(|p| p.to_string_lossy().to_string()),
                name: d.name.clone(),
                unsaved: (d.dirty() || d.path.is_none()).then(|| d.buf.text()).filter(|t| t.len() < 8 << 20),
                caret: d.buf.primary().head,
                scroll: d.ed.scroll.y,
                mode: d.mode.id().to_string(),
            })
            .collect();
        Session { folder: self.sidebar.folder.as_ref().map(|p| p.to_string_lossy().to_string()), active: self.active, tabs }.save();
    }

    // ---------------------------------------------------------- commands
}

/// Caret movement, selection and searching: safe on a read-only document.
fn is_navigation(name: &str) -> bool {
    ["move_", "select", "find", "page"].iter().any(|p| name.starts_with(p))
        || matches!(name, "single_selection" | "expand_selection_to_line" | "show_at_center" | "toggle_bookmark" | "next_bookmark" | "prev_bookmark" | "clear_bookmarks" | "fold" | "unfold" | "fold_all" | "unfold_all" | "scroll_lines_up" | "scroll_lines_down" | "slurp_find_string" | "copy")
}

/// Commands that do not change the text of the active document.
fn usable_when_read_only(name: &str) -> bool {
    const PREFIXES: &[&str] = &["mode", "close", "theme", "open", "new_", "save", "export_", "print", "layout_", "focus_group_", "move_to_group_", "goto_", "show_", "select_tab_", "tool_avro_", "tool_word_count", "reopen_", "set_syntax"];
    const NAMES: &[&str] = &[
        "check_for_updates", "update_download", "update_install", "help_docs", "help_release_notes", "help_report_issue", "command_palette", "show_shortcuts", "select_theme",
        "cycle_mode", "next_tab", "prev_tab", "next_view", "prev_view", "welcome", "about", "quit", "integrations", "copy_path", "reveal_in_explorer", "revert",
        "increase_font_size", "decrease_font_size", "reset_font_size", "toggle_sidebar", "toggle_minimap", "toggle_line_numbers", "toggle_word_wrap", "toggle_status_bar",
        "toggle_fullscreen", "toggle_auto_save", "toggle_bottom_panel", "build", "build_with", "cancel_build",
    ];
    is_navigation(name) || PREFIXES.iter().any(|p| name.starts_with(p)) || NAMES.contains(&name)
}

impl InkApp {

    fn editor_command(&mut self, name: &str, now: f64) -> bool {
        let snippets = self.snippets_for(self.active);
        let Some(d) = self.docs.get_mut(self.active) else { return false };
        if d.read_only && !usable_when_read_only(name) {
            // an editing command: swallow it, the document cannot change
            return true;
        }
        if d.read_only && !is_navigation(name) {
            // an application command (close, mode, theme, export, ...): not for the editor
            return false;
        }
        match d.ed.command(name, &mut d.buf, &d.cfg, &snippets, now) {
            Some(changed) => {
                if changed {
                    d.edited_at = Some(now);
                }
                true
            }
            None => false,
        }
    }

    pub fn run(&mut self, ctx: &egui::Context, cmd: &str) {
        let now = ctx.input(|i| i.time);
        self.last_cmd = cmd.to_string();
        if let Some(rec) = &mut self.macro_rec {
            if !matches!(cmd, "toggle_record_macro" | "run_macro") {
                rec.push(MacroStep::Command(cmd.to_string()));
            }
        }
        let (name, arg) = cmd.split_once(' ').map(|(a, b)| (a, b.trim())).unwrap_or((cmd, ""));
        // Markdown niceties shared by Code, Split and Write modes
        if let Some(d) = self.docs.get_mut(self.active) {
            if d.kind == Kind::Markdown && !d.read_only {
                if name == "insert_newline" && d.ed.completion.is_none() && crate::mdfmt::smart_newline(&mut d.buf) {
                    d.edited_at = Some(now);
                    d.ed.reveal = true;
                    return;
                }
                // in Write mode vertical movement follows the laid-out rows
                if d.mode == Mode::Write && matches!(name, "move_up" | "move_down" | "select_up" | "select_down" | "page_up" | "page_down" | "select_page_up" | "select_page_down") {
                    if let Some(r) = &mut d.view.rich {
                        r.nav.push(name.to_string());
                        return;
                    }
                }
            }
        }
        if self.editor_command(name, now) {
            return;
        }
        if crate::mdfmt::command(self, name, now) {
            return;
        }
        match name {
            "new_file" => self.new_file(None, ""),
            "new_code_file" => {
                self.untitled += 1;
                self.new_file(Some(format!("untitled-{}.txt", self.untitled)), "");
            }
            // a phone has no file dialog Inkdown can call: show its own documents instead
            #[cfg(any(target_os = "android", target_os = "ios"))]
            "open_file" => {
                // the system file picker; the choice arrives in `tick`
                if !crate::mobile::pick_file() {
                    self.set.show_sidebar = true;
                    self.toast("These are the documents kept in Inkdown. To open a file from elsewhere, choose \"Open with Inkdown\" in your Files app.");
                }
            }
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            "open_file" => {
                let dir = self.doc().and_then(|d| d.dir()).or_else(|| self.sidebar.folder.clone());
                for f in crate::platform::open_dialog(dir) {
                    self.open_path(&f);
                }
            }
            "open_folder" => {
                if let Some(f) = crate::platform::folder_dialog(self.sidebar.folder.clone()) {
                    self.open_path(&f);
                }
            }
            "close_folder" => self.sidebar.set_folder(None),
            "open" if !arg.is_empty() => {
                self.open_path(Path::new(arg));
            }
            "save" => {
                self.save_doc(self.active, false);
            }
            "save_as" => {
                self.save_doc(self.active, true);
            }
            "save_all" => {
                for i in 0..self.docs.len() {
                    if self.docs[i].dirty() && self.docs[i].path.is_some() {
                        self.save_doc(i, false);
                    }
                }
            }
            "close" => self.close_tab(self.active),
            "close_all" => {
                let mut i = 0;
                while i < self.docs.len() {
                    if self.docs[i].dirty() {
                        i += 1;
                    } else {
                        self.close_now(i);
                    }
                }
                if let Some(i) = self.docs.iter().position(|d| d.dirty()) {
                    self.close_tab(i);
                }
            }
            "close_others" => {
                let keep = self.docs.get(self.active).map(|d| d.id);
                let mut i = 0;
                while i < self.docs.len() {
                    if Some(self.docs[i].id) == keep || self.docs[i].dirty() {
                        i += 1;
                    } else {
                        self.close_now(i);
                    }
                }
                self.active = self.docs.iter().position(|d| Some(d.id) == keep).unwrap_or(0);
            }
            "reopen_closed_file" => {
                while let Some(p) = self.closed.pop() {
                    if p.is_file() {
                        self.open_path(&p);
                        break;
                    }
                }
            }
            "next_tab" | "prev_tab" => {
                let n = self.docs.len();
                if n > 0 {
                    self.active = if name == "next_tab" { (self.active + 1) % n } else { (self.active + n - 1) % n };
                }
            }
            "revert" => {
                let set = self.set.clone();
                if let Some(d) = self.doc() {
                    if let Err(e) = d.reload(&set) {
                        self.toast(e);
                    }
                }
            }
            "goto_anything" => self.picker = Some(Picker::new(PickKind::Files, self)),
            "command_palette" => self.picker = Some(Picker::new(PickKind::Commands, self)),
            "goto_symbol" => self.picker = Some(Picker::new(PickKind::Symbols, self)),
            "goto_line" => self.picker = Some(Picker::new(PickKind::Line, self)),
            "goto_word" => self.picker = Some(Picker::new(PickKind::Words, self)),
            "select_theme" => self.picker = Some(Picker::new(PickKind::Themes, self)),
            "set_syntax" => self.picker = Some(Picker::new(PickKind::Syntax, self)),
            "set_encoding" => self.picker = Some(Picker::new(PickKind::Encoding, self)),
            "reopen_with_encoding" => self.picker = Some(Picker::new(PickKind::ReopenEncoding, self)),
            "set_line_endings" => self.picker = Some(Picker::new(PickKind::LineEndings, self)),
            "set_indentation" => self.picker = Some(Picker::new(PickKind::Indent, self)),
            "open_recent" => self.picker = Some(Picker::new(PickKind::Recent, self)),
            "new_from_template" => self.picker = Some(Picker::new(PickKind::Templates, self)),
            "insert_snippet" => self.picker = Some(Picker::new(PickKind::Snippets, self)),
            "build_with" => self.picker = Some(Picker::new(PickKind::Builds, self)),
            "goto_definition" => crate::project::goto_definition(self),
            "show_find" | "show_replace" => {
                if let Some(d) = self.doc() {
                    if d.mode == Mode::Read || d.mode == Mode::Write {
                        if d.kind == Kind::Image {
                            return;
                        }
                        d.mode = if d.kind.modes().contains(&Mode::Split) { Mode::Split } else { Mode::Code };
                    }
                    let s = d.buf.primary();
                    if !s.is_empty() && d.buf.line_of(s.start()) == d.buf.line_of(s.end()) {
                        d.ed.find.query = d.buf.slice(s.start(), s.end());
                    }
                    d.ed.find.open = true;
                    d.ed.find.replace_open = name == "show_replace" || d.ed.find.replace_open && name != "show_find";
                    if name == "show_find" {
                        d.ed.find.replace_open = false;
                    }
                    d.ed.find.focus = true;
                }
            }
            "toggle_case_sensitive" | "toggle_regex" | "toggle_whole_word" => {
                if let Some(d) = self.doc() {
                    let f = &mut d.ed.find;
                    match name {
                        "toggle_case_sensitive" => f.case = !f.case,
                        "toggle_regex" => f.regex = !f.regex,
                        _ => f.word = !f.word,
                    }
                }
            }
            "find_in_files" => {
                self.bottom = Bottom::Search;
                self.search.focus = true;
                if let Some(d) = self.doc() {
                    let s = d.buf.primary();
                    if !s.is_empty() && d.buf.line_of(s.start()) == d.buf.line_of(s.end()) {
                        let q = d.buf.slice(s.start(), s.end());
                        self.search.query = q;
                    }
                }
            }
            "toggle_sidebar" => self.set.show_sidebar = !self.set.show_sidebar,
            "toggle_minimap" => self.set.minimap = !self.set.minimap,
            "toggle_line_numbers" => self.set.line_numbers = !self.set.line_numbers,
            "toggle_status_bar" => self.set.show_status_bar = !self.set.show_status_bar,
            "toggle_auto_save" => {
                self.set.auto_save = !self.set.auto_save;
                self.toast(if self.set.auto_save { "AutoSave on" } else { "AutoSave off" });
            }
            "toggle_word_wrap" => {
                let set = self.set.clone();
                if let Some(d) = self.doc() {
                    let w = d.wrap(&set);
                    d.wrap = Some(!w);
                }
            }
            "toggle_fullscreen" => {
                self.fullscreen = !self.fullscreen;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
            }
            "increase_font_size" => self.set.font_size = (self.set.font_size + 1.0).min(40.0),
            "decrease_font_size" => self.set.font_size = (self.set.font_size - 1.0).max(8.0),
            "reset_font_size" => self.set.font_size = 14.0,
            "toggle_record_macro" => match self.macro_rec.take() {
                Some(steps) => {
                    self.macro_saved = steps;
                    self.toast(format!("Macro recorded ({} steps)", self.macro_saved.len()));
                }
                None => {
                    self.macro_rec = Some(Vec::new());
                    self.toast("Recording macro… press Ctrl+Q to stop");
                }
            },
            "run_macro" => {
                let steps = self.macro_saved.clone();
                if steps.is_empty() {
                    self.toast("No macro recorded yet");
                }
                for s in steps {
                    match s {
                        MacroStep::Command(c) => self.run(ctx, &c),
                        MacroStep::Text(t) => {
                            if let Some(d) = self.doc() {
                                crate::edit::exec(&mut d.buf, crate::edit::Cmd::Insert(t), &d.cfg);
                                d.edited_at = Some(now);
                            }
                        }
                    }
                }
            }
            "cycle_mode" => {
                if let Some(d) = self.doc() {
                    let modes = d.kind.modes();
                    let i = modes.iter().position(|m| *m == d.mode).unwrap_or(0);
                    d.mode = modes[(i + 1) % modes.len()];
                    d.ed.focus = d.mode != Mode::Read;
                }
            }
            "mode" => {
                if let (Some(m), Some(d)) = (Mode::from_id(arg), self.docs.get_mut(self.active)) {
                    if d.kind.modes().contains(&m) {
                        d.mode = m;
                        d.ed.focus = m != Mode::Read;
                    }
                }
            }
            "theme" => self.set_theme(arg),
            "open_settings" => self.open_config(crate::settings::settings_path(), &serde_json::to_string_pretty(&self.set).unwrap_or_default()),
            "open_keybindings" => self.open_config(crate::settings::keymap_path(), crate::snippets::KEYMAP_TEMPLATE),
            "open_snippets" => self.open_config(crate::settings::snippets_path(), crate::snippets::SNIPPETS_TEMPLATE),
            "show_shortcuts" => self.dialog = Some(Dialog::Shortcuts),
            "about" => self.dialog = Some(Dialog::About),
            "check_for_updates" => {
                if cfg!(any(target_os = "android", target_os = "ios")) {
                    crate::shell_open(&crate::update::releases_url());
                } else {
                    if self.updater.available().is_none() {
                        self.updater.check(ctx, true);
                    }
                    self.dialog = Some(Dialog::Update);
                }
            }
            // the two steps behind the buttons of the update dialog
            "update_download" => self.updater.download(ctx),
            "update_install" => {
                if self.updater.install_and_restart() {
                    self.pending.push("quit".into());
                }
            }
            "help_docs" => crate::shell_open(&crate::update::docs_url()),
            "help_release_notes" => crate::shell_open(&crate::update::releases_url()),
            "help_report_issue" => crate::shell_open(&crate::update::issues_url()),
            "integrations" => self.dialog = Some(Dialog::Integrations),
            "cut" => ctx.send_viewport_cmd(egui::ViewportCommand::RequestCut),
            "copy" => ctx.send_viewport_cmd(egui::ViewportCommand::RequestCopy),
            // phones: read the system clipboard ourselves, the windowing layer cannot
            #[cfg(any(target_os = "android", target_os = "ios"))]
            "paste" => match crate::mobile::clipboard_get() {
                Some(text) if !text.is_empty() => {
                    self.inject.push(egui::Event::Paste(text));
                    ctx.request_repaint();
                }
                _ => self.toast("The clipboard is empty"),
            },
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            "paste" => ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste),
            "copy_path" => {
                if let Some(p) = self.doc().and_then(|d| d.path.clone()) {
                    ctx.copy_text(p.to_string_lossy().to_string());
                    self.toast("Path copied");
                }
            }
            "reveal_in_explorer" => {
                if let Some(p) = self.doc().and_then(|d| d.path.clone()) {
                    crate::platform::reveal(&p);
                }
            }
            "build" => crate::project::build(self, None),
            "cancel_build" => self.build.cancel(),
            "toggle_bottom_panel" => self.bottom = if self.bottom == Bottom::None { Bottom::Search } else { Bottom::None },
            "welcome" => self.open_welcome(),
            "quit" => {
                self.quit = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            n if n.starts_with("select_tab_") => {
                if let Ok(i) = n[11..].parse::<usize>() {
                    if i >= 1 && i <= self.docs.len() {
                        self.active = i - 1;
                    }
                }
            }
            n if n.starts_with("export_") || n == "print" => crate::export_ui::command(self, ctx, n),
            n if n.starts_with("tool_") => crate::tools::command(self, ctx, n),
            n if n.starts_with("layout_") || n.starts_with("focus_group_") || n.starts_with("move_to_group_") || n == "new_window" => {
                if n == "new_window" {
                    crate::platform::new_window();
                } else {
                    self.pane_command(n);
                }
            }
            _ => {}
        }
    }

    fn open_config(&mut self, path: PathBuf, template: &str) {
        if !path.exists() {
            let _ = std::fs::create_dir_all(crate::settings::config_dir());
            let _ = std::fs::write(&path, template);
        }
        self.open_path(&path);
        if let Some(d) = self.doc() {
            d.mode = Mode::Code;
        }
    }

    // ------------------------------------------------------------- input

    fn process_keys(&mut self, ctx: &egui::Context) {
        let focused = ctx.memory(|m| m.focused());
        let editor_focus = (focused == Some(editor::editor_id_of(0)) || focused == Some(editor::editor_id_of(1))) && self.picker.is_none() && self.dialog.is_none();
        let text_focus = focused.is_some() && !editor_focus;
        let mut commands = Vec::new();
        let modal = self.picker.is_some() || self.dialog.is_some();
        ctx.input_mut(|i| {
            let mut keep = Vec::with_capacity(i.events.len());
            for ev in i.events.drain(..) {
                if let egui::Event::Key { key, pressed: true, modifiers, .. } = &ev {
                    if modal {
                        keep.push(ev);
                        continue;
                    }
                    let combo = Combo::from_event(*key, *modifiers);
                    // plain typing keys in another text field belong to that field
                    let plain = !combo.ctrl && !combo.alt && !matches!(key, egui::Key::F1 | egui::Key::F2 | egui::Key::F3 | egui::Key::F4 | egui::Key::F5 | egui::Key::F6 | egui::Key::F7 | egui::Key::F8 | egui::Key::F9 | egui::Key::F10 | egui::Key::F11 | egui::Key::F12);
                    if text_focus && (plain || (combo.ctrl && !combo.shift && !combo.alt && matches!(key, egui::Key::A | egui::Key::C | egui::Key::V | egui::Key::X | egui::Key::Z | egui::Key::Y | egui::Key::ArrowLeft | egui::Key::ArrowRight | egui::Key::Backspace | egui::Key::Delete))) {
                        keep.push(ev);
                        continue;
                    }
                    match self.keymap.feed(combo, editor_focus) {
                        Match::Command(c, scope) => {
                            let _ = scope == Scope::Editor;
                            commands.push(c);
                        }
                        Match::Pending => {}
                        Match::None => keep.push(ev),
                    }
                } else {
                    keep.push(ev);
                }
            }
            i.events = keep;
        });
        if let Some(rec) = &mut self.macro_rec {
            if editor_focus {
                for ev in ctx.input(|i| i.events.clone()) {
                    if let egui::Event::Text(t) = ev {
                        rec.push(MacroStep::Text(t));
                    }
                }
            }
        }
        for c in commands {
            self.run(ctx, &c);
        }
    }

    fn tick(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        #[cfg(any(target_os = "android", target_os = "ios"))]
        self.poll_mobile(ctx, now);
        // updates: one quiet check a day, a few seconds after start-up
        if now > 3.0 {
            self.updater.auto_check(ctx, self.set.check_for_updates);
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs(3));
        }
        if self.updater.poll() && self.dialog.is_none() {
            self.dialog = Some(Dialog::Update);
        }
        // follow the OS theme
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        let sys = ctx.system_theme().map(|t| t == egui::Theme::Dark).unwrap_or(self.system_dark);
        // the windowing layer does not report the theme on phones; ask the system directly
        #[cfg(any(target_os = "android", target_os = "ios"))]
        let sys = crate::mobile::system_dark(now).or_else(|| ctx.system_theme().map(|t| t == egui::Theme::Dark)).unwrap_or(self.system_dark);
        if sys != self.system_dark {
            self.system_dark = sys;
            if theme::PALETTES.iter().all(|p| p.id != self.set.theme) {
                self.pal = theme::palette(&self.set.theme, sys);
                self.pal_epoch += 1;
            }
        }
        theme::apply_if_changed(ctx, &self.pal, self.pal_epoch);
        // egui throws its glyph atlas away when it fills up or text options change; galleys we
        // keep across frames would then point at the wrong pixels
        let (ratio, opts) = ctx.fonts_mut(|f| (f.font_atlas_fill_ratio(), *f.options()));
        let ppp = ctx.pixels_per_point();
        if ratio + 1e-4 < self.atlas.0 || self.atlas.1 != Some(opts) || (ppp - self.atlas.2).abs() > 1e-3 {
            self.atlas_resets += 1;
            self.pal_epoch += 1;
            theme::apply_if_changed(ctx, &self.pal, self.pal_epoch);
        }
        self.atlas = (ratio, Some(opts), ppp);
        // dropped files
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        for f in dropped {
            self.open_path(&f);
        }
        // AutoSave
        if self.set.auto_save {
            let delay = self.set.auto_save_delay_ms as f64 / 1000.0;
            let mut due = Vec::new();
            let mut soonest: Option<f64> = None;
            for (i, d) in self.docs.iter().enumerate() {
                if let (Some(t), true, true) = (d.edited_at, d.path.is_some(), d.dirty()) {
                    if now - t >= delay {
                        due.push(i);
                    } else {
                        soonest = Some(soonest.map_or(t + delay - now, |s: f64| s.min(t + delay - now)));
                    }
                }
            }
            for i in due {
                self.save_doc(i, false);
            }
            if let Some(s) = soonest {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(s.max(0.05)));
            }
        }
        // files changed by other programs
        if now - self.last_disk_check > 1.5 && ctx.input(|i| i.focused) {
            self.last_disk_check = now;
            let set = self.set.clone();
            let mut ask = None;
            for (i, d) in self.docs.iter_mut().enumerate() {
                if d.changed_on_disk() {
                    if d.dirty() {
                        d.mtime = d.path.as_ref().and_then(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
                        ask = Some(i);
                    } else {
                        let _ = d.reload(&set);
                    }
                }
            }
            if let (Some(i), None) = (ask, &self.dialog) {
                self.dialog = Some(Dialog::ReloadChanged(i));
            }
        }
        if self.set != self.saved_set {
            self.set.save();
            self.saved_set = self.set.clone();
        }
        for done in self.integ.poll() {
            match done {
                crate::export_ui::Done::Toast(t) => self.toast(t),
                crate::export_ui::Done::Open(url, msg) => {
                    self.toast(msg);
                    if !url.is_empty() {
                        ctx.open_url(egui::OpenUrl::new_tab(url));
                    }
                }
                crate::export_ui::Done::Connected(r) => match r {
                    Ok(()) => {
                        self.integ.connected = true;
                        self.integ.secret.clear();
                        self.toast("Google account connected");
                    }
                    Err(e) => self.integ.error = Some(e),
                },
            }
        }
        if self.integ.running() {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
        self.build.poll();
        self.search.poll();
        self.sidebar.poll();
        if self.build.running() || self.search.running() || self.sidebar.indexing() {
            ctx.request_repaint_after(std::time::Duration::from_millis(60));
        }
        // window title
        let title = match self.docs.get(self.active) {
            Some(d) => format!("{}{} - Inkdown", if d.dirty() { "● " } else { "" }, d.name),
            None => "Inkdown".to_string(),
        };
        if title != self.title {
            self.title = title.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            self.save_session();
            self.set.save();
        }
    }

    fn handle_actions(&mut self, ctx: &egui::Context, actions: Vec<Action>) {
        let now = ctx.input(|i| i.time);
        for a in actions {
            match a {
                Action::Open(url) => self.open_link(ctx, &url),
                Action::ToggleTask(line) => {
                    if let Some(d) = self.doc() {
                        let l = line.saturating_sub(1);
                        let text = d.buf.line_text(l);
                        let hit = text.find("[ ]").map(|i| (i, "[x]")).or_else(|| text.find("[x]").or(text.find("[X]")).map(|i| (i, "[ ]")));
                        if let Some((byte, new)) = hit {
                            let col = text[..byte].chars().count();
                            let st = d.buf.line_start(l) + col;
                            let sels = d.buf.sels.clone();
                            d.buf.edit(vec![Edit::new(st, st + 3, new)], EditKind::Other);
                            d.buf.sels = sels;
                            d.buf.commit_sels();
                            d.edited_at = Some(now);
                        }
                    }
                }
                Action::Zoom(key) => self.lightbox = Some((key, Default::default())),
                Action::Copy(t) => {
                    ctx.copy_text(t);
                    self.toast("Copied");
                }
                Action::ErrorLine(l) => {
                    if let Some(d) = self.doc() {
                        d.ed.error_line = l;
                    }
                }
                Action::Replace(t) => {
                    if let Some(d) = self.doc() {
                        d.buf.set_text(&t);
                        d.edited_at = Some(now);
                    }
                }
                Action::Toast(t) => self.toast(t),
            }
        }
    }

    pub fn open_link(&mut self, ctx: &egui::Context, url: &str) {
        if let Some(anchor) = url.strip_prefix('#') {
            if let Some(d) = self.doc() {
                d.view.md.goto_anchor = Some(anchor.to_string());
            }
            return;
        }
        if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("mailto:") {
            ctx.open_url(egui::OpenUrl::new_tab(url));
            return;
        }
        let (file, anchor) = url.split_once('#').map(|(f, a)| (f, Some(a.to_string()))).unwrap_or((url, None));
        let base = self.doc().and_then(|d| d.dir());
        match crate::assets::resolve_path(file, base.as_deref()) {
            Some(p) if p.is_file() => {
                let textish = Kind::of(&p.to_string_lossy(), "") != Kind::Code || crate::platform::looks_like_text(&p);
                if textish {
                    self.open_path(&p);
                    if let (Some(a), Some(d)) = (anchor, self.doc()) {
                        d.view.md.goto_anchor = Some(a);
                    }
                } else {
                    crate::platform::shell_open(&p.to_string_lossy());
                }
            }
            Some(p) if p.is_dir() => crate::platform::shell_open(&p.to_string_lossy()),
            _ => self.toast(format!("Cannot open {url}")),
        }
    }

    // -------------------------------------------------------------- view

    fn ed_style(&self, d: &Doc) -> EdStyle {
        EdStyle {
            pal: self.pal,
            font_size: self.set.font_size,
            line_numbers: self.set.line_numbers,
            minimap: self.set.minimap && d.mode == Mode::Code,
            highlight_line: self.set.highlight_line,
            indent_guides: self.set.indent_guides,
            rulers: self.set.rulers.clone(),
            wrap: d.wrap(&self.set),
            auto_complete: self.set.auto_complete,
            read_only: d.read_only,
        }
    }

    fn code_pane(&mut self, ui: &mut Ui) {
        let snippets = self.snippets_for(self.active);
        let now = ui.input(|i| i.time);
        let pal = self.pal;
        let Some(d) = self.docs.get(self.active) else { return };
        let st = self.ed_style(d);
        let md_bar = d.kind == Kind::Markdown && !d.read_only;
        if md_bar {
            egui::Panel::top(("md-toolbar", editor::pane())).frame(egui::Frame::new().fill(pal.bg).inner_margin(egui::Margin::symmetric(8, 4))).show_separator_line(false).show(ui, |ui| {
                crate::mdfmt::toolbar(ui, self);
            });
        }
        let d = &mut self.docs[self.active];
        if d.ed.find.open {
            egui::Panel::bottom(("find-bar", editor::pane())).frame(egui::Frame::new()).show(ui, |ui| {
                if editor::find_bar(ui, &mut d.ed, &mut d.buf, &pal) {
                    d.edited_at = Some(now);
                }
            });
        }
        let mut action = None;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(pal.bg)).show(ui, |ui| {
            let out = editor::show(ui, &mut d.ed, &mut d.buf, &mut d.hl, &d.cfg, &st, &snippets);
            if out.changed {
                d.edited_at = Some(now);
            }
            action = out.action;
        });
        if let Some(a) = action {
            let ctx = ui.ctx().clone();
            self.run(&ctx, a);
        }
    }

    fn read_pane(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let pal_epoch = self.pal_epoch;
        let Some(d) = self.docs.get_mut(self.active) else { return };
        let id = Id::new(("view", d.id));
        let mut cx = ViewCtx { pal: &pal, assets: &mut self.assets, font_size: self.set.document_font_size, doc_width: self.set.document_width, base: d.dir(), actions: Vec::new() };
        let version = d.buf.version;
        let buf = &d.buf;
        match d.kind {
            Kind::Markdown => preview::markdown(ui, &mut d.view.md, id, || buf.text(), version, &mut cx, pal_epoch),
            Kind::Asciidoc => preview::markdown(ui, &mut d.view.md, id, || crate::adoc::to_markdown(&buf.text()), version, &mut cx, pal_epoch),
            Kind::Data(fmt) => preview::data_tree(ui, &mut d.view.tree, id, fmt, || buf.text(), version, &mut cx),
            Kind::Table(delim) => preview::table(ui, &mut d.view.table, id, delim, || buf.text(), version, &mut cx),
            Kind::Mermaid => {
                let t = buf.text();
                preview::canvas(ui, &mut d.view.canvas, id, CanvasSource::Mermaid(&t), &mut cx);
            }
            Kind::Dot => {
                let t = buf.text();
                preview::canvas(ui, &mut d.view.canvas, id, CanvasSource::Dot(&t), &mut cx);
            }
            Kind::Svg => {
                let t = buf.text();
                preview::canvas(ui, &mut d.view.canvas, id, CanvasSource::Svg(&t), &mut cx);
            }
            Kind::Image => {
                if let Some(b) = d.bytes.clone() {
                    preview::canvas(ui, &mut d.view.canvas, id, CanvasSource::Image(b), &mut cx);
                }
            }
            Kind::Code => {}
        }
        let actions = cx.actions;
        let ctx = ui.ctx().clone();
        self.handle_actions(&ctx, actions);
    }

    /// Keep the two halves of Split mode showing the same part of the document.
    fn sync_split(&mut self, ui: &Ui, editor_rect: egui::Rect) {
        let Some(d) = self.docs.get_mut(self.active) else { return };
        if !matches!(d.kind, Kind::Markdown | Kind::Asciidoc) {
            return;
        }
        let over_editor = ui.rect_contains_pointer(editor_rect);
        let (id, top, pscroll) = (d.id, d.ed.top_line, d.view.md.scroll);
        let (_, last_top, last_scroll) = self.last_sync;
        if self.last_sync.0 != id {
            self.last_sync = (id, top, pscroll);
            return;
        }
        if top != last_top && (over_editor || d.ed.has_focus) && !(pscroll != last_scroll && !over_editor) {
            d.view.md.goto_line = Some(top + 1);
            ui.ctx().request_repaint();
        } else if (pscroll - last_scroll).abs() > 0.5 && !over_editor {
            let line = d.view.md.line_at_y(pscroll + 24.0).saturating_sub(1);
            d.ed.scroll_to_line(line);
        }
        let d = &self.docs[self.active];
        self.last_sync = (id, d.ed.top_line, d.view.md.scroll);
    }

    /// Keep the split layout consistent with what was activated or focused since last frame.
    fn sync_panes(&mut self, ctx: &egui::Context) {
        let Some(mut panes) = self.split else {
            editor::set_pane(0);
            self.focus_pane = 0;
            self.last_active = self.docs.get(self.active).map(|d| d.id).unwrap_or(0);
            return;
        };
        let has = |id: u64| self.docs.iter().any(|d| d.id == id);
        if !has(panes[0]) || !has(panes[1]) || panes[0] == panes[1] {
            // a pane's document was closed: refill it, or drop the split
            let keep = if has(panes[0]) { panes[0] } else { panes[1] };
            match self.docs.iter().find(|d| d.id != keep && has(keep)) {
                Some(other) if self.docs.len() >= 2 => {
                    let other = other.id;
                    panes = if has(panes[0]) { [keep, other] } else { [other, keep] };
                    self.split = Some(panes);
                }
                _ => {
                    self.split = None;
                    editor::set_pane(0);
                    self.focus_pane = 0;
                    return;
                }
            }
        }
        let f = self.focus_pane.min(1) as usize;
        let active_id = self.docs.get(self.active).map(|d| d.id).unwrap_or(0);
        if active_id != self.last_active && active_id != panes[f] {
            // something else was activated (a tab click, Open, Goto Anything …)
            if active_id == panes[1 - f] {
                self.focus_pane = (1 - f) as u8;
            } else {
                panes[f] = active_id;
                self.split = Some(panes);
            }
        } else {
            let focused = ctx.memory(|m| m.focused());
            for p in 0..2u8 {
                if focused == Some(editor::editor_id_of(p)) {
                    self.focus_pane = p;
                }
            }
        }
        let f = self.focus_pane.min(1) as usize;
        if let Some(i) = self.docs.iter().position(|d| d.id == panes[f]) {
            self.active = i;
        }
        self.last_active = panes[f];
        editor::set_pane(self.focus_pane);
    }

    fn doc_view(&mut self, ui: &mut Ui) {
        let Some(panes) = self.split else {
            self.pane_view(ui);
            return;
        };
        let pal = self.pal;
        let ix = |me: &Self, id: u64| me.docs.iter().position(|d| d.id == id);
        let (Some(left), Some(right)) = (ix(self, panes[0]), ix(self, panes[1])) else {
            self.pane_view(ui);
            return;
        };
        let focus = self.focus_pane;
        let total = ui.available_width();
        let pressed = ui.input(|i| if i.pointer.any_pressed() { i.pointer.interact_pos() } else { None });
        let mark = |ui: &Ui, on: bool| {
            let r = ui.max_rect();
            ui.painter().rect_filled(egui::Rect::from_min_size(r.min, vec2(r.width(), 2.0)), 0, if on { pal.accent } else { pal.border_soft });
        };
        let mut clicked: Option<u8> = None;
        let l = egui::Panel::left("pane-left").resizable(true).default_size(total / 2.0).size_range(220.0..=(total - 220.0).max(240.0)).frame(egui::Frame::new().fill(pal.bg)).show(ui, |ui| {
            editor::set_pane(0);
            self.active = left;
            mark(ui, focus == 0);
            ui.add_space(2.0);
            self.pane_view(ui);
        });
        if pressed.is_some_and(|p| l.response.rect.contains(p)) {
            clicked = Some(0);
        }
        let r = egui::CentralPanel::default().frame(egui::Frame::new().fill(pal.bg)).show(ui, |ui| {
            editor::set_pane(1);
            self.active = right;
            mark(ui, focus == 1);
            ui.add_space(2.0);
            self.pane_view(ui);
        });
        if pressed.is_some_and(|p| r.response.rect.contains(p)) {
            clicked = Some(1);
        }
        if let Some(p) = clicked {
            self.focus_pane = p;
        }
        editor::set_pane(self.focus_pane);
        self.active = if self.focus_pane == 0 { left } else { right };
        self.last_active = panes[self.focus_pane as usize];
    }

    fn pane_command(&mut self, name: &str) {
        match name {
            "layout_single" => self.split = None,
            "layout_columns_2" => {
                let Some(cur) = self.docs.get(self.active).map(|d| d.id) else { return };
                match self.docs.iter().map(|d| d.id).find(|id| *id != cur) {
                    Some(other) => {
                        // the current file stays on the left; the next tab opens beside it
                        let next = self.docs.get(self.active + 1).map(|d| d.id).unwrap_or(other);
                        self.split = Some([cur, next]);
                        self.focus_pane = 0;
                    }
                    None => self.toast("Open a second file to view two side by side"),
                }
            }
            "focus_group_1" | "focus_group_2" => {
                let p = if name.ends_with('1') { 0 } else { 1 };
                if let Some(panes) = self.split {
                    self.focus_pane = p;
                    if let Some(d) = self.docs.iter_mut().find(|d| d.id == panes[p as usize]) {
                        d.ed.focus = true;
                    }
                }
            }
            "move_to_group_1" | "move_to_group_2" => {
                let p = if name.ends_with('1') { 0usize } else { 1 };
                let Some(cur) = self.docs.get(self.active).map(|d| d.id) else { return };
                match self.split {
                    Some(mut panes) => {
                        if panes[p] != cur {
                            panes.swap(0, 1);
                            self.split = Some(panes);
                            self.focus_pane = p as u8;
                        }
                    }
                    None => {
                        self.pane_command("layout_columns_2");
                        if let (Some(mut panes), 1) = (self.split, p) {
                            panes.swap(0, 1);
                            self.split = Some(panes);
                            self.focus_pane = 1;
                        }
                    }
                }
            }
            _ => self.toast("That layout is not available; use two columns (Alt+Shift+2)"),
        }
    }

    fn pane_view(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let Some(d) = self.docs.get(self.active) else {
            egui::CentralPanel::default().frame(egui::Frame::new().fill(pal.bg)).show(ui, |ui| crate::shell::empty_state(ui, self));
            return;
        };
        let mode = if d.kind.modes().contains(&d.mode) { d.mode } else { d.kind.modes()[0] };
        let narrow = ui.available_width() < 700.0;
        match mode {
            Mode::Code => self.code_pane(ui),
            Mode::Read => {
                egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| self.read_pane(ui));
            }
            Mode::Split if narrow => {
                egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| self.read_pane(ui));
            }
            Mode::Split => {
                let total = ui.available_width();
                let resp = egui::Panel::left(("split-editor", editor::pane())).resizable(true).default_size(total * self.split_ratio).size_range(240.0..=(total - 240.0).max(260.0)).frame(egui::Frame::new().fill(pal.bg)).show(ui, |ui| self.code_pane(ui));
                let rect = resp.response.rect;
                egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| self.read_pane(ui));
                self.sync_split(ui, rect);
            }
            Mode::Write => {
                egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| {
                    let kind = self.docs[self.active].kind;
                    if kind == Kind::Svg {
                        crate::designer::show(ui, self);
                    } else {
                        crate::rich::show(ui, self);
                    }
                });
            }
        }
    }
}

fn strip_unc(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => p,
    }
}

impl eframe::App for InkApp {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        raw.events.append(&mut self.inject);
        // The Back button or gesture of a phone closes whatever is open, like Esc on a keyboard.
        for ev in &mut raw.events {
            if let egui::Event::Key { key, .. } = ev {
                if *key == egui::Key::BrowserBack {
                    *key = egui::Key::Escape;
                }
            }
        }
        if let Some(a) = &mut self.automation {
            a.inject(ctx, raw);
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let cpu_ms = _frame.info().cpu_usage.unwrap_or(0.0) * 1000.0;
        self.total_sum += cpu_ms;
        let ctx = ui.ctx().clone();
        self.assets.poll(&ctx);
        if let Some(mut a) = self.automation.take() {
            a.step(self, &ctx);
            self.automation = Some(a);
        }
        let t0 = std::time::Instant::now();
        self.sync_panes(&ctx);
        self.process_keys(&ctx);
        let pending = std::mem::take(&mut self.pending);
        for c in pending {
            self.run(&ctx, &c);
        }
        self.tick(&ctx);

        let narrow = ctx.content_rect().width() < 720.0;
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            // keep clear of the status bar and the navigation bar
            let (top, bottom) = crate::mobile::insets_px();
            let ppp = ctx.pixels_per_point();
            let pal = self.pal;
            if top > 0.0 {
                // the system draws its clock and icons in white, so the strip behind them stays dark
                let strip = if pal.dark { pal.chrome } else { egui::Color32::from_rgb(0x1e, 0x22, 0x30) };
                egui::Panel::top("inset-top").exact_size(top / ppp).frame(egui::Frame::new().fill(strip)).show_separator_line(false).show(ui, |_| {});
            }
            if bottom > 0.0 {
                egui::Panel::bottom("inset-bottom").exact_size(bottom / ppp).frame(egui::Frame::new().fill(pal.chrome)).show_separator_line(false).show(ui, |_| {});
            }
        }
        crate::shell::top_bar(ui, self, narrow);
        crate::shell::tab_strip(ui, self);
        if self.set.show_status_bar {
            crate::shell::status_bar(ui, self);
        }
        if self.set.show_sidebar && !narrow {
            crate::sidebar::show(ui, self);
        }
        if self.bottom != Bottom::None {
            crate::project::bottom_panel(ui, self);
        }
        self.doc_view(ui);

        crate::picker::show(&ctx, self);
        crate::shell::dialogs(&ctx, self);
        crate::shell::lightbox(&ctx, self);
        crate::shell::toasts(&ctx, self);
        if narrow && self.set.show_sidebar {
            crate::sidebar::drawer(&ctx, self);
        }
        // The default window is 1280 x 840 points. On a smaller screen (a 1366 x 768 laptop at
        // 125 % is only 1093 x 614 points) shrink it so that it fits on the screen.
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        if self.frame_count < 3 {
            let (monitor, inner, outer) = ctx.input(|i| (i.viewport().monitor_size, i.viewport().inner_rect, i.viewport().outer_rect));
            if let (Some(m), Some(inner), Some(outer)) = (monitor, inner, outer) {
                // leave room for the title bar, window borders and the taskbar
                let chrome = outer.size() - inner.size();
                let max = vec2((m.x - chrome.x - 16.0).max(360.0), (m.y - chrome.y - 64.0).max(300.0));
                if m.x > 0.0 && (inner.width() > max.x + 0.5 || inner.height() > max.y + 0.5) {
                    let size = vec2(inner.width().min(max.x), inner.height().min(max.y));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(8.0, 8.0)));
                }
            }
        }
        // Commands queued by a button run at the start of the next frame, and several views
        // settle one frame after the input that changed them. With a mouse there is always
        // another frame (the pointer keeps moving); after a finger tap there is not, so a
        // tapped menu item appeared to do nothing until the next touch. Ask for the frame.
        // phones: text copied in Inkdown goes to the system clipboard for other apps
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            let copied: Vec<String> = ctx.output(|o| o.commands.iter().filter_map(|c| if let egui::OutputCommand::CopyText(t) = c { Some(t.clone()) } else { None }).collect());
            if let Some(text) = copied.last() {
                crate::mobile::clipboard_set(text);
            }
        }
        let had_input = ctx.input(|i| !i.events.is_empty() || i.pointer.any_released() || i.pointer.any_pressed());
        if !self.pending.is_empty() || had_input {
            ctx.request_repaint();
        }
        // An editor can lose focus for a single frame during a click. Without this the phone
        // keyboard gets "hide" and "show" back to back and stays hidden.
        match ctx.output(|o| o.ime.clone()) {
            Some(ime) => self.ime_keep = Some((ime, 0)),
            None => match &mut self.ime_keep {
                Some((ime, age)) if *age < 3 => {
                    *age += 1;
                    let ime = ime.clone();
                    ctx.output_mut(|o| o.ime = Some(ime));
                    ctx.request_repaint();
                }
                _ => self.ime_keep = None,
            },
        }
        self.frame_ms = t0.elapsed().as_secs_f32() * 1000.0;
        // on a phone, a line in the system log every 120 frames tells how smooth things are:
        // time spent building a frame, and the gap between frames while they follow closely
        #[cfg(target_os = "android")]
        {
            use std::sync::Mutex;
            static STATS: Mutex<Option<(std::time::Instant, u32, f32, f32, u32, f32, f32)>> = Mutex::new(None);
            static CPU: Mutex<(f32, f32)> = Mutex::new((0.0, 0.0));
            let mut cpu = CPU.lock().unwrap();
            cpu.0 += cpu_ms;
            cpu.1 = cpu.1.max(cpu_ms);
            let mut s = STATS.lock().unwrap();
            let now_i = std::time::Instant::now();
            let st = s.get_or_insert((now_i, 0, 0.0, 0.0, 0, 0.0, 0.0));
            let gap = now_i.duration_since(st.0).as_secs_f32() * 1000.0;
            st.0 = now_i;
            st.1 += 1;
            st.2 += self.frame_ms;
            st.3 = st.3.max(self.frame_ms);
            if gap < 120.0 {
                st.4 += 1;
                st.5 += gap;
                st.6 = st.6.max(gap);
            }
            if st.1 >= 120 {
                crate::mobile::log(&format!("frames: build avg {:.1} ms, max {:.1} ms; build+paint avg {:.1} ms, max {:.1} ms; gap avg {:.1} ms, max {:.1} ms over {} frames; atlas {:.0}% full, {} resets, scale {:.2}", st.2 / st.1 as f32, st.3, cpu.0 / st.1 as f32, cpu.1, if st.4 > 0 { st.5 / st.4 as f32 } else { 0.0 }, st.6, st.4, ctx.fonts(|f| f.font_atlas_fill_ratio()) * 100.0, self.atlas_resets, ctx.pixels_per_point()));
                *cpu = (0.0, 0.0);
                *st = (now_i, 0, 0.0, 0.0, 0, 0.0, 0.0);
            }
        }
        self.frame_max = self.frame_max.max(self.frame_ms);
        if self.frame_ms > 50.0 && self.slow.len() < 40 {
            self.slow.push(format!("{:.0} ms after {}", self.frame_ms, self.last_cmd));
        }
        self.frame_sum += self.frame_ms;
        self.frame_count += 1;
        if self.first_frame_ms.is_none() {
            self.first_frame_ms = Some(self.started.elapsed().as_secs_f32() * 1000.0);
        }
        let _ = vec2(0.0, 0.0);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let c = self.pal.bg;
        [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, 1.0]
    }
}
