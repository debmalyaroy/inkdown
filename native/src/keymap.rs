//! Key bindings: Sublime Text's default map, with user overrides from `keybindings.json`
//! (`[{ "keys": ["ctrl+k", "ctrl+u"], "command": "upper_case" }]`).

use egui::{Key, Modifiers};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Combo {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: Key,
}

impl Combo {
    pub fn from_event(key: Key, m: Modifiers) -> Self {
        Self { ctrl: m.command || m.ctrl, shift: m.shift, alt: m.alt, key }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let mut c = Combo { ctrl: false, shift: false, alt: false, key: Key::A };
        let mut key = None;
        let s = s.trim().to_lowercase();
        // "ctrl++" and "ctrl+plus"
        let parts: Vec<&str> = if s.ends_with("++") { s[..s.len() - 2].split('+').chain(std::iter::once("+")).collect() } else { s.split('+').collect() };
        for p in parts {
            match p {
                "ctrl" | "control" | "cmd" | "super" | "primary" => c.ctrl = true,
                "shift" => c.shift = true,
                "alt" | "option" => c.alt = true,
                k => key = Some(key_from(k)?),
            }
        }
        c.key = key?;
        Some(c)
    }

    pub fn label(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str(if cfg!(target_os = "macos") { "⌘" } else { "Ctrl+" });
        }
        if self.alt {
            s.push_str("Alt+");
        }
        if self.shift {
            s.push_str("Shift+");
        }
        s.push_str(key_label(self.key));
        s
    }
}

fn key_from(k: &str) -> Option<Key> {
    Some(match k {
        "enter" | "return" => Key::Enter,
        "escape" | "esc" => Key::Escape,
        "tab" => Key::Tab,
        "backspace" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "insert" => Key::Insert,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "up" => Key::ArrowUp,
        "down" => Key::ArrowDown,
        "left" => Key::ArrowLeft,
        "right" => Key::ArrowRight,
        "space" => Key::Space,
        "/" | "slash" => Key::Slash,
        "\\" | "backslash" => Key::Backslash,
        "[" => Key::OpenBracket,
        "]" => Key::CloseBracket,
        "," => Key::Comma,
        "." => Key::Period,
        "-" | "minus" => Key::Minus,
        "=" | "equals" => Key::Equals,
        "+" | "plus" => Key::Plus,
        ";" => Key::Semicolon,
        "'" => Key::Quote,
        "`" => Key::Backtick,
        _ => {
            if let Some(n) = k.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
                return Some(match n {
                    1 => Key::F1,
                    2 => Key::F2,
                    3 => Key::F3,
                    4 => Key::F4,
                    5 => Key::F5,
                    6 => Key::F6,
                    7 => Key::F7,
                    8 => Key::F8,
                    9 => Key::F9,
                    10 => Key::F10,
                    11 => Key::F11,
                    12 => Key::F12,
                    _ => return None,
                });
            }
            let mut it = k.chars();
            let c = it.next()?;
            if it.next().is_some() {
                return None;
            }
            Key::from_name(&c.to_uppercase().to_string())?
        }
    })
}

