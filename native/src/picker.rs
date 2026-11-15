//! The fuzzy picker behind the Command Palette, Goto Anything (files, `:line`, `@symbol`,
//! `#word`) and the small choosers for theme, syntax, encoding, indentation and so on.

use std::path::PathBuf;

use egui::{vec2, Color32, Id, Key};

use crate::app::InkApp;
use crate::doc::ENCODINGS;
use crate::icons;
use crate::theme;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PickKind {
    Commands,
    Files,
    Symbols,
    Line,
    Words,
    Themes,
    Syntax,
    Encoding,
    ReopenEncoding,
    LineEndings,
    Indent,
    Recent,
    Templates,
    Snippets,
    Builds,
    /// Where to file a note.
    Notebooks,
}

#[derive(Clone)]
pub enum Act {
    Command(String),
    Open(PathBuf),
    Goto(usize, usize),
    Theme(&'static str),
    Syntax(String),
    Encoding(&'static str),
    Reopen(&'static str),
    Eol(bool),
    Indent(bool, usize),
    ConvertIndent(bool),
    Template(usize),
    Snippet(String),
    Build(usize),
    None,
}

pub struct Item {
    pub label: String,
    pub detail: String,
    pub icon: &'static str,
    pub act: Act,
}

pub struct Picker {
    pub kind: PickKind,
    base: PickKind,
    pub query: String,
    items: Vec<Item>,
    shown: Vec<(usize, Vec<usize>)>,
    selected: usize,
    focus: bool,
    last_query: Option<String>,
    original_theme: String,
    origin: Option<(Vec<crate::buffer::Sel>, f32)>,
    scroll_to: bool,
}

/// Whether a command can do anything on this platform. Menus and the palette leave out
/// the ones that cannot, so that every item shown does something.
pub fn available(cmd: &str) -> bool {
    let name = cmd.split(' ').next().unwrap_or(cmd);
    if cfg!(any(target_os = "android", target_os = "ios")) {
        // no folders outside the app, no second window or pane, no desktop programs to call
        const DESKTOP_ONLY: &[&str] = &[
            "open_folder", "close_folder", "new_window", "reveal_in_explorer", "toggle_fullscreen", "copy_path", "export_copy_rich", "export_gdoc", "integrations",
            "build", "build_with", "cancel_build", "layout_columns_2", "layout_single", "focus_group_1", "focus_group_2", "move_to_group_2", "toggle_minimap", "update_download", "update_install",
        ];
        if DESKTOP_ONLY.contains(&name) {
            return false;
        }
    }
    if name == "tool_set_default" {
        return cfg!(windows);
    }
    if name == "export_downloads" {
        return cfg!(target_os = "android");
    }
    true
}

pub const COMMANDS: &[(&str, &str, &str)] = &[
    ("new_file", "File: New Document", icons::FILE_PLUS),
    ("new_code_file", "File: New Text File", icons::FILE_PLUS),
    ("new_from_template", "File: New from Template…", icons::LAYOUT_TEMPLATE),
    ("open_file", "File: Open…", icons::FOLDER_OPEN),
    ("open_folder", "File: Open Folder…", icons::FOLDER),
    ("close_folder", "File: Close Folder", icons::FOLDER),
    ("open_recent", "File: Open Recent…", icons::HISTORY),
    ("save", "File: Save", icons::SAVE),
    ("save_as", "File: Save As…", icons::SAVE),
    ("save_all", "File: Save All", icons::SAVE),
    ("revert", "File: Revert to Saved", icons::REFRESH_CW),
    ("notes_file", "Notes: File in Notes…", icons::FOLDER),
    ("notes_file_to", "Notes: File in Inbox", icons::FOLDER),
    ("notes_file_all", "Notes: File All Drafts in Inbox", icons::FOLDER),
    ("notes_open", "Notes: Browse Notes", icons::FOLDER_OPEN),
    ("notes_new_notebook", "Notes: New Notebook…", icons::FILE_PLUS),
    ("close", "File: Close Tab", icons::X),
    ("close_others", "File: Close Other Tabs", icons::X),
    ("close_all", "File: Close All Tabs", icons::X),
    ("reopen_closed_file", "File: Reopen Closed Tab", icons::HISTORY),
    ("copy_path", "File: Copy Path", icons::COPY),
    ("reveal_in_explorer", "File: Show in File Manager", icons::EXTERNAL_LINK),
    ("new_window", "Window: New Window", icons::MONITOR),
    ("layout_columns_2", "Window: Split into Two Columns", icons::COLUMNS_2),
    ("layout_single", "Window: Single Pane", icons::SQUARE),
    ("focus_group_1", "Window: Focus Left Pane", icons::PANEL_LEFT),
    ("focus_group_2", "Window: Focus Right Pane", icons::PANEL_RIGHT),
    ("move_to_group_2", "Window: Move File to the Other Pane", icons::ARROW_RIGHT),
    ("toggle_fullscreen", "Window: Toggle Full Screen", icons::MAXIMIZE_2),
    ("mode read", "View: Read", icons::EYE),
    ("mode write", "View: Write (Word-style)", icons::PEN_LINE),
    ("mode split", "View: Split", icons::COLUMNS_2),
    ("mode code", "View: Code", icons::CODE),
    ("cycle_mode", "View: Next Mode", icons::REFRESH_CW),
    ("toggle_sidebar", "View: Toggle Sidebar", icons::PANEL_LEFT),
    ("toggle_minimap", "View: Toggle Minimap", icons::MAP),
    ("toggle_line_numbers", "View: Toggle Line Numbers", icons::HASH),
    ("toggle_word_wrap", "View: Toggle Word Wrap", icons::WRAP_TEXT),
    ("toggle_status_bar", "View: Toggle Status Bar", icons::PANEL_BOTTOM),
    ("increase_font_size", "View: Larger Text", icons::ZOOM_IN),
    ("decrease_font_size", "View: Smaller Text", icons::ZOOM_OUT),
    ("reset_font_size", "View: Reset Text Size", icons::TYPE),
    ("select_theme", "Preferences: Theme…", icons::PALETTE),
    ("open_settings", "Preferences: Settings", icons::SETTINGS),
    ("open_keybindings", "Preferences: Key Bindings", icons::KEYBOARD),
    ("open_snippets", "Preferences: Snippets", icons::ZAP),
    ("toggle_auto_save", "Preferences: Toggle AutoSave", icons::SAVE),
    ("integrations", "Preferences: Integrations (Google Drive)…", icons::PLUG),
    ("show_shortcuts", "Help: Keyboard Shortcuts", icons::KEYBOARD),
    ("welcome", "Help: Welcome Tour", icons::BOOK_OPEN),
    ("help_docs", "Help: Documentation", icons::BOOK_OPEN),
    ("help_release_notes", "Help: Release Notes", icons::HISTORY),
    ("help_report_issue", "Help: Report an Issue", icons::INFO),
    ("check_for_updates", "Help: Check for Updates…", icons::REFRESH_CW),
    ("about", "Help: About Inkdown", icons::INFO),
    ("goto_anything", "Goto: Anything…", icons::SEARCH),
    ("goto_symbol", "Goto: Symbol…", icons::AT_SIGN),
    ("goto_line", "Goto: Line…", icons::HASH),
    ("goto_word", "Goto: Word in File…", icons::TEXT_CURSOR),
    ("goto_definition", "Goto: Definition", icons::CORNER_DOWN_LEFT),
    ("move_to_bracket", "Goto: Matching Bracket", icons::BRACES),
    ("next_bookmark", "Bookmarks: Next", icons::BOOKMARK),
    ("prev_bookmark", "Bookmarks: Previous", icons::BOOKMARK),
    ("toggle_bookmark", "Bookmarks: Toggle", icons::BOOKMARK),
    ("clear_bookmarks", "Bookmarks: Clear All", icons::BOOKMARK),
    ("show_find", "Find: Find…", icons::SEARCH),
    ("show_replace", "Find: Replace…", icons::REPLACE),
    ("find_in_files", "Find: Find in Files…", icons::FILE_SEARCH),
    ("find_next", "Find: Next", icons::ARROW_DOWN),
    ("find_prev", "Find: Previous", icons::ARROW_UP),
    ("replace_all", "Find: Replace All", icons::REPLACE_ALL),
    ("select_all", "Selection: Select All", icons::TEXT_SELECT),
    ("expand_selection_to_line", "Selection: Expand to Line", icons::TEXT_SELECT),
    ("find_under_expand", "Selection: Add Next Occurrence", icons::TEXT_SELECT),
    ("find_all_under", "Selection: Select All Occurrences", icons::TEXT_SELECT),
    ("split_selection_into_lines", "Selection: Split into Lines", icons::TEXT_SELECT),
    ("add_cursor_above", "Selection: Add Cursor Above", icons::TEXT_CURSOR),
    ("add_cursor_below", "Selection: Add Cursor Below", icons::TEXT_CURSOR),
    ("expand_selection_to_brackets", "Selection: Expand to Brackets", icons::BRACES),
    ("expand_selection_to_indentation", "Selection: Expand to Indentation", icons::INDENT_INCREASE),
    ("expand_selection", "Selection: Expand", icons::TEXT_SELECT),
    ("undo", "Edit: Undo", icons::UNDO_2),
    ("redo", "Edit: Redo", icons::REDO_2),
    ("duplicate_line", "Edit: Duplicate Line", icons::COPY),
    ("delete_line", "Edit: Delete Line", icons::TRASH_2),
    ("swap_line_up", "Edit: Move Line Up", icons::ARROW_UP),
    ("swap_line_down", "Edit: Move Line Down", icons::ARROW_DOWN),
    ("join_lines", "Edit: Join Lines", icons::WRAP_TEXT),
    ("toggle_comment", "Edit: Toggle Comment", icons::CODE),
    ("toggle_block_comment", "Edit: Toggle Block Comment", icons::CODE),
    ("indent", "Edit: Indent", icons::INDENT_INCREASE),
    ("unindent", "Edit: Unindent", icons::INDENT_DECREASE),
    ("transpose", "Edit: Transpose", icons::REFRESH_CW),
    ("upper_case", "Edit: Convert to UPPER CASE", icons::TYPE),
    ("lower_case", "Edit: Convert to lower case", icons::TYPE),
    ("title_case", "Edit: Convert to Title Case", icons::TYPE),
    ("sort_lines", "Edit: Sort Lines", icons::LIST),
    ("sort_lines_case_insensitive", "Edit: Sort Lines (Ignore Case)", icons::LIST),
    ("reverse_lines", "Edit: Reverse Lines", icons::LIST),
    ("unique_lines", "Edit: Remove Duplicate Lines", icons::LIST),
    ("trim_trailing_white_space", "Edit: Trim Trailing Spaces", icons::ERASER),
    ("insert_snippet", "Edit: Insert Snippet…", icons::ZAP),
    ("auto_complete", "Edit: Show Completions", icons::ZAP),
    ("fold", "Code Folding: Fold", icons::CHEVRON_RIGHT),
    ("unfold", "Code Folding: Unfold", icons::CHEVRON_DOWN),
    ("fold_all", "Code Folding: Fold All", icons::CHEVRON_RIGHT),
    ("unfold_all", "Code Folding: Unfold All", icons::CHEVRON_DOWN),
    ("set_syntax", "Document: Set Syntax…", icons::FILE_CODE),
    ("set_encoding", "Document: Save with Encoding…", icons::LANGUAGES),
    ("reopen_with_encoding", "Document: Reopen with Encoding…", icons::LANGUAGES),
    ("set_line_endings", "Document: Line Endings…", icons::CORNER_DOWN_LEFT),
    ("set_indentation", "Document: Indentation…", icons::INDENT_INCREASE),
    ("build", "Build: Run", icons::PLAY),
    ("build_with", "Build: Run With…", icons::HAMMER),
    ("cancel_build", "Build: Cancel", icons::SQUARE),
    ("toggle_record_macro", "Macro: Start / Stop Recording", icons::CIRCLE),
    ("run_macro", "Macro: Play Back", icons::PLAY),
    ("export_downloads", "Export: Save a Copy to Downloads", icons::FILE_DOWN),
    ("export_html", "Export: HTML…", icons::FILE_DOWN),
    ("export_pdf", "Export: PDF…", icons::FILE_DOWN),
    ("export_docx", "Export: Word (.docx)…", icons::FILE_DOWN),
    ("export_rtf", "Export: Rich Text (.rtf)…", icons::FILE_DOWN),
    ("export_copy_rich", "Export: Copy as Rich Text", icons::CLIPBOARD),
    ("export_copy_html", "Export: Copy as HTML", icons::CLIPBOARD),
    ("export_gdoc", "Export: Upload to Google Docs", icons::CLOUD_UPLOAD),
    ("export_png", "Export: Diagram as PNG…", icons::FILE_IMAGE),
    ("export_svg", "Export: Diagram as SVG…", icons::FILE_IMAGE),
    ("tool_word_count", "Tools: Word Count", icons::WRENCH),
    ("tool_format_tables", "Tools: Align Markdown Tables", icons::TABLE),
    ("tool_toc", "Tools: Insert Table of Contents", icons::LIST_TREE),
    ("tool_renumber", "Tools: Renumber Ordered Lists", icons::LIST_ORDERED),
    ("tool_smart_quotes", "Tools: Straighten Quotes", icons::QUOTE),
    ("tool_json_format", "Tools: Format JSON / YAML / TOML", icons::BRACES),
    ("tool_to_json", "Tools: Convert to JSON", icons::FILE_JSON),
    ("tool_to_yaml", "Tools: Convert to YAML", icons::FILE_JSON),
    ("tool_to_toml", "Tools: Convert to TOML", icons::FILE_JSON),
    ("tool_csv_to_table", "Tools: CSV to Markdown Table", icons::TABLE),
    ("tool_avro_to_json", "Tools: Avro Records to a JSON Document", icons::FILE_JSON),
    ("tool_avro_schema", "Tools: Avro Schema to a Document (.avsc)", icons::FILE_JSON),
    ("tool_base64_encode", "Tools: Base64 Encode Selection", icons::LOCK),
    ("tool_base64_decode", "Tools: Base64 Decode Selection", icons::LOCK),
    ("tool_url_encode", "Tools: URL Encode Selection", icons::LINK),
    ("tool_url_decode", "Tools: URL Decode Selection", icons::LINK),
    ("tool_insert_date", "Tools: Insert Date", icons::HISTORY),
    ("tool_insert_uuid", "Tools: Insert UUID", icons::HASH),
    ("tool_set_default", "Tools: Make Inkdown the Default Markdown App…", icons::STAR),
];

/// Subsequence match with bonuses for word starts and runs. Higher is better.
pub fn fuzzy(query: &[char], text: &str) -> Option<(i32, Vec<usize>)> {
    if query.is_empty() {
        return Some((0, Vec::new()));
    }
    let chars: Vec<char> = text.chars().collect();
    let lower: Vec<char> = chars.iter().flat_map(|c| c.to_lowercase().take(1)).collect();
    if lower.len() < query.len() {
        return None;
    }
    let mut pos = Vec::with_capacity(query.len());
    let mut score = 0i32;
    let mut ti = 0usize;
    let mut prev: Option<usize> = None;
    for q in query {
        // prefer a word-start match if one exists ahead, otherwise the next occurrence
        let mut found = None;
        let mut k = ti;
        while k < lower.len() {
            if lower[k] == *q {
                let boundary = k == 0 || !chars[k - 1].is_alphanumeric() || (chars[k].is_uppercase() && chars[k - 1].is_lowercase());
                if found.is_none() {
                    found = Some((k, boundary));
                }
                if boundary || prev == Some(k.wrapping_sub(1)) {
                    found = Some((k, boundary));
                    break;
                }
                if k > ti + 24 {
                    break;
                }
            }
            k += 1;
        }
        let (at, boundary) = found?;
        score += 10;
        if boundary {
            score += 12;
        }
        if prev == Some(at.wrapping_sub(1)) {
            score += 14;
        }
        if let Some(p) = prev {
            score -= ((at - p) as i32 - 1).min(8);
        } else {
            score -= (at as i32).min(12);
        }
        pos.push(at);
        prev = Some(at);
        ti = at + 1;
    }
    score -= (lower.len() as i32 / 8).min(12);
    Some((score, pos))
}

impl Picker {
    pub fn new(kind: PickKind, app: &mut InkApp) -> Self {
        let origin = app.docs.get(app.active).map(|d| (d.buf.sels.clone(), d.ed.scroll.y));
        let mut p = Self { kind, base: kind, query: String::new(), items: Vec::new(), shown: Vec::new(), selected: 0, focus: true, last_query: None, original_theme: app.set.theme.clone(), origin, scroll_to: false };
        match kind {
            PickKind::Symbols => p.query = "@".into(),
            PickKind::Line => p.query = ":".into(),
            PickKind::Words => p.query = "#".into(),
            _ => {}
        }
        if matches!(kind, PickKind::Symbols | PickKind::Line | PickKind::Words) {
            p.base = PickKind::Files;
        }
        p.load(app);
        p
    }

    pub fn visible_labels(&self, n: usize) -> Vec<String> {
        self.shown.iter().take(n).map(|(i, _)| self.items[*i].label.clone()).collect()
    }

    fn effective(&self) -> (PickKind, &str) {
        if self.base == PickKind::Files {
            if let Some(r) = self.query.strip_prefix('@') {
                return (PickKind::Symbols, r);
            }
            if let Some(r) = self.query.strip_prefix(':') {
                return (PickKind::Line, r);
            }
            if let Some(r) = self.query.strip_prefix('#') {
                return (PickKind::Words, r);
            }
            if let Some(r) = self.query.strip_prefix('>') {
                return (PickKind::Commands, r);
            }
            return (PickKind::Files, &self.query);
        }
        (self.base, &self.query)
    }

    fn load(&mut self, app: &mut InkApp) {
        let (kind, _) = self.effective();
        self.kind = kind;
        self.items.clear();
        match kind {
            PickKind::Commands => {
                for (id, title, icon) in COMMANDS.iter().filter(|c| available(c.0)) {
                    let key = app.keymap.label(id.split(' ').next().unwrap_or(id));
                    self.items.push(Item { label: title.to_string(), detail: key, icon, act: Act::Command(id.to_string()) });
                }
            }
            PickKind::Files => {
                let mut seen = std::collections::HashSet::new();
                for d in &app.docs {
                    if let Some(p) = &d.path {
                        if seen.insert(p.clone()) {
                            self.items.push(Item { label: d.name.clone(), detail: "open".into(), icon: icons::FILE_TEXT, act: Act::Open(p.clone()) });
                        }
                    }
                }
                let root = app.sidebar.folder.clone();
                if let Some(index) = app.sidebar.index() {
                    for p in index.iter() {
                        if seen.insert(p.clone()) {
                            let rel = root.as_ref().and_then(|r| p.strip_prefix(r).ok()).unwrap_or(p).to_string_lossy().replace('\\', "/");
                            self.items.push(Item { label: rel, detail: String::new(), icon: icons::FILE, act: Act::Open(p.clone()) });
                        }
                    }
                }
                for r in &app.set.recent_files {
                    let p = PathBuf::from(r);
                    if seen.insert(p.clone()) {
                        self.items.push(Item { label: p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), detail: p.parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default(), icon: icons::HISTORY, act: Act::Open(p) });
                    }
                }
            }
            PickKind::Recent => {
                for r in &app.set.recent_files {
                    let p = PathBuf::from(r);
                    self.items.push(Item { label: p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), detail: p.parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default(), icon: icons::FILE_TEXT, act: Act::Open(p) });
                }
                for r in &app.set.recent_folders {
                    self.items.push(Item { label: r.clone(), detail: "folder".into(), icon: icons::FOLDER, act: Act::Open(PathBuf::from(r)) });
                }
            }
            PickKind::Symbols => {
                if let Some(d) = app.docs.get(app.active) {
                    let from_outline = matches!(d.kind, crate::doc::Kind::Markdown) && !d.view.md.outline.is_empty();
                    if from_outline {
                        for (level, text, _, line) in &d.view.md.outline {
                            self.items.push(Item { label: format!("{}{}", "  ".repeat((*level as usize).saturating_sub(1)), text), detail: format!("H{level}"), icon: icons::HASH, act: Act::Goto(line.saturating_sub(1), 0) });
                        }
                    } else {
                        for (line, name) in d.hl.symbols(&d.buf, std::time::Duration::from_millis(600)) {
                            self.items.push(Item { label: name, detail: format!(":{}", line + 1), icon: icons::AT_SIGN, act: Act::Goto(line, 0) });
                        }
                    }
                }
            }
            PickKind::Words => {
                if let Some(d) = app.docs.get(app.active) {
                    let text: String = d.buf.rope.slice(..d.buf.len().min(2_000_000)).to_string();
                    let mut seen = std::collections::HashSet::new();
                    for (ln, line) in text.lines().enumerate() {
                        let mut start = None;
                        for (i, c) in line.char_indices().chain(std::iter::once((line.len(), ' '))) {
                            if crate::buffer::is_word(c) {
                                start.get_or_insert(i);
                            } else if let Some(s) = start.take() {
                                let w = &line[s..i];
                                if w.len() >= 3 && seen.insert(w.to_string()) {
                                    self.items.push(Item { label: w.to_string(), detail: format!(":{}", ln + 1), icon: icons::TEXT_CURSOR, act: Act::Goto(ln, line[..s].chars().count()) });
                                }
                            }
                        }
                        if self.items.len() > 30_000 {
                            break;
                        }
                    }
                }
            }
            PickKind::Line => {}
            PickKind::Themes => {
                self.items.push(Item { label: "Follow system".into(), detail: "light / dark".into(), icon: icons::MONITOR, act: Act::Theme("system") });
                for p in theme::PALETTES {
                    self.items.push(Item { label: p.name.to_string(), detail: if p.dark { "dark".into() } else { "light".into() }, icon: if p.dark { icons::MOON } else { icons::SUN }, act: Act::Theme(p.id) });
                }
                self.selected = self.items.iter().position(|i| matches!(i.act, Act::Theme(id) if id == app.set.theme)).unwrap_or(0);
            }
            PickKind::Syntax => {
                for n in crate::syntax::names() {
                    self.items.push(Item { label: n.clone(), detail: String::new(), icon: icons::FILE_CODE, act: Act::Syntax(n) });
                }
            }
            PickKind::Encoding | PickKind::ReopenEncoding => {
                for (label, id) in ENCODINGS {
                    self.items.push(Item { label: label.to_string(), detail: id.to_string(), icon: icons::LANGUAGES, act: if kind == PickKind::Encoding { Act::Encoding(id) } else { Act::Reopen(id) } });
                }
            }
            PickKind::LineEndings => {
                self.items.push(Item { label: "Unix (LF)".into(), detail: "\\n".into(), icon: icons::CORNER_DOWN_LEFT, act: Act::Eol(false) });
                self.items.push(Item { label: "Windows (CRLF)".into(), detail: "\\r\\n".into(), icon: icons::CORNER_DOWN_LEFT, act: Act::Eol(true) });
            }
            PickKind::Indent => {
                for w in [2usize, 4, 8] {
                    self.items.push(Item { label: format!("Spaces: {w}"), detail: String::new(), icon: icons::INDENT_INCREASE, act: Act::Indent(true, w) });
                }
                for w in [2usize, 4, 8] {
                    self.items.push(Item { label: format!("Tabs, width {w}"), detail: String::new(), icon: icons::INDENT_INCREASE, act: Act::Indent(false, w) });
                }
                self.items.push(Item { label: "Convert indentation to spaces".into(), detail: String::new(), icon: icons::REFRESH_CW, act: Act::ConvertIndent(true) });
                self.items.push(Item { label: "Convert indentation to tabs".into(), detail: String::new(), icon: icons::REFRESH_CW, act: Act::ConvertIndent(false) });
            }
            PickKind::Templates => {
                for (i, (name, file, _)) in crate::snippets::TEMPLATES.iter().enumerate() {
                    self.items.push(Item { label: name.to_string(), detail: file.to_string(), icon: icons::LAYOUT_TEMPLATE, act: Act::Template(i) });
                }
            }
            PickKind::Snippets => {
                for (trigger, body) in app.snippets_for(app.active).iter() {
                    self.items.push(Item { label: trigger.clone(), detail: body.lines().next().unwrap_or("").chars().take(60).collect(), icon: icons::ZAP, act: Act::Snippet(body.clone()) });
                }
            }
            PickKind::Notebooks => {
                for n in crate::notes::notebooks(app) {
                    self.items.push(Item { label: n.clone(), detail: "notebook".into(), icon: icons::FOLDER, act: Act::Command(format!("notes_file_to {n}")) });
                }
                self.items.push(Item { label: "New notebook…".into(), detail: String::new(), icon: icons::FILE_PLUS, act: Act::Command("notes_new_notebook".into()) });
            }
            PickKind::Builds => {
                for (i, b) in crate::project::build_systems(&app.set).iter().enumerate() {
                    self.items.push(Item { label: b.name.clone(), detail: b.cmd.join(" "), icon: icons::HAMMER, act: Act::Build(i) });
                }
            }
        }
        self.last_query = None;
    }

    fn filter(&mut self) {
        let (kind, q) = self.effective();
        let q = q.trim().to_string();
        if self.last_query.as_deref() == Some(q.as_str()) {
            return;
        }
        self.last_query = Some(q.clone());
        let keep_sel = kind == PickKind::Themes && q.is_empty();
        let query: Vec<char> = q.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
        let mut scored: Vec<(i32, usize, Vec<usize>)> = Vec::new();
        for (i, item) in self.items.iter().enumerate() {
            if let Some((mut s, pos)) = fuzzy(&query, &item.label) {
                if kind == PickKind::Files && !query.is_empty() {
                    // matches in the file name beat matches in the folder part
                    let name_start = item.label.rfind('/').map(|x| item.label[..x + 1].chars().count()).unwrap_or(0);
                    if pos.first().is_some_and(|p| *p >= name_start) {
                        s += 20;
                    }
                    if item.detail == "open" {
                        s += 6;
                    }
                }
                scored.push((s, i, pos));
            }
        }
        if !query.is_empty() {
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        }
        scored.truncate(400);
        self.shown = scored.into_iter().map(|(_, i, p)| (i, p)).collect();
        if !keep_sel {
            self.selected = 0;
        } else {
            self.selected = self.shown.iter().position(|(i, _)| *i == self.selected).unwrap_or(0);
        }
        self.scroll_to = true;
    }
}

fn apply(app: &mut InkApp, ctx: &egui::Context, act: &Act, preview: bool) {
    let now = ctx.input(|i| i.time);
    match act {
        Act::Command(c) if !preview => app.pending.push(c.clone()),
        Act::Open(p) if !preview => {
            app.open_path(p);
        }
        Act::Goto(line, col) => {
            if let Some(d) = app.doc() {
                d.ed.goto(&mut d.buf, *line, *col);
                if !preview {
                    if d.mode == crate::doc::Mode::Read && d.kind == crate::doc::Kind::Markdown {
                        d.view.md.goto_line = Some(line + 1);
                    } else {
                        d.ed.focus = true;
                    }
                }
            }
        }
        Act::Theme(id) => app.set_theme(id),
        Act::Syntax(n) if !preview => {
            if let Some(d) = app.doc() {
                d.set_syntax(n);
            }
        }
        Act::Encoding(id) if !preview => {
            if let Some(d) = app.doc() {
                let (name, bom) = match *id {
                    "utf-8-bom" => ("utf-8", true),
                    other => (other, other.starts_with("utf-16")),
                };
                if let Some(e) = encoding_rs::Encoding::for_label(name.as_bytes()) {
                    d.encoding = e;
                    d.bom = bom;
                    d.edited_at = Some(now);
                    let label = e.name();
                    app.toast(format!("Will save as {label}"));
                }
            }
        }
        Act::Reopen(id) if !preview => {
            let name = if *id == "utf-8-bom" { "utf-8" } else { id };
            if let (Some(e), Some(d)) = (encoding_rs::Encoding::for_label(name.as_bytes()), app.docs.get_mut(app.active)) {
                if let Some(bytes) = d.path.as_ref().and_then(|p| std::fs::read(p).ok()) {
                    let (text, _, _) = e.decode(&bytes);
                    d.buf.set_text(&text.replace("\r\n", "\n"));
                    d.buf.mark_saved();
                    d.encoding = e;
                }
            }
        }
        Act::Eol(crlf) if !preview => {
            if let Some(d) = app.doc() {
                if d.crlf != *crlf {
                    d.crlf = *crlf;
                    // the text itself is unchanged; mark it so the next save rewrites the file
                    let n = d.buf.len();
                    let sels = d.buf.sels.clone();
                    d.buf.edit(vec![crate::buffer::Edit::new(n, n, " ")], crate::buffer::EditKind::Other);
                    d.buf.edit(vec![crate::buffer::Edit::new(n, n + 1, "")], crate::buffer::EditKind::Other);
                    d.buf.sels = sels;
                    d.buf.commit_sels();
                    d.edited_at = Some(now);
                }
            }
        }
        Act::Indent(soft, w) if !preview => {
            if let Some(d) = app.doc() {
                d.cfg.soft_tabs = *soft;
                d.cfg.tab_width = *w;
            }
        }
        Act::ConvertIndent(to_spaces) if !preview => {
            if let Some(d) = app.doc() {
                crate::edit::exec(&mut d.buf, if *to_spaces { crate::edit::Cmd::TabsToSpaces } else { crate::edit::Cmd::SpacesToTabs }, &d.cfg);
                d.cfg.soft_tabs = *to_spaces;
                d.edited_at = Some(now);
            }
        }
        Act::Template(i) if !preview => {
            let (_, file, body) = crate::snippets::TEMPLATES[*i];
            app.new_file(Some(file.to_string()), body);
            if file.ends_with(".svg") {
                if let Some(d) = app.doc() {
                    d.mode = crate::doc::Mode::Write;
                }
            }
        }
        Act::Snippet(body) if !preview => {
            if let Some(d) = app.doc() {
                d.ed.start_snippet(&mut d.buf, body, 0, &d.cfg);
                d.edited_at = Some(now);
                d.ed.focus = true;
            }
        }
        Act::Build(i) if !preview => crate::project::build(app, Some(*i)),
        _ => {}
    }
}

pub fn show(ctx: &egui::Context, app: &mut InkApp) {
    let Some(mut p) = app.picker.take() else { return };
    let pal = app.pal;
    let before_kind = p.kind;
    if p.effective().0 != before_kind {
        p.load(app);
    }
    p.filter();
    let (kind, rest) = p.effective();
    let rest = rest.trim().to_string();

    let mut close = false;
    let mut accept = false;
    let (up, down, enter, esc, pg_up, pg_dn) = ctx.input(|i| (i.key_pressed(Key::ArrowUp), i.key_pressed(Key::ArrowDown), i.key_pressed(Key::Enter), i.key_pressed(Key::Escape), i.key_pressed(Key::PageUp), i.key_pressed(Key::PageDown)));
    let n = p.shown.len();
    let before = p.selected;
    if n > 0 {
        if down {
            p.selected = (p.selected + 1) % n;
        }
        if up {
            p.selected = (p.selected + n - 1) % n;
        }
        if pg_dn {
            p.selected = (p.selected + 10).min(n - 1);
        }
        if pg_up {
            p.selected = p.selected.saturating_sub(10);
        }
    }
    if p.selected != before {
        p.scroll_to = true;
    }
    if esc {
        close = true;
    }
    if enter {
        accept = true;
    }

    let screen = ctx.content_rect();
    let width = (screen.width() - 32.0).min(640.0);
    let mut clicked = None;
    let area = egui::Area::new(Id::new("ink-picker")).order(egui::Order::Foreground).fixed_pos(egui::pos2(screen.center().x - width / 2.0, screen.min.y + (screen.height() * 0.12).min(90.0)));
    // dim the window behind
    ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, Id::new("ink-picker-dim"))).rect_filled(screen, 0, Color32::from_black_alpha(if pal.dark { 110 } else { 50 }));
    let resp = area.show(ctx, |ui| {
        egui::Frame::popup(ui.style()).fill(pal.elev).corner_radius(12).inner_margin(8).show(ui, |ui| {
            ui.set_width(width - 16.0);
            let hint = match p.base {
                PickKind::Commands => "Type a command",
                PickKind::Files => "Go to file   :line   @symbol   #word   >command",
                PickKind::Themes => "Choose a theme",
                PickKind::Syntax => "Set syntax",
                PickKind::Templates => "New from template",
                PickKind::Recent => "Open recent",
                PickKind::Notebooks => "File this note in",
                _ => "Type to filter",
            };
            let edit = ui.add(egui::TextEdit::singleline(&mut p.query).hint_text(hint).desired_width(f32::INFINITY).font(egui::FontId::proportional(15.5)).margin(egui::Margin::symmetric(10, 9)).frame(egui::Frame::new()));
            if p.focus {
                edit.request_focus();
                p.focus = false;
                // put the caret after any prefix
                if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), edit.id) {
                    let end = egui::text::CCursor::new(p.query.chars().count());
                    st.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                    st.store(ui.ctx(), edit.id);
                }
            }
            if !edit.has_focus() && !close {
                edit.request_focus();
            }
            ui.add(egui::Separator::default().spacing(6.0));
            if kind == PickKind::Line {
                let msg = match rest.split(':').next().and_then(|l| l.trim().parse::<usize>().ok()) {
                    Some(l) => format!("Go to line {l}"),
                    None => "Type a line number, optionally :column".to_string(),
                };
                ui.label(egui::RichText::new(msg).color(pal.muted).size(13.0));
                return;
            }
            if p.shown.is_empty() {
                let msg = if kind == PickKind::Files && p.items.is_empty() { "Open a folder (Ctrl+K Ctrl+O) to search its files" } else { "No matches" };
                ui.add_space(4.0);
                ui.label(egui::RichText::new(msg).color(pal.muted).size(13.0));
                ui.add_space(4.0);
                return;
            }
            let row_h = 32.0;
            egui::ScrollArea::vertical().max_height(row_h * 11.5).auto_shrink([false, true]).show_rows(ui, row_h, p.shown.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for vi in range {
                    let (ii, pos) = &p.shown[vi];
                    let item = &p.items[*ii];
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), egui::Sense::click());
                    let sel = vi == p.selected;
                    if sel || resp.hovered() {
                        ui.painter().rect_filled(rect.shrink2(vec2(0.0, 1.0)), 7, if sel { pal.accent_soft() } else { pal.hover() });
                    }
                    if sel && p.scroll_to {
                        ui.scroll_to_rect(rect, None);
                    }
                    let icon = theme::text(ui, item.icon, 14.0, if sel { pal.accent } else { pal.muted }, 400.0);
                    ui.painter().galley(egui::pos2(rect.min.x + 10.0, rect.center().y - icon.size().y / 2.0), icon, pal.muted);
                    // label with the matched characters emphasised
                    let mut job = egui::text::LayoutJob::default();
                    job.wrap = egui::text::TextWrapping { max_width: rect.width() - 190.0, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
                    let mut pi = 0;
                    let mut run = String::new();
                    let mut run_hit = false;
                    for (ci, ch) in item.label.chars().enumerate() {
                        while pi < pos.len() && pos[pi] < ci {
                            pi += 1;
                        }
                        let hit = pi < pos.len() && pos[pi] == ci;
                        if hit != run_hit && !run.is_empty() {
                            job.append(&run, 0.0, theme::fmt(13.5, if run_hit { pal.accent } else { pal.text }, if run_hit { 700.0 } else { 430.0 }));
                            run.clear();
                        }
                        run_hit = hit;
                        run.push(ch);
                    }
                    if !run.is_empty() {
                        job.append(&run, 0.0, theme::fmt(13.5, if run_hit { pal.accent } else { pal.text }, if run_hit { 700.0 } else { 430.0 }));
                    }
                    let g = ui.fonts_mut(|f| f.layout_job(job));
                    ui.painter().galley(egui::pos2(rect.min.x + 36.0, rect.center().y - g.size().y / 2.0), g, pal.text);
                    if !item.detail.is_empty() {
                        let mut dj = egui::text::LayoutJob::single_section(item.detail.clone(), theme::fmt(11.5, pal.faint, 450.0));
                        dj.wrap = egui::text::TextWrapping { max_width: 170.0, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
                        let d = ui.fonts_mut(|f| f.layout_job(dj));
                        ui.painter().galley(egui::pos2(rect.max.x - d.size().x - 10.0, rect.center().y - d.size().y / 2.0), d, pal.faint);
                    }
                    if resp.clicked() {
                        clicked = Some(vi);
                    }
                }
            });
            p.scroll_to = false;
        });
    });
    if let Some(vi) = clicked {
        p.selected = vi;
        accept = true;
    }
    // click outside closes
    if ctx.input(|i| i.pointer.any_pressed()) && !resp.response.contains_pointer() {
        close = true;
    }

    // live preview
    if kind == PickKind::Line {
        let mut parts = rest.split(':');
        if let Some(l) = parts.next().and_then(|l| l.trim().parse::<usize>().ok()) {
            let col = parts.next().and_then(|c| c.trim().parse::<usize>().ok()).unwrap_or(1);
            apply(app, ctx, &Act::Goto(l.saturating_sub(1), col.saturating_sub(1)), !accept);
        }
    } else if matches!(kind, PickKind::Themes | PickKind::Symbols | PickKind::Words) && (p.selected != before || p.last_query.is_some()) {
        if let Some((ii, _)) = p.shown.get(p.selected) {
            let act = p.items[*ii].act.clone();
            apply(app, ctx, &act, true);
        }
    }

    if accept {
        if kind != PickKind::Line {
            if let Some((ii, _)) = p.shown.get(p.selected) {
                let act = p.items[*ii].act.clone();
                apply(app, ctx, &act, false);
            }
        }
        if let Some(d) = app.doc() {
            d.ed.focus = d.mode != crate::doc::Mode::Read;
        }
        return;
    }
    if close {
        // undo any preview
        if matches!(kind, PickKind::Themes) {
            let t = p.original_theme.clone();
            app.set_theme(&t);
        }
        if matches!(kind, PickKind::Line | PickKind::Symbols | PickKind::Words) {
            if let (Some((sels, scroll)), Some(d)) = (p.origin.take(), app.docs.get_mut(app.active)) {
                d.buf.sels = sels;
                d.buf.normalize();
                d.ed.scroll.y = scroll;
            }
        }
        if let Some(d) = app.doc() {
            d.ed.focus = d.mode != crate::doc::Mode::Read;
        }
        return;
    }
    app.picker = Some(p);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_ranks() {
        let q: Vec<char> = "sf".chars().collect();
        let a = fuzzy(&q, "save_file.rs").unwrap().0;
        let b = fuzzy(&q, "transfer.rs").unwrap().0;
        assert!(a > b, "{a} vs {b}");
        assert!(fuzzy(&"xyz".chars().collect::<Vec<_>>(), "abc").is_none());
        let q: Vec<char> = "cmdpal".chars().collect();
        assert!(fuzzy(&q, "Command Palette").is_some());
        // every command id is unique
        let mut ids: Vec<&str> = COMMANDS.iter().map(|c| c.0).collect();
        ids.sort();
        let n = ids.len();
        ids.dedup();
        assert_eq!(n, ids.len());
    }
}
