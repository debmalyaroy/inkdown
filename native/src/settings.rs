//! User settings, key bindings and the saved session. Plain JSON files in the config folder,
//! editable from inside the app (Preferences: Settings).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Settings folder. `INKDOWN_DATA_DIR` gives a fully separate profile (portable use, tests).
pub fn config_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("INKDOWN_DATA_DIR") {
        return PathBuf::from(d).join("config");
    }
    #[cfg(windows)]
    {
        PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join("Inkdown")
    }
    #[cfg(target_os = "macos")]
    {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Library/Application Support/Inkdown")
    }
    #[cfg(target_os = "android")]
    {
        std::env::var_os("INKDOWN_ANDROID_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
    }
    #[cfg(target_os = "ios")]
    {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Documents/.inkdown")
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "android", target_os = "ios")))]
    {
        match std::env::var_os("XDG_CONFIG_HOME") {
            Some(x) => PathBuf::from(x).join("inkdown"),
            None => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config/inkdown"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Palette id, or "system" to follow the OS light/dark setting.
    pub theme: String,
    pub font_size: f32,
    pub ui_scale: f32,
    pub tab_size: usize,
    pub translate_tabs_to_spaces: bool,
    pub detect_indentation: bool,
    /// "auto" wraps prose (Markdown, text) and not code.
    pub word_wrap: String,
    pub line_numbers: bool,
    pub minimap: bool,
    pub highlight_line: bool,
    pub indent_guides: bool,
    pub rulers: Vec<usize>,
    pub auto_save: bool,
    pub auto_save_delay_ms: u64,
    pub auto_match_enabled: bool,
    pub auto_indent: bool,
    pub auto_complete: bool,
    pub trim_trailing_white_space_on_save: bool,
    pub ensure_newline_at_eof_on_save: bool,
    pub hot_exit: bool,
    pub show_sidebar: bool,
    pub show_status_bar: bool,
    /// Reading layout: "sans" or "serif", and the column width in points.
    pub document_font: String,
    pub document_width: f32,
    pub document_font_size: f32,
    /// Default mode for Markdown: "read", "write", "split" or "code".
    pub markdown_mode: String,
    pub recent_files: Vec<String>,
    pub recent_folders: Vec<String>,
    /// Extra build systems: `{ "name": .., "selector": ["rs"], "cmd": ["cargo", "run"] }`.
    pub build_systems: Vec<BuildSystem>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct BuildSystem {
    pub name: String,
    /// File extensions this applies to.
    pub selector: Vec<String>,
    /// Command and arguments. `$file`, `$file_name`, `$file_base_name`, `$file_path`, `$folder` are substituted.
    pub cmd: Vec<String>,
    pub working_dir: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            font_size: 14.0,
            ui_scale: 1.0,
            tab_size: 4,
            translate_tabs_to_spaces: true,
            detect_indentation: true,
            word_wrap: "auto".into(),
            line_numbers: true,
            minimap: true,
            highlight_line: true,
            indent_guides: true,
            rulers: Vec::new(),
            auto_save: false,
            auto_save_delay_ms: 1200,
            auto_match_enabled: true,
            auto_indent: true,
            auto_complete: true,
            trim_trailing_white_space_on_save: false,
            ensure_newline_at_eof_on_save: false,
            hot_exit: true,
            show_sidebar: false,
            show_status_bar: true,
            document_font: "sans".into(),
            document_width: 820.0,
            document_font_size: 16.0,
            markdown_mode: "read".into(),
            recent_files: Vec::new(),
            recent_folders: Vec::new(),
            build_systems: Vec::new(),
        }
    }
}

pub fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}
pub fn keymap_path() -> PathBuf {
    config_dir().join("keybindings.json")
}
pub fn snippets_path() -> PathBuf {
    config_dir().join("snippets.json")
}
pub fn session_path() -> PathBuf {
    config_dir().join("session.json")
}

impl Settings {
    pub fn load() -> Self {
        let Ok(text) = std::fs::read_to_string(settings_path()) else { return Self::default() };
        Self::parse(&text).unwrap_or_default()
    }

    /// Accepts JSON with `//` comments, and the web build's camelCase keys.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut v: serde_json::Value = serde_json::from_str(&strip_json_comments(text)).map_err(|e| e.to_string())?;
        if let Some(o) = v.as_object_mut() {
            for (old, new) in [("lineNumbers", "line_numbers"), ("autoSave", "auto_save"), ("fontSize", "font_size"), ("recent", "recent_files"), ("wrap", "word_wrap"), ("sidebar", "show_sidebar")] {
                if let Some(x) = o.remove(old) {
                    o.entry(new.to_string()).or_insert(x);
                }
            }
            if let Some(w) = o.get_mut("word_wrap") {
                if let Some(b) = w.as_bool() {
                    *w = serde_json::Value::String(if b { "true" } else { "false" }.into());
                }
            }
        }
        // ignore fields of the wrong type instead of rejecting the whole file
        let mut s = serde_json::to_value(Self::default()).unwrap();
        if let (Some(dst), Some(src)) = (s.as_object_mut(), v.as_object()) {
            for (k, val) in src {
                if let Some(cur) = dst.get(k) {
                    let same = std::mem::discriminant(cur) == std::mem::discriminant(val) || (cur.is_number() && val.is_number());
                    if same {
                        dst.insert(k.clone(), val.clone());
                    }
                }
            }
        }
        serde_json::from_value(s).map_err(|e| e.to_string())
    }

    pub fn save(&self) {
        let dir = config_dir();
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(settings_path(), text);
        }
    }

    pub fn add_recent(&mut self, path: &std::path::Path) {
        let p = path.to_string_lossy().to_string();
        self.recent_files.retain(|x| x != &p);
        self.recent_files.insert(0, p);
        self.recent_files.truncate(30);
    }
}

pub fn strip_json_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_str = false;
    while let Some(c) = chars.next() {
        if in_str {
            out.push(c);
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
            out.push(c);
        } else if c == '/' && chars.peek() == Some(&'/') {
            for n in chars.by_ref() {
                if n == '\n' {
                    out.push('\n');
                    break;
                }
            }
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            let mut prev = ' ';
            for n in chars.by_ref() {
                if prev == '*' && n == '/' {
                    break;
                }
                prev = n;
            }
        } else {
            out.push(c);
        }
    }
    // tolerate trailing commas
    let re = regex::Regex::new(r",(\s*[}\]])").unwrap();
    re.replace_all(&out, "$1").into_owned()
}

/// What was open when the app closed, including unsaved text (hot exit).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub folder: Option<String>,
    pub active: usize,
    pub tabs: Vec<SessionTab>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionTab {
    pub path: Option<String>,
    pub name: String,
    /// Present only when the buffer had unsaved changes.
    pub unsaved: Option<String>,
    pub caret: usize,
    pub scroll: f32,
    pub mode: String,
}

impl Session {
    pub fn load() -> Self {
        std::fs::read_to_string(session_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }
    pub fn save(&self) {
        let _ = std::fs::create_dir_all(config_dir());
        if let Ok(t) = serde_json::to_string(self) {
            let _ = std::fs::write(session_path(), t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_loose_settings() {
        let s = Settings::parse("{\n // comment\n \"theme\": \"dracula\", \"lineNumbers\": false, \"tab_size\": \"oops\", \"font_size\": 15,\n}").unwrap();
        assert_eq!(s.theme, "dracula");
        assert!(!s.line_numbers);
        assert_eq!(s.tab_size, 4);
        assert_eq!(s.font_size, 15.0);
    }
}