fn key_label(k: Key) -> &'static str {
    match k {
        Key::ArrowUp => "↑",
        Key::ArrowDown => "↓",
        Key::ArrowLeft => "←",
        Key::ArrowRight => "→",
        Key::Slash => "/",
        Key::Backslash => "\\",
        Key::OpenBracket => "[",
        Key::CloseBracket => "]",
        Key::Comma => ",",
        Key::Period => ".",
        Key::Minus => "-",
        Key::Equals => "=",
        Key::Plus => "+",
        Key::Semicolon => ";",
        Key::Quote => "'",
        Key::Backtick => "`",
        Key::Escape => "Esc",
        Key::Backspace => "Backspace",
        Key::Delete => "Del",
        Key::PageUp => "PgUp",
        Key::PageDown => "PgDn",
        other => other.name(),
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Scope {
    /// Works anywhere in the window.
    Global,
    /// Only while the code editor has keyboard focus.
    Editor,
}

#[derive(Clone, Debug)]
pub struct Binding {
    pub keys: Vec<Combo>,
    pub command: String,
    pub scope: Scope,
}

pub struct Keymap {
    pub bindings: Vec<Binding>,
    pending: Option<Combo>,
}

pub enum Match {
    None,
    /// First half of a chord: wait for the next key.
    Pending,
    Command(String, Scope),
}

const EDITOR: &[(&str, &str)] = &[
    ("left", "move_left"),
    ("right", "move_right"),
    ("up", "move_up"),
    ("down", "move_down"),
    ("shift+left", "select_left"),
    ("shift+right", "select_right"),
    ("shift+up", "select_up"),
    ("shift+down", "select_down"),
    ("ctrl+left", "move_word_left"),
    ("ctrl+right", "move_word_right"),
    ("ctrl+shift+left", "select_word_left"),
    ("ctrl+shift+right", "select_word_right"),
    ("alt+left", "move_subword_left"),
    ("alt+right", "move_subword_right"),
    ("alt+shift+left", "select_subword_left"),
    ("alt+shift+right", "select_subword_right"),
    ("home", "move_to_bol"),
    ("end", "move_to_eol"),
    ("shift+home", "select_to_bol"),
    ("shift+end", "select_to_eol"),
    ("ctrl+home", "move_to_bof"),
    ("ctrl+end", "move_to_eof"),
    ("ctrl+shift+home", "select_to_bof"),
    ("ctrl+shift+end", "select_to_eof"),
    ("pageup", "page_up"),
    ("pagedown", "page_down"),
    ("shift+pageup", "select_page_up"),
    ("shift+pagedown", "select_page_down"),
    ("ctrl+m", "move_to_bracket"),
    ("ctrl+a", "select_all"),
    ("ctrl+l", "expand_selection_to_line"),
    ("ctrl+d", "find_under_expand"),
    ("ctrl+k ctrl+d", "find_under_expand_skip"),
    ("alt+f3", "find_all_under"),
    ("ctrl+shift+l", "split_selection_into_lines"),
    ("ctrl+alt+up", "add_cursor_above"),
    ("ctrl+alt+down", "add_cursor_below"),
    ("ctrl+shift+m", "expand_selection_to_brackets"),
    ("ctrl+shift+j", "expand_selection_to_indentation"),
    ("ctrl+shift+space", "expand_selection"),
    ("enter", "insert_newline"),
    ("shift+enter", "insert_newline"),
    ("backspace", "left_delete"),
    ("shift+backspace", "left_delete"),
    ("delete", "right_delete"),
    ("ctrl+backspace", "delete_word_left"),
    ("ctrl+delete", "delete_word_right"),
    ("ctrl+shift+backspace", "delete_to_bol"),
    ("ctrl+shift+delete", "delete_to_eol"),
    ("ctrl+k ctrl+k", "delete_to_eol"),
    ("ctrl+k ctrl+backspace", "delete_to_bol"),
    ("ctrl+shift+k", "delete_line"),
    ("ctrl+shift+d", "duplicate_line"),
    ("ctrl+shift+up", "swap_line_up"),
    ("ctrl+shift+down", "swap_line_down"),
    ("ctrl+j", "join_lines"),
    ("ctrl+]", "indent"),
    ("ctrl+[", "unindent"),
    ("tab", "insert_tab"),
    ("shift+tab", "unindent_tab"),
    ("ctrl+/", "toggle_comment"),
    ("ctrl+shift+/", "toggle_block_comment"),
    ("ctrl+enter", "insert_line_after"),
    ("ctrl+shift+enter", "insert_line_before"),
    ("ctrl+t", "transpose"),
    ("ctrl+k ctrl+u", "upper_case"),
    ("ctrl+k ctrl+l", "lower_case"),
    ("f9", "sort_lines"),
    ("ctrl+f9", "sort_lines_case_insensitive"),
    ("ctrl+z", "undo"),
    ("ctrl+y", "redo"),
    ("ctrl+shift+z", "redo"),
    ("ctrl+u", "soft_undo"),
    ("escape", "single_selection"),
    ("ctrl+space", "auto_complete"),
    ("ctrl+shift+[", "fold"),
    ("ctrl+shift+]", "unfold"),
    ("ctrl+k ctrl+j", "unfold_all"),
    ("ctrl+k ctrl+0", "unfold_all"),
    ("ctrl+k ctrl+1", "fold_level_1"),
    ("ctrl+k ctrl+2", "fold_level_2"),
    ("ctrl+k ctrl+3", "fold_level_3"),
    ("ctrl+f2", "toggle_bookmark"),
    ("f2", "next_bookmark"),
    ("shift+f2", "prev_bookmark"),
    ("ctrl+shift+f2", "clear_bookmarks"),
    ("ctrl+up", "scroll_lines_up"),
    ("ctrl+down", "scroll_lines_down"),
    ("ctrl+k ctrl+c", "show_at_center"),
    ("ctrl+e", "slurp_find_string"),
    ("ctrl+shift+h", "replace_next"),
    ("ctrl+alt+enter", "replace_all"),
];

const GLOBAL: &[(&str, &str)] = &[
    ("ctrl+n", "new_file"),
    ("ctrl+o", "open_file"),
    ("ctrl+k ctrl+o", "open_folder"),
    ("ctrl+s", "save"),
    ("ctrl+shift+s", "save_as"),
    ("ctrl+alt+s", "save_all"),
    ("ctrl+w", "close"),
    ("ctrl+f4", "close"),
    ("ctrl+shift+w", "close_all"),
    ("ctrl+shift+t", "reopen_closed_file"),
    ("ctrl+tab", "next_tab"),
    ("ctrl+shift+tab", "prev_tab"),
    ("ctrl+pagedown", "next_tab"),
    ("ctrl+pageup", "prev_tab"),
    ("alt+1", "select_tab_1"),
    ("alt+2", "select_tab_2"),
    ("alt+3", "select_tab_3"),
    ("alt+4", "select_tab_4"),
    ("alt+5", "select_tab_5"),
    ("alt+6", "select_tab_6"),
    ("alt+7", "select_tab_7"),
    ("alt+8", "select_tab_8"),
    ("alt+9", "select_tab_9"),
    ("ctrl+p", "goto_anything"),
    ("ctrl+shift+p", "command_palette"),
    ("ctrl+r", "goto_symbol"),
    ("ctrl+g", "goto_line"),
    ("ctrl+;", "goto_word"),
    ("f12", "goto_definition"),
    ("ctrl+f", "show_find"),
    ("ctrl+h", "show_replace"),
    ("f3", "find_next"),
    ("shift+f3", "find_prev"),
    ("ctrl+shift+f", "find_in_files"),
    ("alt+c", "toggle_case_sensitive"),
    ("alt+r", "toggle_regex"),
    ("alt+w", "toggle_whole_word"),
    ("ctrl+k ctrl+b", "toggle_sidebar"),
    ("ctrl+b", "build"),
    ("ctrl+i", "md_italic"),
    ("ctrl+shift+b", "build_with"),
    ("ctrl+shift+c", "cancel_build"),
    ("f11", "toggle_fullscreen"),
    ("ctrl+=", "increase_font_size"),
    ("ctrl+plus", "increase_font_size"),
    ("ctrl+-", "decrease_font_size"),
    ("ctrl+0", "reset_font_size"),
    ("ctrl+q", "toggle_record_macro"),
    ("ctrl+shift+q", "run_macro"),
    ("ctrl+shift+e", "cycle_mode"),
    ("alt+z", "toggle_word_wrap"),
    ("ctrl+,", "open_settings"),
    ("ctrl+k ctrl+t", "select_theme"),
    ("ctrl+k ctrl+m", "toggle_minimap"),
    ("ctrl+shift+n", "new_window"),
    ("alt+shift+1", "layout_single"),
    ("alt+shift+2", "layout_columns_2"),
    ("alt+shift+3", "layout_columns_3"),
    ("alt+shift+8", "layout_rows_2"),
    ("ctrl+1", "focus_group_1"),
    ("ctrl+2", "focus_group_2"),
    ("ctrl+3", "focus_group_3"),
    ("ctrl+shift+1", "move_to_group_1"),
    ("ctrl+shift+2", "move_to_group_2"),
    ("ctrl+shift+3", "move_to_group_3"),
    ("f1", "show_shortcuts"),
];

fn parse_keys(s: &str) -> Option<Vec<Combo>> {
    s.split_whitespace().map(Combo::parse).collect()
}

impl Keymap {
    pub fn load() -> Self {
        let mut bindings = Vec::new();
        for (table, scope) in [(EDITOR, Scope::Editor), (GLOBAL, Scope::Global)] {
            for (keys, cmd) in table {
                if let Some(k) = parse_keys(keys) {
                    bindings.push(Binding { keys: k, command: cmd.to_string(), scope });
                }
            }
        }
        if let Ok(text) = std::fs::read_to_string(crate::settings::keymap_path()) {
            bindings.extend(Self::parse_user(&text, &bindings));
        }
        Self { bindings, pending: None }
    }

    fn parse_user(text: &str, defaults: &[Binding]) -> Vec<Binding> {
        let mut out = Vec::new();
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&crate::settings::strip_json_comments(text)) else { return out };
        for item in v.as_array().into_iter().flatten() {
            let Some(cmd) = item["command"].as_str() else { continue };
            let keys: Option<Vec<Combo>> = match &item["keys"] {
                serde_json::Value::Array(a) => a.iter().map(|k| k.as_str().and_then(Combo::parse)).collect(),
                serde_json::Value::String(s) => parse_keys(s),
                _ => None,
            };
            let Some(keys) = keys.filter(|k| !k.is_empty()) else { continue };
            let scope = defaults.iter().find(|b| b.command == cmd).map(|b| b.scope).unwrap_or(Scope::Global);
            out.push(Binding { keys, command: cmd.to_string(), scope });
        }
        out
    }

    pub fn chord_pending(&self) -> Option<Combo> {
        self.pending
    }

    /// Feed one key press. Later bindings (user ones) win over earlier ones.
    pub fn feed(&mut self, combo: Combo, editor_focus: bool) -> Match {
        let usable = |b: &&Binding| b.scope == Scope::Global || editor_focus;
        if let Some(first) = self.pending.take() {
            if let Some(b) = self.bindings.iter().rev().filter(usable).find(|b| b.keys.len() == 2 && b.keys[0] == first && b.keys[1] == combo) {
                return Match::Command(b.command.clone(), b.scope);
            }
            return Match::None;
        }
        // the most recent binding for this key decides
        let hit = self.bindings.iter().rev().filter(usable).find(|b| b.keys[0] == combo);
        match hit {
            Some(b) if b.keys.len() == 1 => {
                // a chord starting with the same key takes precedence only if it was bound later
                Match::Command(b.command.clone(), b.scope)
            }
            Some(_) => {
                self.pending = Some(combo);
                Match::Pending
            }
            None => Match::None,
        }
    }

    /// Shortcut text for a command, for menus and the palette.
    pub fn label(&self, command: &str) -> String {
        self.bindings.iter().rev().find(|b| b.command == command).map(|b| b.keys.iter().map(Combo::label).collect::<Vec<_>>().join(" ")).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_matches() {
        let c = Combo::parse("ctrl+shift+p").unwrap();
        assert!(c.ctrl && c.shift && !c.alt && c.key == Key::P);
        assert_eq!(Combo::parse("ctrl+/").unwrap().key, Key::Slash);
        assert_eq!(Combo::parse("f12").unwrap().key, Key::F12);
        assert!(Combo::parse("ctrl+nonsense").is_none());
        let mut km = Keymap { bindings: Vec::new(), pending: None };
        for (keys, cmd) in EDITOR.iter().chain(GLOBAL) {
            let k = parse_keys(keys).unwrap_or_else(|| panic!("bad default binding {keys}"));
            km.bindings.push(Binding { keys: k, command: cmd.to_string(), scope: Scope::Global });
        }
        assert!(matches!(km.feed(Combo::parse("ctrl+d").unwrap(), true), Match::Command(c, _) if c == "find_under_expand"));
        assert!(matches!(km.feed(Combo::parse("ctrl+k").unwrap(), true), Match::Pending));
        assert!(matches!(km.feed(Combo::parse("ctrl+u").unwrap(), true), Match::Command(c, _) if c == "upper_case"));
        assert_eq!(km.label("save"), "Ctrl+S");
        let user = Keymap::parse_user(r#"[{ "keys": ["ctrl+d"], "command": "duplicate_line" }]"#, &km.bindings);
        km.bindings.extend(user);
        assert!(matches!(km.feed(Combo::parse("ctrl+d").unwrap(), true), Match::Command(c, _) if c == "duplicate_line"));
    }
}
