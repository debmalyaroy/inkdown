//! Project-wide tools: Find in Files, build systems with an output panel, and
//! search-based Go to Definition.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};

use egui::{vec2, Sense, Ui};

use crate::app::{Bottom, InkApp};
use crate::icons;
use crate::settings::{BuildSystem, Settings};
use crate::theme;

// --------------------------------------------------------- find in files

#[derive(Clone)]
pub struct Hit {
    pub path: PathBuf,
    pub line: usize,
    pub col: usize,
    pub len: usize,
    pub text: String,
}

enum Msg {
    Hits(Vec<Hit>),
    Done(usize),
}

#[derive(Default)]
pub struct Search {
    pub query: String,
    pub replace: String,
    pub regex: bool,
    pub case: bool,
    pub word: bool,
    pub results: Vec<Hit>,
    pub files: usize,
    pub scanned: usize,
    pub error: Option<String>,
    pub focus: bool,
    rx: Option<Receiver<Msg>>,
    cancel: Option<Arc<AtomicBool>>,
    pub confirm_replace: bool,
}

pub fn build_regex(query: &str, regex: bool, case: bool, word: bool) -> Result<regex::Regex, String> {
    let mut p = if regex { query.to_string() } else { regex::escape(query) };
    if word {
        p = format!(r"\b(?:{p})\b");
    }
    regex::RegexBuilder::new(&p).case_insensitive(!case).build().map_err(|e| e.to_string().lines().last().unwrap_or("Invalid pattern").trim().to_string())
}

fn search_file(path: &Path, re: &regex::Regex, out: &mut Vec<Hit>) {
    let Ok(meta) = std::fs::metadata(path) else { return };
    if meta.len() > 6 * 1024 * 1024 {
        return;
    }
    let Ok(bytes) = std::fs::read(path) else { return };
    if bytes.iter().take(4096).any(|b| *b == 0) {
        return;
    }
    let text = String::from_utf8_lossy(&bytes);
    for (ln, line) in text.lines().enumerate() {
        if line.len() > 2000 {
            continue;
        }
        for m in re.find_iter(line) {
            if m.start() == m.end() {
                continue;
            }
            let shown: String = line.trim_end().chars().take(240).collect();
            out.push(Hit { path: path.to_path_buf(), line: ln, col: line[..m.start()].chars().count(), len: line[m.start()..m.end()].chars().count(), text: shown });
            if out.len() >= 200 {
                return;
            }
        }
    }
}

impl Search {
    pub fn running(&self) -> bool {
        self.rx.is_some()
    }

    pub fn start(&mut self, files: Vec<PathBuf>) {
        if let Some(c) = self.cancel.take() {
            c.store(true, Ordering::Relaxed);
        }
        self.results.clear();
        self.files = 0;
        self.scanned = 0;
        self.error = None;
        self.rx = None;
        if self.query.is_empty() {
            return;
        }
        let re = match build_regex(&self.query, self.regex, self.case, self.word) {
            Ok(r) => r,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(cancel.clone());
        self.rx = Some(rx);
        let _ = std::thread::Builder::new().name("ink-search".into()).spawn(move || {
            let mut total = 0usize;
            let mut scanned = 0usize;
            for chunk in files.chunks(64) {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let mut hits = Vec::new();
                for f in chunk {
                    let mut local = Vec::new();
                    search_file(f, &re, &mut local);
                    scanned += 1;
                    hits.extend(local);
                }
                total += hits.len();
                if !hits.is_empty() && tx.send(Msg::Hits(hits)).is_err() {
                    return;
                }
                if total >= 20_000 {
                    break;
                }
            }
            let _ = tx.send(Msg::Done(scanned));
        });
    }

    pub fn poll(&mut self) {
        let Some(rx) = &self.rx else { return };
        let mut done = false;
        while let Ok(m) = rx.try_recv() {
            match m {
                Msg::Hits(h) => self.results.extend(h),
                Msg::Done(n) => {
                    self.scanned = n;
                    done = true;
                }
            }
        }
        if done {
            self.rx = None;
            let mut files: Vec<&PathBuf> = self.results.iter().map(|h| &h.path).collect();
            files.dedup();
            self.files = files.len();
        }
    }
}

fn search_scope(app: &InkApp) -> Vec<PathBuf> {
    match app.sidebar.index() {
        Some(i) => i.as_ref().clone(),
        None => app.docs.iter().filter_map(|d| d.path.clone()).collect(),
    }
}

/// Replace every match in every file that has results. Open, unmodified tabs reload on their own.
pub fn replace_in_files(app: &mut InkApp) {
    let Ok(re) = build_regex(&app.search.query, app.search.regex, app.search.case, app.search.word) else { return };
    let mut files: Vec<PathBuf> = app.search.results.iter().map(|h| h.path.clone()).collect();
    files.dedup();
    let rep = if app.search.regex { app.search.replace.clone() } else { app.search.replace.replace('$', "$$") };
    let (mut changed, mut failed) = (0usize, 0usize);
    for f in &files {
        // never write under a tab with unsaved edits
        if app.docs.iter().any(|d| d.path.as_ref() == Some(f) && d.dirty()) {
            failed += 1;
            continue;
        }
        let Ok(bytes) = std::fs::read(f) else {
            failed += 1;
            continue;
        };
        let Ok((text, enc, bom)) = crate::doc::decode(&bytes) else {
            failed += 1;
            continue;
        };
        let new = re.replace_all(&text, rep.as_str());
        if new != text {
            match std::fs::write(f, crate::doc::encode(&new, enc, bom)) {
                Ok(()) => changed += 1,
                Err(_) => failed += 1,
            }
        }
    }
    app.toast(if failed > 0 { format!("Replaced in {changed} files; {failed} skipped (unsaved or not writable)") } else { format!("Replaced in {changed} files") });
    let scope = search_scope(app);
    app.search.start(scope);
}

// ------------------------------------------------------------------ build

#[derive(Default)]
pub struct Build {
    pub lines: Vec<String>,
    pub title: String,
    rx: Option<Receiver<Option<String>>>,
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    child: Option<Arc<Mutex<std::process::Child>>>,
    started: Option<std::time::Instant>,
    pub cwd: Option<PathBuf>,
}

impl Build {
    pub fn running(&self) -> bool {
        self.rx.is_some()
    }

    pub fn cancel(&mut self) {
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        if let Some(c) = self.child.take() {
            let _ = c.lock().unwrap().kill();
            self.lines.push("[Cancelled]".into());
        }
    }

    pub fn poll(&mut self) {
        let Some(rx) = &self.rx else { return };
        let mut done = false;
        for _ in 0..4000 {
            match rx.try_recv() {
                Ok(Some(line)) => self.lines.push(line),
                Ok(None) => done = true,
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(_) => {
                    done = true;
                    break;
                }
            }
        }
        if self.lines.len() > 20_000 {
            let cut = self.lines.len() - 20_000;
            self.lines.drain(..cut);
        }
        if done {
            self.rx = None;
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            {
                let code = self.child.take().and_then(|c| c.lock().unwrap().wait().ok()).and_then(|s| s.code());
                let secs = self.started.map(|s| s.elapsed().as_secs_f32()).unwrap_or(0.0);
                self.lines.push(match code {
                    Some(0) => format!("[Finished in {secs:.1}s]"),
                    Some(c) => format!("[Finished in {secs:.1}s with exit code {c}]"),
                    None => format!("[Stopped after {secs:.1}s]"),
                });
            }
        }
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    fn start(&mut self, title: String, cmd: &[String], cwd: Option<PathBuf>) {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};
        self.cancel();
        self.lines.clear();
        self.title = title;
        self.cwd = cwd.clone();
        let Some((program, args)) = cmd.split_first() else { return };
        self.lines.push(format!("> {}", cmd.join(" ")));
        let mut c = Command::new(program);
        c.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(d) = cwd.filter(|d| d.is_dir()) {
            c.current_dir(d);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000);
        }
        match c.spawn() {
            Ok(mut child) => {
                let (tx, rx) = channel();
                let out = child.stdout.take();
                let err = child.stderr.take();
                let pending = Arc::new(Mutex::new(2));
                for stream in [out.map(|s| Box::new(s) as Box<dyn std::io::Read + Send>), err.map(|s| Box::new(s) as Box<dyn std::io::Read + Send>)] {
                    let tx = tx.clone();
                    let pending = pending.clone();
                    std::thread::spawn(move || {
                        if let Some(s) = stream {
                            let mut reader = BufReader::new(s);
                            let mut buf = Vec::new();
                            while let Ok(n) = reader.read_until(b'\n', &mut buf) {
                                if n == 0 {
                                    break;
                                }
                                let line = String::from_utf8_lossy(&buf).trim_end_matches(['\n', '\r']).to_string();
                                buf.clear();
                                if tx.send(Some(strip_ansi(&line))).is_err() {
                                    break;
                                }
                            }
                        }
                        let mut p = pending.lock().unwrap();
                        *p -= 1;
                        if *p == 0 {
                            let _ = tx.send(None);
                        }
                    });
                }
                self.child = Some(Arc::new(Mutex::new(child)));
                self.rx = Some(rx);
                self.started = Some(std::time::Instant::now());
            }
            Err(e) => self.lines.push(format!("Could not start {program}: {e}")),
        }
    }
}

fn strip_ansi(s: &str) -> String {
    if !s.contains('\u{1b}') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for n in chars.by_ref() {
                if n.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn build_systems(set: &Settings) -> Vec<BuildSystem> {
    let b = |name: &str, sel: &[&str], cmd: &[&str]| BuildSystem { name: name.into(), selector: sel.iter().map(|s| s.to_string()).collect(), cmd: cmd.iter().map(|s| s.to_string()).collect(), working_dir: String::new() };
    let mut v = set.build_systems.clone();
    v.extend([
        b("Cargo: run", &["rs"], &["cargo", "run"]),
        b("Cargo: build", &["rs", "toml"], &["cargo", "build"]),
        b("Cargo: test", &["rs"], &["cargo", "test"]),
        b("Cargo: check", &["rs"], &["cargo", "check"]),
        b("Python", &["py"], &["python", "-u", "$file"]),
        b("Node.js", &["js", "mjs", "cjs"], &["node", "$file"]),
        b("TypeScript (tsx)", &["ts", "tsx"], &["npx", "tsx", "$file"]),
        b("Go: run", &["go"], &["go", "run", "$file"]),
        b("Shell", &["sh", "bash"], &["bash", "$file"]),
        b("PowerShell", &["ps1"], &["pwsh", "-NoProfile", "-File", "$file"]),
        b("Batch", &["bat", "cmd"], &["cmd", "/C", "$file"]),
        b("C (gcc)", &["c"], &["gcc", "$file", "-o", "$file_base_name"]),
        b("C++ (g++)", &["cpp", "cc", "cxx"], &["g++", "$file", "-o", "$file_base_name"]),
        b("Java", &["java"], &["java", "$file"]),
        b("Ruby", &["rb"], &["ruby", "$file"]),
        b("PHP", &["php"], &["php", "$file"]),
        b("Lua", &["lua"], &["lua", "$file"]),
        b("Perl", &["pl"], &["perl", "$file"]),
        b("Make", &["mk", "makefile"], &["make"]),
        b("npm: run build", &["json"], &["npm", "run", "build"]),
    ]);
    v
}

pub fn build(app: &mut InkApp, which: Option<usize>) {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let _ = which;
        app.toast("Build systems need a desktop.");
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let Some(d) = app.docs.get(app.active) else { return };
        let Some(path) = d.path.clone() else {
            app.toast("Save the file before building it.");
            return;
        };
        if d.dirty() {
            app.save_doc(app.active, false);
        }
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| name.to_lowercase());
        let systems = build_systems(&app.set);
        let sys = match which {
            Some(i) => systems.get(i).cloned(),
            None => systems.iter().find(|s| s.selector.iter().any(|x| x.eq_ignore_ascii_case(&ext))).cloned(),
        };
        let Some(sys) = sys else {
            app.toast(format!("No build system for .{ext} files. Add one under build_systems in Settings."));
            return;
        };
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let base = path.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let folder = app.sidebar.folder.clone().unwrap_or_else(|| dir.clone());
        let sub = |s: &str| s.replace("$file_base_name", &base).replace("$file_name", &name).replace("$file_path", &dir.to_string_lossy()).replace("$folder", &folder.to_string_lossy()).replace("$file", &path.to_string_lossy());
        let cmd: Vec<String> = sys.cmd.iter().map(|c| sub(c)).collect();
        // cargo and make run from the project root
        let mut cwd = if sys.working_dir.is_empty() { dir.clone() } else { PathBuf::from(sub(&sys.working_dir)) };
        if cmd.first().is_some_and(|c| c == "cargo" || c == "npm" || c == "make") {
            let marker = match cmd[0].as_str() {
                "cargo" => "Cargo.toml",
                "npm" => "package.json",
                _ => "Makefile",
            };
            let mut p = Some(dir.as_path());
            while let Some(x) = p {
                if x.join(marker).exists() {
                    cwd = x.to_path_buf();
                    break;
                }
                p = x.parent();
            }
        }
        app.build.start(sys.name.clone(), &cmd, Some(cwd));
        app.bottom = Bottom::Build;
    }
}

// ----------------------------------------------------- go to definition

pub fn definition_regex(word: &str) -> Option<regex::Regex> {
    let w = regex::escape(word);
    let p = format!(
        r"(?m)^[ \t]*(?:(?:pub(?:\([^)]*\))?|export|default|async|static|public|private|protected|internal|abstract|final|unsafe|extern|inline|virtual|override|const)\s+)*(?:fn|def|class|struct|enum|trait|interface|type|function|func|module|mod|macro_rules!|namespace|object|record|union|typedef|sub|proc|let|var|val|const)\s+(?:\([^)]*\)\s*)?{w}\b|^[ \t]*(?:export\s+)?(?:const|let|var)?\s*{w}\s*[:=]\s*(?:async\s*)?(?:function\b|\([^)]*\)\s*=>)|^#{{1,6}}\s+{w}\b"
    );
    regex::Regex::new(&p).ok()
}

pub fn goto_definition(app: &mut InkApp) {
    let Some(d) = app.docs.get(app.active) else { return };
    let s = d.buf.primary();
    let (a, z) = if s.is_empty() { crate::edit::word_at(&d.buf, s.head) } else { (s.start(), s.end()) };
    let word = d.buf.slice(a, z);
    if word.trim().is_empty() || !word.chars().all(crate::buffer::is_word) {
        app.toast("Place the caret on a name first");
        return;
    }
    let Some(re) = definition_regex(&word) else { return };
    let here = d.path.clone();
    let here_line = d.buf.line_of(a);
    let mut hits: Vec<Hit> = Vec::new();
    // this file first
    let text = d.buf.text();
    for m in re.find_iter(&text) {
        let line = text[..m.start()].matches('\n').count();
        if line != here_line {
            let line_text = text.lines().nth(line).unwrap_or("").to_string();
            let col = line_text.find(&word).map(|b| line_text[..b].chars().count()).unwrap_or(0);
            hits.push(Hit { path: here.clone().unwrap_or_default(), line, col, len: word.chars().count(), text: line_text });
        }
    }
    if hits.is_empty() {
        let ext = here.as_ref().and_then(|p| p.extension()).map(|e| e.to_string_lossy().to_lowercase());
        let started = std::time::Instant::now();
        let mut files = search_scope(app);
        // same language first
        files.sort_by_key(|f| f.extension().map(|e| e.to_string_lossy().to_lowercase()) != ext);
        for f in files {
            if Some(&f) == here.as_ref() {
                continue;
            }
            let mut local = Vec::new();
            search_file(&f, &re, &mut local);
            for mut h in local {
                h.col = h.text.find(&word).map(|b| h.text[..b].chars().count()).unwrap_or(h.col);
                h.len = word.chars().count();
                hits.push(h);
            }
            if hits.len() > 50 || started.elapsed().as_millis() > 1500 {
                break;
            }
        }
    }
    match hits.len() {
        0 => app.toast(format!("No definition found for {word}")),
        1 => {
            let h = hits.remove(0);
            if h.path.as_os_str().is_empty() || Some(&h.path) == here.as_ref() {
                if let Some(d) = app.doc() {
                    d.ed.goto(&mut d.buf, h.line, h.col);
                }
            } else {
                app.open_at(&h.path, h.line, h.col);
            }
        }
        n => {
            app.search.query = word.clone();
            app.search.results = hits;
            app.search.files = 0;
            app.search.error = None;
            app.bottom = Bottom::Search;
            app.toast(format!("{n} definitions of {word}"));
        }
    }
}

// ---------------------------------------------------------- bottom panel

pub fn bottom_panel(ui: &mut Ui, app: &mut InkApp) {
    let pal = app.pal;
    let mut open_hit: Option<(PathBuf, usize, usize)> = None;
    let mut run_search = false;
    let mut close = false;
    egui::Panel::bottom("bottom-panel").resizable(true).default_size(230.0).size_range(90.0..=600.0).frame(egui::Frame::new().fill(pal.chrome).inner_margin(egui::Margin::symmetric(10, 6))).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (b, label, icon) in [(Bottom::Search, "Find in Files", icons::FILE_SEARCH), (Bottom::Build, "Build Output", icons::TERMINAL)] {
                let on = app.bottom == b;
                let btn = egui::Button::new(egui::RichText::new(format!("{icon}  {label}")).size(12.5).color(if on { pal.text } else { pal.muted })).fill(if on { pal.active() } else { egui::Color32::TRANSPARENT }).corner_radius(6);
                if ui.add(btn).clicked() {
                    app.bottom = b;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(icons::X).frame_when_inactive(false)).clicked() {
                    close = true;
                }
                if app.bottom == Bottom::Build && app.build.running() && ui.button(format!("{}  Stop", icons::SQUARE)).clicked() {
                    app.build.cancel();
                }
            });
        });
        ui.add_space(4.0);
        match app.bottom {
            Bottom::Search => {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let toggle = |ui: &mut Ui, on: &mut bool, icon: &str, tip: &str| {
                        let b = egui::Button::new(egui::RichText::new(icon).size(14.0).color(if *on { pal.accent } else { pal.muted })).fill(if *on { pal.accent_soft() } else { egui::Color32::TRANSPARENT }).min_size(vec2(28.0, 26.0));
                        if ui.add(b).on_hover_text(tip).clicked() {
                            *on = !*on;
                        }
                    };
                    toggle(ui, &mut app.search.regex, icons::REGEX, "Regular expression");
                    toggle(ui, &mut app.search.case, icons::CASE_SENSITIVE, "Case sensitive");
                    toggle(ui, &mut app.search.word, icons::WHOLE_WORD, "Whole word");
                    let r = ui.add(egui::TextEdit::singleline(&mut app.search.query).hint_text("Find in files").desired_width(260.0).font(egui::TextStyle::Monospace));
                    if app.search.focus {
                        app.search.focus = false;
                        r.request_focus();
                    }
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        run_search = true;
                    }
                    let r2 = ui.add(egui::TextEdit::singleline(&mut app.search.replace).hint_text("Replace with").desired_width(200.0).font(egui::TextStyle::Monospace));
                    let _ = r2;
                    if ui.button("Find").clicked() {
                        run_search = true;
                    }
                    let can_replace = !app.search.results.is_empty() && !app.search.running() && app.search.files > 0;
                    if ui.add_enabled(can_replace, egui::Button::new("Replace All…")).clicked() {
                        app.search.confirm_replace = true;
                    }
                    let status = if let Some(e) = &app.search.error {
                        e.clone()
                    } else if app.search.running() {
                        format!("Searching… {} matches", app.search.results.len())
                    } else if app.search.results.is_empty() {
                        if app.sidebar.folder.is_none() { "Searches open files; open a folder to search a project".into() } else { String::new() }
                    } else {
                        format!("{} matches in {} files", app.search.results.len(), app.search.files.max(1))
                    };
                    ui.label(egui::RichText::new(status).size(12.0).color(if app.search.error.is_some() { pal.danger } else { pal.muted }));
                });
                if app.search.confirm_replace {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("{}  Replace {} matches in {} files on disk? This cannot be undone from here.", icons::TRIANGLE_ALERT, app.search.results.len(), app.search.files)).color(pal.warning).size(12.5));
                        if ui.button("Replace").clicked() {
                            app.search.confirm_replace = false;
                            replace_in_files(app);
                        }
                        if ui.button("Cancel").clicked() {
                            app.search.confirm_replace = false;
                        }
                    });
                }
                ui.add_space(4.0);
                let root = app.sidebar.folder.clone();
                let row_h = 22.0;
                let results = &app.search.results;
                egui::ScrollArea::vertical().id_salt("search-results").auto_shrink([false, false]).show_rows(ui, row_h, results.len(), |ui, range| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for i in range {
                        let h = &results[i];
                        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(rect, 4, pal.hover());
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        let rel = root.as_ref().and_then(|r| h.path.strip_prefix(r).ok()).unwrap_or(&h.path).to_string_lossy().replace('\\', "/");
                        let mut job = egui::text::LayoutJob::default();
                        job.wrap = egui::text::TextWrapping { max_width: rect.width() - 12.0, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
                        job.append(&format!("{rel}:{}", h.line + 1), 0.0, theme::fmt(12.0, pal.accent, 500.0));
                        job.append("   ", 0.0, theme::mono(12.0, pal.muted));
                        let lead = h.text.chars().take_while(|c| c.is_whitespace()).count();
                        let chars: Vec<char> = h.text.chars().collect();
                        let (a, z) = (h.col.max(lead).min(chars.len()), (h.col + h.len).min(chars.len()));
                        job.append(&chars[lead.min(a)..a].iter().collect::<String>(), 0.0, theme::mono(12.0, pal.muted));
                        job.append(&chars[a..z].iter().collect::<String>(), 0.0, egui::TextFormat { background: theme::with_alpha(pal.warning, 0.3), ..theme::mono(12.0, pal.text) });
                        job.append(&chars[z..].iter().collect::<String>(), 0.0, theme::mono(12.0, pal.muted));
                        let g = ui.fonts_mut(|f| f.layout_job(job));
                        ui.painter().galley(egui::pos2(rect.min.x + 6.0, rect.center().y - g.size().y / 2.0), g, pal.text);
                        if resp.clicked() {
                            open_hit = Some((h.path.clone(), h.line, h.col));
                        }
                    }
                });
            }
            Bottom::Build => {
                let cwd = app.build.cwd.clone();
                let re = regex::Regex::new(r#"((?:[A-Za-z]:)?[^\s:"'()<>|]+\.[A-Za-z0-9_]+)(?::|\(|, line )(\d+)(?:[:,](\d+))?"#).unwrap();
                let row_h = 19.0;
                let lines = &app.build.lines;
                egui::ScrollArea::both().id_salt("build-out").auto_shrink([false, false]).stick_to_bottom(true).show_rows(ui, row_h, lines.len(), |ui, range| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for i in range {
                        let line = &lines[i];
                        let lower = line.to_lowercase();
                        let color = if lower.starts_with("error") || lower.contains("error:") || lower.contains("traceback") { pal.danger } else if lower.starts_with("warning") || lower.contains("warning:") { pal.warning } else if line.starts_with('[') || line.starts_with('>') { pal.faint } else { pal.text };
                        let link = re.captures(line).and_then(|c| Some((c.get(1)?.as_str().to_string(), c.get(2)?.as_str().parse::<usize>().ok()?, c.get(3).and_then(|x| x.as_str().parse::<usize>().ok()).unwrap_or(1))));
                        let g = ui.fonts_mut(|f| f.layout_no_wrap(line.clone(), egui::FontId::monospace(12.0), color));
                        let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x.max(ui.available_width()), row_h), if link.is_some() { Sense::click() } else { Sense::hover() });
                        if link.is_some() && resp.hovered() {
                            ui.painter().rect_filled(rect, 3, pal.hover());
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        ui.painter().galley(egui::pos2(rect.min.x + 4.0, rect.center().y - g.size().y / 2.0), g, color);
                        if let (true, Some((file, l, c))) = (resp.clicked(), link) {
                            let p = PathBuf::from(&file);
                            let p = if p.is_absolute() { p } else { cwd.clone().unwrap_or_default().join(p) };
                            if p.is_file() {
                                open_hit = Some((p, l.saturating_sub(1), c.saturating_sub(1)));
                            }
                        }
                    }
                });
            }
            Bottom::None => {}
        }
    });
    if run_search {
        let scope = search_scope(app);
        app.search.start(scope);
    }
    if let Some((p, l, c)) = open_hit {
        app.open_at(&p, l, c);
    }
    if close {
        app.bottom = Bottom::None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions_and_search() {
        let re = definition_regex("render").unwrap();
        for s in ["fn render(x: u8) {", "    pub async fn render() {", "def render(self):", "export function render() {", "const render = (a) => a", "class render:", "func (s *Server) render() {", "## render"] {
            assert!(re.is_match(s), "{s}");
        }
        for s in ["render();", "let x = render(1);", "// fn renderer"] {
            assert!(!re.is_match(s), "{s}");
        }
        let dir = std::env::temp_dir().join(format!("ink-search-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("a.txt");
        std::fs::write(&f, "one\nTwo words\nthree two\n").unwrap();
        let mut hits = Vec::new();
        search_file(&f, &build_regex("two", false, false, true).unwrap(), &mut hits);
        assert_eq!(hits.iter().map(|h| (h.line, h.col)).collect::<Vec<_>>(), vec![(1, 0), (2, 6)]);
        let mut hits = Vec::new();
        search_file(&f, &build_regex("two", false, true, false).unwrap(), &mut hits);
        assert_eq!(hits.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(strip_ansi("\u{1b}[31merror\u{1b}[0m: x"), "error: x");
        assert!(build_systems(&Settings::default()).iter().any(|b| b.name == "Python"));
    }
}
