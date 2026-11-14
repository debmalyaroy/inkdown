//! Editing commands on a `Buffer`: everything the keyboard can do, independent of the UI.

use crate::buffer::{is_word, Buffer, Edit, EditKind, Sel};

#[derive(Clone, Debug)]
pub struct EdCfg {
    pub tab_width: usize,
    pub soft_tabs: bool,
    pub auto_pair: bool,
    pub auto_indent: bool,
    pub line_comment: Option<String>,
    pub block_comment: Option<(String, String)>,
}

impl Default for EdCfg {
    fn default() -> Self {
        Self { tab_width: 4, soft_tabs: true, auto_pair: true, auto_indent: true, line_comment: Some("//".into()), block_comment: Some(("/*".into(), "*/".into())) }
    }
}

impl EdCfg {
    pub fn unit(&self) -> String {
        if self.soft_tabs { " ".repeat(self.tab_width) } else { "\t".into() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    Left(bool),
    Right(bool),
    Up(bool),
    Down(bool),
    WordLeft(bool),
    WordRight(bool),
    SubwordLeft(bool),
    SubwordRight(bool),
    Home(bool),
    End(bool),
    DocStart(bool),
    DocEnd(bool),
    PageUp(bool, usize),
    PageDown(bool, usize),
    MatchBracket,
    SelectAll,
    SelectLine,
    SelectWordOrNext,
    SkipNext,
    SelectAllOccurrences,
    SplitIntoLines,
    CursorAbove,
    CursorBelow,
    SingleSel,
    ExpandToBrackets,
    ExpandToIndent,
    ExpandSelection,
    Insert(String),
    Newline,
    Backspace,
    Delete,
    DeleteWordLeft,
    DeleteWordRight,
    DeleteToLineStart,
    DeleteToLineEnd,
    DeleteLine,
    Duplicate,
    MoveLinesUp,
    MoveLinesDown,
    JoinLines,
    Indent,
    Unindent,
    Tab,
    BackTab,
    ToggleComment,
    ToggleBlockComment,
    LineAfter,
    LineBefore,
    Transpose,
    Upper,
    Lower,
    TitleCase,
    SortLines,
    SortLinesInsensitive,
    ReverseLines,
    UniqueLines,
    Paste(String),
    Undo,
    Redo,
    TrimTrailing,
    TabsToSpaces,
    SpacesToTabs,
}

// ------------------------------------------------------------- utilities

pub fn vcol(line: &str, col: usize, tab: usize) -> usize {
    let mut v = 0;
    for c in line.chars().take(col) {
        v = if c == '\t' { (v / tab + 1) * tab } else { v + 1 };
    }
    v
}

pub fn col_at_vcol(line: &str, target: usize, tab: usize) -> usize {
    let mut v = 0;
    for (i, c) in line.chars().enumerate() {
        let next = if c == '\t' { (v / tab + 1) * tab } else { v + 1 };
        if next > target {
            return if c == '\t' && target - v > next - target { i + 1 } else { i };
        }
        v = next;
    }
    line.chars().count()
}

fn indent_of(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ' || *c == '\t').count()
}

#[derive(PartialEq, Clone, Copy)]
enum Class {
    Space,
    Word,
    Punct,
    Newline,
}
fn class(c: char) -> Class {
    if c == '\n' {
        Class::Newline
    } else if c.is_whitespace() {
        Class::Space
    } else if is_word(c) {
        Class::Word
    } else {
        Class::Punct
    }
}

pub fn word_left(b: &Buffer, mut p: usize, sub: bool) -> usize {
    if p == 0 {
        return 0;
    }
    if b.char_at(p - 1) == Some('\n') {
        return p - 1;
    }
    while p > 0 && class(b.rope.char(p - 1)) == Class::Space {
        p -= 1;
    }
    if p == 0 {
        return 0;
    }
    let k = class(b.rope.char(p - 1));
    if k == Class::Newline {
        return p;
    }
    let start = p;
    while p > 0 && class(b.rope.char(p - 1)) == k {
        if sub && k == Class::Word && p < start {
            let (prev, cur) = (b.rope.char(p - 1), b.rope.char(p));
            if (prev == '_') != (cur == '_') || (prev.is_lowercase() && cur.is_uppercase()) {
                break;
            }
            if cur.is_uppercase() && prev.is_uppercase() && b.char_at(p + 1).is_some_and(|n| n.is_lowercase()) {
                break;
            }
        }
        p -= 1;
    }
    p
}

pub fn word_right(b: &Buffer, mut p: usize, sub: bool) -> usize {
    let len = b.len();
    if p >= len {
        return len;
    }
    if b.rope.char(p) == '\n' {
        return p + 1;
    }
    while p < len && class(b.rope.char(p)) == Class::Space {
        p += 1;
    }
    if p >= len {
        return len;
    }
    let k = class(b.rope.char(p));
    if k == Class::Newline {
        return p;
    }
    let start = p;
    while p < len && class(b.rope.char(p)) == k {
        if sub && k == Class::Word && p > start {
            let (prev, cur) = (b.rope.char(p - 1), b.rope.char(p));
            if (prev == '_') != (cur == '_') || (prev.is_lowercase() && cur.is_uppercase()) {
                break;
            }
            if prev.is_uppercase() && cur.is_uppercase() && b.char_at(p + 1).is_some_and(|n| n.is_lowercase()) {
                break;
            }
        }
        p += 1;
    }
    p
}

/// The word around `p` (or the punctuation run), as a char range.
pub fn word_at(b: &Buffer, p: usize) -> (usize, usize) {
    let len = b.len();
    let k = match b.char_at(p).map(class) {
        Some(Class::Word) => Class::Word,
        _ if p > 0 && class(b.rope.char(p - 1)) == Class::Word => Class::Word,
        Some(Class::Punct) => Class::Punct,
        _ => return (p, p),
    };
    let (mut a, mut z) = (p, p);
    while a > 0 && class(b.rope.char(a - 1)) == k {
        a -= 1;
    }
    while z < len && class(b.rope.char(z)) == k {
        z += 1;
    }
    (a, z)
}

const OPEN: &str = "([{";
const CLOSE: &str = ")]}";

/// Position of the bracket matching the one at `p`.
pub fn match_bracket(b: &Buffer, p: usize) -> Option<usize> {
    let c = b.char_at(p)?;
    const LIMIT: usize = 400_000;
    if let Some(i) = OPEN.find(c) {
        let close = CLOSE.as_bytes()[i] as char;
        let mut depth = 0usize;
        for (n, ch) in b.rope.chars_at(p).enumerate() {
            if n > LIMIT {
                break;
            }
            if ch == c {
                depth += 1;
            } else if ch == close {
                depth -= 1;
                if depth == 0 {
                    return Some(p + n);
                }
            }
        }
    } else if let Some(i) = CLOSE.find(c) {
        let open = OPEN.as_bytes()[i] as char;
        let mut depth = 0usize;
        let mut it = b.rope.chars_at(p + 1);
        let mut n = 0;
        while let Some(ch) = it.prev() {
            if n > LIMIT {
                break;
            }
            if ch == c {
                depth += 1;
            } else if ch == open {
                depth -= 1;
                if depth == 0 {
                    return Some(p - n);
                }
            }
            n += 1;
        }
    }
    None
}

/// The innermost bracket pair enclosing `a..z`: returns positions of the two brackets.
pub fn enclosing_brackets(b: &Buffer, a: usize, z: usize) -> Option<(usize, usize)> {
    let mut depth = 0i32;
    let mut it = b.rope.chars_at(a);
    let mut p = a;
    let mut n = 0;
    while let Some(ch) = it.prev() {
        p -= 1;
        n += 1;
        if n > 400_000 {
            return None;
        }
        if CLOSE.contains(ch) {
            depth += 1;
        } else if OPEN.contains(ch) {
            if depth == 0 {
                let m = match_bracket(b, p)?;
                if m >= z {
                    return Some((p, m));
                }
            } else {
                depth -= 1;
            }
        }
    }
    None
}

/// Find `needle` starting at char `from`. Returns the char range.
pub fn find_text(hay: &str, b: &Buffer, needle: &str, from: usize, case: bool, word: bool, wrap: bool) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return None;
    }
    let n_chars = needle.chars().count();
    let from_byte = b.rope.char_to_byte(from.min(b.len()));
    let lower_hay;
    let lower_needle;
    let (h, n): (&str, &str) = if case {
        (hay, needle)
    } else {
        lower_hay = hay.to_ascii_lowercase();
        lower_needle = needle.to_ascii_lowercase();
        (&lower_hay, &lower_needle)
    };
    let ok = |byte: usize| {
        if !word {
            return true;
        }
        let before = hay[..byte].chars().next_back();
        let after = hay[byte + needle.len()..].chars().next();
        !before.is_some_and(is_word) && !after.is_some_and(is_word)
    };
    let scan = |start: usize, end: usize| -> Option<usize> {
        let mut at = start;
        while at <= end {
            let i = h[at..].find(n)? + at;
            if i >= end && end < h.len() {
                return None;
            }
            if ok(i) {
                return Some(i);
            }
            at = i + n.len().max(1);
            while at < h.len() && !h.is_char_boundary(at) {
                at += 1;
            }
        }
        None
    };
    let hit = scan(from_byte, h.len()).or_else(|| if wrap { scan(0, from_byte) } else { None })?;
    let c = b.rope.byte_to_char(hit);
    Some((c, c + n_chars))
}

/// Line ranges (inclusive) covered by the selections, merged.
fn line_blocks(b: &Buffer) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut sels = b.sels.clone();
    sels.sort_by_key(|s| s.start());
    for s in sels {
        let l0 = b.line_of(s.start());
        let mut l1 = b.line_of(s.end());
        if l1 > l0 && s.end() == b.line_start(l1) {
            l1 -= 1; // selection ending at column 0 does not include that line
        }
        match out.last_mut() {
            Some(last) if l0 <= last.1 + 1 => last.1 = last.1.max(l1),
            _ => out.push((l0, l1)),
        }
    }
    out
}

/// Replace each range and place a selection relative to the start of its new text.
fn replace_each(b: &mut Buffer, items: Vec<(usize, usize, String, usize, usize)>, kind: EditKind) {
    let mut items = items;
    items.sort_by_key(|i| (i.0, i.1));
    let mut clean: Vec<(usize, usize, String, usize, usize)> = Vec::new();
    for it in items {
        if clean.last().is_some_and(|l| it.0 < l.1 || (it.0 == l.0 && it.1 == l.1)) {
            continue;
        }
        clean.push(it);
    }
    let mut sels = Vec::with_capacity(clean.len());
    let mut delta: isize = 0;
    for (s, e, t, a, h) in &clean {
        let base = (*s as isize + delta) as usize;
        sels.push(Sel::new(base + a, base + h));
        delta += t.chars().count() as isize - (*e - *s) as isize;
    }
    let edits = clean.into_iter().map(|(s, e, t, _, _)| Edit::new(s, e, t)).collect();
    b.edit(edits, kind);
    b.sels = sels;
    b.commit_sels();
}

fn move_sels(b: &mut Buffer, extend: bool, f: impl Fn(&Buffer, &Sel) -> (usize, Option<usize>)) {
    let new: Vec<Sel> = b
        .sels
        .iter()
        .map(|s| {
            let (head, goal) = f(b, s);
            Sel { anchor: if extend { s.anchor } else { head }, head, goal }
        })
        .collect();
    b.sels = new;
    b.normalize();
}

fn vertical(b: &Buffer, s: &Sel, delta: isize, tab: usize) -> (usize, Option<usize>) {
    let (line, col) = b.line_col(s.head);
    let goal = s.goal.unwrap_or_else(|| vcol(&b.line_text(line), col, tab));
    let target = line as isize + delta;
    if target < 0 {
        return (0, Some(goal));
    }
    if target as usize >= b.lines() {
        return (b.len(), Some(goal));
    }
    let t = target as usize;
    (b.line_start(t) + col_at_vcol(&b.line_text(t), goal, tab), Some(goal))
}

fn map_text(b: &mut Buffer, f: impl Fn(&str) -> String) {
    let mut items = Vec::new();
    for s in b.sels.clone() {
        let (a, z) = if s.is_empty() { word_at(b, s.head) } else { (s.start(), s.end()) };
        if a == z {
            continue;
        }
        let t = f(&b.slice(a, z));
        let n = t.chars().count();
        items.push((a, z, t, 0, n));
    }
    if !items.is_empty() {
        replace_each(b, items, EditKind::Other);
    }
}

/// Apply `f` to the lines of the selection (or the whole document when nothing is selected).
fn map_lines(b: &mut Buffer, f: impl Fn(Vec<&str>) -> Vec<String>) {
    let multi = b.sels.iter().any(|s| b.line_of(s.start()) != b.line_of(s.end()));
    let blocks = if multi { line_blocks(b) } else { vec![(0, b.lines().saturating_sub(1))] };
    let mut items = Vec::new();
    for (l0, mut l1) in blocks {
        // the empty line after a final newline is not a line to sort
        if !multi && l1 > l0 && b.line_len(l1) == 0 {
            l1 -= 1;
        }
        let (a, z) = (b.line_start(l0), b.line_end(l1));
        let text = b.slice(a, z);
        let out = f(text.split('\n').collect()).join("\n");
        let n = out.chars().count();
        items.push((a, z, out, 0, if multi { n } else { 0 }));
    }
    replace_each(b, items, EditKind::Other);
}

fn pair_for(c: char) -> Option<char> {
    Some(match c {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '"' => '"',
        '\'' => '\'',
        '`' => '`',
        _ => return None,
    })
}

// ------------------------------------------------------------------ commands

/// Run a command. Returns true when the text changed.
pub fn exec(b: &mut Buffer, cmd: Cmd, cfg: &EdCfg) -> bool {
    let v0 = b.version;
    let tab = cfg.tab_width.max(1);
    match cmd {
        Cmd::Left(ext) => move_sels(b, ext, |b, s| {
            if !ext && !s.is_empty() {
                (s.start(), None)
            } else {
                let _ = b;
                (s.head.saturating_sub(1), None)
            }
        }),
        Cmd::Right(ext) => move_sels(b, ext, |b, s| if !ext && !s.is_empty() { (s.end(), None) } else { ((s.head + 1).min(b.len()), None) }),
        Cmd::Up(ext) => move_sels(b, ext, |b, s| vertical(b, s, -1, tab)),
        Cmd::Down(ext) => move_sels(b, ext, |b, s| vertical(b, s, 1, tab)),
        Cmd::PageUp(ext, n) => move_sels(b, ext, |b, s| vertical(b, s, -(n as isize), tab)),
        Cmd::PageDown(ext, n) => move_sels(b, ext, |b, s| vertical(b, s, n as isize, tab)),
        Cmd::WordLeft(ext) => move_sels(b, ext, |b, s| (word_left(b, s.head, false), None)),
        Cmd::WordRight(ext) => move_sels(b, ext, |b, s| (word_right(b, s.head, false), None)),
        Cmd::SubwordLeft(ext) => move_sels(b, ext, |b, s| (word_left(b, s.head, true), None)),
        Cmd::SubwordRight(ext) => move_sels(b, ext, |b, s| (word_right(b, s.head, true), None)),
        Cmd::Home(ext) => move_sels(b, ext, |b, s| {
            let (line, col) = b.line_col(s.head);
            let ind = indent_of(&b.line_text(line));
            (b.line_start(line) + if col == ind { 0 } else { ind }, None)
        }),
        Cmd::End(ext) => move_sels(b, ext, |b, s| (b.line_end(b.line_of(s.head)), None)),
        Cmd::DocStart(ext) => move_sels(b, ext, |_, _| (0, None)),
        Cmd::DocEnd(ext) => move_sels(b, ext, |b, _| (b.len(), None)),
        Cmd::MatchBracket => move_sels(b, false, |b, s| {
            let p = s.head;
            let at = match_bracket(b, p).or_else(|| p.checked_sub(1).and_then(|q| match_bracket(b, q)));
            match at {
                Some(m) => (m, None),
                None => (enclosing_brackets(b, p, p).map(|(a, _)| a).unwrap_or(p), None),
            }
        }),
        Cmd::SelectAll => b.sels = vec![Sel::new(0, b.len())],
        Cmd::SelectLine => {
            let new = b
                .sels
                .iter()
                .map(|s| {
                    let l0 = b.line_of(s.start());
                    let l1 = b.line_of(s.end());
                    let end = if l1 + 1 < b.lines() { b.line_start(l1 + 1) } else { b.len() };
                    Sel::new(b.line_start(l0), end)
                })
                .collect();
            b.sels = new;
            b.normalize();
        }
        Cmd::SelectWordOrNext => {
            if b.sels.iter().any(|s| s.is_empty()) {
                let new = b
                    .sels
                    .iter()
                    .map(|s| if s.is_empty() { let (a, z) = word_at(b, s.head); Sel::new(a, z) } else { *s })
                    .collect();
                b.sels = new;
                b.occur_word = true;
                b.normalize();
            } else {
                add_next(b, false);
            }
        }
        Cmd::SkipNext => add_next(b, true),
        Cmd::SelectAllOccurrences => {
            let p = b.primary();
            let (a, z) = if p.is_empty() { word_at(b, p.head) } else { (p.start(), p.end()) };
            let word = p.is_empty() || b.occur_word;
            let needle = b.slice(a, z);
            if !needle.is_empty() {
                let hay = b.text();
                let mut out = Vec::new();
                let mut from = 0;
                while let Some((s, e)) = find_text(&hay, b, &needle, from, true, word, false) {
                    out.push(Sel::new(s, e));
                    from = e;
                    if out.len() > 20_000 {
                        break;
                    }
                }
                if !out.is_empty() {
                    // keep the original as primary (last)
                    if let Some(i) = out.iter().position(|s| s.start() == a) {
                        let me = out.remove(i);
                        out.push(me);
                    }
                    b.sels = out;
                    b.primary_last = true;
                }
            }
        }
        Cmd::SplitIntoLines => {
            let mut out = Vec::new();
            for s in &b.sels {
                let (l0, l1) = (b.line_of(s.start()), b.line_of(s.end()));
                if l0 == l1 {
                    out.push(*s);
                    continue;
                }
                for l in l0..=l1 {
                    let a = if l == l0 { s.start() } else { b.line_start(l) };
                    let z = if l == l1 { s.end() } else { b.line_end(l) };
                    if l == l1 && z == b.line_start(l) && l > l0 {
                        continue;
                    }
                    out.push(Sel::new(a, z));
                }
            }
            b.sels = out;
            b.normalize();
        }
        Cmd::CursorAbove | Cmd::CursorBelow => {
            let up = cmd == Cmd::CursorAbove;
            let mut extra = Vec::new();
            for s in &b.sels {
                let (line, col) = b.line_col(s.head);
                let goal = s.goal.unwrap_or_else(|| vcol(&b.line_text(line), col, tab));
                let target = if up { line.checked_sub(1) } else { Some(line + 1).filter(|l| *l < b.lines()) };
                if let Some(t) = target {
                    let p = b.line_start(t) + col_at_vcol(&b.line_text(t), goal, tab);
                    extra.push(Sel { anchor: p, head: p, goal: Some(goal) });
                }
            }
            for s in &mut b.sels {
                if s.goal.is_none() {
                    let (line, col) = (b.rope.char_to_line(s.head), s.head - b.rope.line_to_char(b.rope.char_to_line(s.head)));
                    s.goal = Some(vcol(&b.rope.line(line).to_string(), col, tab));
                }
            }
            b.sels.extend(extra);
            b.normalize();
        }
        Cmd::SingleSel => {
            let p = b.primary();
            b.sels = vec![if b.sels.len() > 1 { p } else { Sel::caret(p.head) }];
        }
        Cmd::ExpandToBrackets => {
            let new = b
                .sels
                .iter()
                .map(|s| match enclosing_brackets(b, s.start(), s.end()) {
                    Some((a, z)) if a + 1 == s.start() && z == s.end() => Sel::new(a, z + 1),
                    Some((a, z)) => Sel::new(a + 1, z),
                    None => *s,
                })
                .collect();
            b.sels = new;
            b.normalize();
        }
        Cmd::ExpandToIndent => {
            let new = b
                .sels
                .iter()
                .map(|s| {
                    let line = b.line_of(s.head);
                    let ind = |l: usize| {
                        let t = b.line_text(l);
                        if t.trim().is_empty() { usize::MAX } else { vcol(&t, indent_of(&t), tab) }
                    };
                    let base = ind(line);
                    if base == usize::MAX {
                        return *s;
                    }
                    let (mut a, mut z) = (line, line);
                    while a > 0 && ind(a - 1) >= base {
                        a -= 1;
                    }
                    while z + 1 < b.lines() && ind(z + 1) >= base {
                        z += 1;
                    }
                    while z > line && b.line_text(z).trim().is_empty() {
                        z -= 1;
                    }
                    while a < line && b.line_text(a).trim().is_empty() {
                        a += 1;
                    }
                    Sel::new(b.line_start(a), b.line_end(z))
                })
                .collect();
            b.sels = new;
            b.normalize();
        }
        Cmd::ExpandSelection => {
            let new = b
                .sels
                .iter()
                .map(|s| {
                    if s.is_empty() {
                        let (a, z) = word_at(b, s.head);
                        if a != z {
                            return Sel::new(a, z);
                        }
                    }
                    match enclosing_brackets(b, s.start(), s.end()) {
                        Some((a, z)) if a + 1 == s.start() && z == s.end() => Sel::new(a, z + 1),
                        Some((a, z)) => Sel::new(a + 1, z),
                        None => {
                            let (l0, l1) = (b.line_of(s.start()), b.line_of(s.end()));
                            let (a, z) = (b.line_start(l0), b.line_end(l1));
                            if (a, z) == (s.start(), s.end()) { Sel::new(0, b.len()) } else { Sel::new(a, z) }
                        }
                    }
                })
                .collect();
            b.sels = new;
            b.normalize();
        }
        Cmd::Insert(text) => insert(b, &text, cfg),
        Cmd::Paste(text) => paste(b, &text),
        Cmd::Newline => {
            let mut items = Vec::new();
            for s in b.sels.clone() {
                let (a, z) = (s.start(), s.end());
                let line = b.line_of(a);
                let lt = b.line_text(line);
                let col = a - b.line_start(line);
                let mut indent: String = if cfg.auto_indent { lt.chars().take(indent_of(&lt).min(col)).collect() } else { String::new() };
                let prev = lt.chars().take(col).filter(|c| !c.is_whitespace()).last();
                let next = b.char_at(z);
                let mut text = String::from("\n");
                let opens = cfg.auto_indent && matches!(prev, Some('{' | '[' | '(' | ':')) && col > 0 && lt.chars().take(col).last().is_some_and(|c| !c.is_whitespace() || prev.is_some());
                let opener_bracket = matches!(prev, Some('{' | '[' | '('));
                if opens && (opener_bracket || lt.trim_end().ends_with(':')) {
                    let closes = opener_bracket && next.is_some_and(|n| Some(n) == prev.and_then(pair_for));
                    let outer = indent.clone();
                    indent.push_str(&cfg.unit());
                    text.push_str(&indent);
                    let caret = text.chars().count();
                    if closes {
                        text.push('\n');
                        text.push_str(&outer);
                    }
                    items.push((a, z, text, caret, caret));
                } else {
                    text.push_str(&indent);
                    let n = text.chars().count();
                    items.push((a, z, text, n, n));
                }
            }
            replace_each(b, items, EditKind::Other);
        }
        Cmd::Backspace => {
            let mut items = Vec::new();
            for s in b.sels.clone() {
                if !s.is_empty() {
                    items.push((s.start(), s.end(), String::new(), 0, 0));
                    continue;
                }
                let p = s.head;
                if p == 0 {
                    continue;
                }
                let (line, col) = b.line_col(p);
                let lt = b.line_text(line);
                let ind = indent_of(&lt);
                let prev = b.rope.char(p - 1);
                if cfg.auto_pair && pair_for(prev).is_some_and(|c| b.char_at(p) == Some(c)) {
                    items.push((p - 1, p + 1, String::new(), 0, 0));
                } else if cfg.soft_tabs && col > 0 && col <= ind && lt.chars().take(col).all(|c| c == ' ') {
                    let n = if col % tab == 0 { tab } else { col % tab };
                    items.push((p - n, p, String::new(), 0, 0));
                } else {
                    items.push((p - 1, p, String::new(), 0, 0));
                }
            }
            replace_each(b, items, EditKind::Delete);
        }
        Cmd::Delete => delete_with(b, |b, p| (p, (p + 1).min(b.len())), EditKind::Delete),
        Cmd::DeleteWordLeft => delete_with(b, |b, p| (word_left(b, p, false), p), EditKind::Other),
        Cmd::DeleteWordRight => delete_with(b, |b, p| (p, word_right(b, p, false)), EditKind::Other),
        Cmd::DeleteToLineStart => delete_with(b, |b, p| (b.line_start(b.line_of(p)), p), EditKind::Other),
        Cmd::DeleteToLineEnd => delete_with(b, |b, p| (p, b.line_end(b.line_of(p))), EditKind::Other),
        Cmd::DeleteLine => {
            let mut items = Vec::new();
            for (l0, l1) in line_blocks(b) {
                let (a, z) = if l1 + 1 < b.lines() {
                    (b.line_start(l0), b.line_start(l1 + 1))
                } else if l0 > 0 {
                    (b.line_end(l0 - 1), b.line_end(l1))
                } else {
                    (0, b.len())
                };
                items.push((a, z, String::new(), 0, 0));
            }
            replace_each(b, items, EditKind::Other);
            let new = b.sels.iter().map(|s| Sel::caret(if b.char_at(s.head) == Some('\n') && s.head + 1 <= b.len() && b.line_start(b.line_of(s.head)) != s.head { s.head + 1 } else { s.head })).collect();
            b.sels = new;
            b.commit_sels();
        }
        Cmd::Duplicate => {
            let mut items = Vec::new();
            for s in b.sels.clone() {
                if s.is_empty() {
                    let line = b.line_of(s.head);
                    let col = s.head - b.line_start(line);
                    let t = b.line_text(line);
                    let e = b.line_end(line);
                    items.push((e, e, format!("\n{t}"), 1 + col, 1 + col));
                } else {
                    let t = b.slice(s.start(), s.end());
                    let n = t.chars().count();
                    items.push((s.end(), s.end(), t, 0, n));
                }
            }
            replace_each(b, items, EditKind::Other);
        }
        Cmd::MoveLinesUp | Cmd::MoveLinesDown => {
            let up = cmd == Cmd::MoveLinesUp;
            let blocks = line_blocks(b);
            let old = b.sels.clone();
            let mut edits = Vec::new();
            let mut shifts: Vec<(usize, usize, isize)> = Vec::new();
            for (l0, l1) in blocks {
                if up {
                    if l0 == 0 {
                        continue;
                    }
                    let prev = b.line_text(l0 - 1);
                    let block = b.slice(b.line_start(l0), b.line_end(l1));
                    edits.push(Edit::new(b.line_start(l0 - 1), b.line_end(l1), format!("{block}\n{prev}")));
                    shifts.push((b.line_start(l0), b.line_end(l1), -(prev.chars().count() as isize + 1)));
                } else {
                    if l1 + 1 >= b.lines() {
                        continue;
                    }
                    let next = b.line_text(l1 + 1);
                    let block = b.slice(b.line_start(l0), b.line_end(l1));
                    edits.push(Edit::new(b.line_start(l0), b.line_end(l1 + 1), format!("{next}\n{block}")));
                    shifts.push((b.line_start(l0), b.line_end(l1), next.chars().count() as isize + 1));
                }
            }
            if !edits.is_empty() {
                b.edit(edits, EditKind::Other);
                b.sels = old
                    .iter()
                    .map(|s| {
                        let d = shifts.iter().find(|(a, z, _)| s.start() >= *a && s.end() <= *z + 1).map(|x| x.2).unwrap_or(0);
                        Sel::new((s.anchor as isize + d) as usize, (s.head as isize + d) as usize)
                    })
                    .collect();
                b.commit_sels();
            }
        }
        Cmd::JoinLines => {
            let mut items = Vec::new();
            let mut seen = std::collections::BTreeSet::new();
            for s in b.sels.clone() {
                let (l0, mut l1) = (b.line_of(s.start()), b.line_of(s.end()));
                if l1 == l0 {
                    l1 = l0 + 1;
                }
                for l in l0..l1 {
                    if l + 1 >= b.lines() || !seen.insert(l) {
                        continue;
                    }
                    let a = b.line_end(l);
                    let next = b.line_text(l + 1);
                    let skip = next.chars().take_while(|c| c.is_whitespace()).count();
                    let sep = if next.trim().is_empty() || b.line_text(l).trim().is_empty() { "" } else { " " };
                    let trail = b.line_text(l).chars().rev().take_while(|c| c.is_whitespace()).count();
                    items.push((a - trail, a + 1 + skip, sep.to_string(), 0, 0));
                }
            }
            replace_each(b, items, EditKind::Other);
        }
        Cmd::Indent => indent_lines(b, cfg, true),
        Cmd::Unindent | Cmd::BackTab => indent_lines(b, cfg, false),
        Cmd::Tab => {
            if b.sels.iter().any(|s| b.line_of(s.start()) != b.line_of(s.end())) {
                indent_lines(b, cfg, true);
            } else {
                let mut items = Vec::new();
                for s in b.sels.clone() {
                    let (line, col) = b.line_col(s.start());
                    let text = if cfg.soft_tabs {
                        let v = vcol(&b.line_text(line), col, tab);
                        " ".repeat(tab - v % tab)
                    } else {
                        "\t".to_string()
                    };
                    let n = text.chars().count();
                    items.push((s.start(), s.end(), text, n, n));
                }
                replace_each(b, items, EditKind::Other);
            }
        }
        Cmd::ToggleComment => toggle_comment(b, cfg),
        Cmd::ToggleBlockComment => toggle_block_comment(b, cfg),
        Cmd::LineAfter | Cmd::LineBefore => {
            let after = cmd == Cmd::LineAfter;
            let mut items = Vec::new();
            for s in b.sels.clone() {
                let line = b.line_of(s.head);
                let lt = b.line_text(line);
                let indent: String = lt.chars().take(indent_of(&lt)).collect();
                let n = indent.chars().count();
                if after {
                    let e = b.line_end(line);
                    items.push((e, e, format!("\n{indent}"), n + 1, n + 1));
                } else {
                    let st = b.line_start(line);
                    items.push((st, st, format!("{indent}\n"), n, n));
                }
            }
            replace_each(b, items, EditKind::Other);
        }
        Cmd::Transpose => {
            let mut items = Vec::new();
            for s in b.sels.clone() {
                let p = s.head;
                let (ls, le) = (b.line_start(b.line_of(p)), b.line_end(b.line_of(p)));
                let p = if p == le && p > ls + 1 { p - 1 } else { p };
                if p > ls && p < le {
                    let (x, y) = (b.rope.char(p - 1), b.rope.char(p));
                    items.push((p - 1, p + 1, format!("{y}{x}"), 2, 2));
                }
            }
            replace_each(b, items, EditKind::Other);
        }
        Cmd::Upper => map_text(b, |t| t.to_uppercase()),
        Cmd::Lower => map_text(b, |t| t.to_lowercase()),
        Cmd::TitleCase => map_text(b, |t| {
            let mut out = String::with_capacity(t.len());
            let mut start = true;
            for c in t.chars() {
                if start && c.is_alphabetic() {
                    out.extend(c.to_uppercase());
                    start = false;
                } else {
                    out.extend(c.to_lowercase());
                    if !is_word(c) && c != '\'' {
                        start = true;
                    }
                }
            }
            out
        }),
        Cmd::SortLines => map_lines(b, |mut l| {
            l.sort();
            l.into_iter().map(String::from).collect()
        }),
        Cmd::SortLinesInsensitive => map_lines(b, |mut l| {
            l.sort_by_key(|s| s.to_lowercase());
            l.into_iter().map(String::from).collect()
        }),
        Cmd::ReverseLines => map_lines(b, |l| l.into_iter().rev().map(String::from).collect()),
        Cmd::UniqueLines => map_lines(b, |l| {
            let mut seen = std::collections::HashSet::new();
            l.into_iter().filter(|s| seen.insert(s.to_string())).map(String::from).collect()
        }),
        Cmd::TrimTrailing => {
            let mut edits = Vec::new();
            for l in 0..b.lines() {
                let t = b.line_text(l);
                let n = t.chars().rev().take_while(|c| *c == ' ' || *c == '\t').count();
                if n > 0 {
                    let e = b.line_end(l);
                    edits.push(Edit::new(e - n, e, ""));
                }
            }
            b.edit(edits, EditKind::Other);
            b.commit_sels();
        }
        Cmd::TabsToSpaces | Cmd::SpacesToTabs => {
            let to_spaces = cmd == Cmd::TabsToSpaces;
            let mut edits = Vec::new();
            for l in 0..b.lines() {
                let t = b.line_text(l);
                let n = indent_of(&t);
                if n == 0 {
                    continue;
                }
                let width = vcol(&t, n, tab);
                let new = if to_spaces { " ".repeat(width) } else { format!("{}{}", "\t".repeat(width / tab), " ".repeat(width % tab)) };
                if new != t.chars().take(n).collect::<String>() {
                    let st = b.line_start(l);
                    edits.push(Edit::new(st, st + n, new));
                }
            }
            b.edit(edits, EditKind::Other);
            b.commit_sels();
        }
        Cmd::Undo => {
            b.undo();
        }
        Cmd::Redo => {
            b.redo();
        }
    }
    b.version != v0
}

fn add_next(b: &mut Buffer, skip: bool) {
    let p = b.primary();
    if p.is_empty() {
        return;
    }
    let needle = b.slice(p.start(), p.end());
    let hay = b.text();
    let mut from = p.end();
    for _ in 0..b.sels.len() + 2 {
        let Some((s, e)) = find_text(&hay, b, &needle, from, true, b.occur_word, true) else { return };
        if b.sels.iter().any(|x| x.start() == s && x.end() == e) {
            if s == p.start() {
                return; // wrapped all the way round
            }
            from = e;
            continue;
        }
        if skip {
            b.sels.pop();
        }
        b.sels.push(Sel::new(s, e));
        b.primary_last = true;
        return;
    }
}

fn delete_with(b: &mut Buffer, range: impl Fn(&Buffer, usize) -> (usize, usize), kind: EditKind) {
    let mut items = Vec::new();
    for s in b.sels.clone() {
        let (a, z) = if s.is_empty() { range(b, s.head) } else { (s.start(), s.end()) };
        if a < z {
            items.push((a, z, String::new(), 0, 0));
        }
    }
    if !items.is_empty() {
        replace_each(b, items, kind);
    }
}

fn insert(b: &mut Buffer, text: &str, cfg: &EdCfg) {
    let single = { let mut it = text.chars(); it.next().filter(|_| it.next().is_none()) };
    let mut items = Vec::new();
    for s in b.sels.clone() {
        let (a, z) = (s.start(), s.end());
        if let (Some(c), true) = (single, cfg.auto_pair) {
            let next = b.char_at(z);
            let prev = a.checked_sub(1).and_then(|p| b.char_at(p));
            if let Some(close) = pair_for(c) {
                if !s.is_empty() {
                    let inner = b.slice(a, z);
                    let n = inner.chars().count();
                    items.push((a, z, format!("{c}{inner}{close}"), 1, 1 + n));
                    continue;
                }
                let quote = c == close;
                if quote && next == Some(c) {
                    items.push((a, a + 1, c.to_string(), 1, 1)); // type over the closing quote
                    continue;
                }
                let room = next.map_or(true, |n| n.is_whitespace() || ")]}>,;:".contains(n));
                let after_word = quote && prev.is_some_and(|p| is_word(p) || p == c);
                if room && !after_word {
                    items.push((a, z, format!("{c}{close}"), 1, 1));
                    continue;
                }
            } else if s.is_empty() && ")]}".contains(c) && next == Some(c) {
                items.push((a, a + 1, c.to_string(), 1, 1)); // type over the closer
                continue;
            } else if s.is_empty() && c == '}' && cfg.auto_indent {
                // dedent a closing brace typed on an otherwise blank line
                let line = b.line_of(a);
                let lt = b.line_text(line);
                let col = a - b.line_start(line);
                if col > 0 && lt.chars().take(col).all(|x| x == ' ' || x == '\t') && lt.chars().skip(col).all(char::is_whitespace) {
                    let unit = cfg.unit().chars().count().min(col);
                    let st = b.line_start(line);
                    let keep: String = lt.chars().take(col - unit).collect();
                    let n = keep.chars().count() + 1;
                    items.push((st, a, format!("{keep}}}"), n, n));
                    continue;
                }
            }
        }
        let n = text.chars().count();
        items.push((a, z, text.to_string(), n, n));
    }
    let kind = if single.is_some() { EditKind::Type } else { EditKind::Other };
    replace_each(b, items, kind);
}

fn paste(b: &mut Buffer, text: &str) {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    // one clipboard line per cursor, like Sublime
    let spread = b.sels.len() > 1 && lines.len() == b.sels.len();
    let mut sorted = b.sels.clone();
    sorted.sort_by_key(|s| s.start());
    let items = sorted
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let t = if spread { lines[i].to_string() } else { text.clone() };
            let n = t.chars().count();
            (s.start(), s.end(), t, n, n)
        })
        .collect();
    replace_each(b, items, EditKind::Other);
}

fn indent_lines(b: &mut Buffer, cfg: &EdCfg, add: bool) {
    let tab = cfg.tab_width.max(1);
    let unit = cfg.unit();
    let un = unit.chars().count();
    let mut edits = Vec::new();
    let old = b.sels.clone();
    let mut deltas: Vec<(usize, isize, usize)> = Vec::new(); // (line start, delta, indent len)
    for (l0, l1) in line_blocks(b) {
        for l in l0..=l1 {
            let t = b.line_text(l);
            let st = b.line_start(l);
            if add {
                if t.is_empty() && l0 != l1 {
                    continue;
                }
                edits.push(Edit::new(st, st, unit.clone()));
                deltas.push((st, un as isize, 0));
            } else {
                let n = if t.starts_with('\t') { 1 } else { t.chars().take(tab).take_while(|c| *c == ' ').count() };
                if n > 0 {
                    edits.push(Edit::new(st, st + n, ""));
                    deltas.push((st, -(n as isize), n));
                }
            }
        }
    }
    if edits.is_empty() {
        return;
    }
    b.edit(edits, EditKind::Other);
    let shift = |p: usize| -> usize {
        let mut d: isize = 0;
        for (st, delta, n) in &deltas {
            if *st > p {
                break;
            }
            if *delta > 0 {
                // text at or after the line start moves right, except a caret at column 0 of a multi-line selection end
                d += *delta;
            } else {
                d -= (p - *st).min(*n) as isize;
            }
        }
        (p as isize + d).max(0) as usize
    };
    b.sels = old
        .iter()
        .map(|s| {
            let multi = b.line_of(shift(s.start())) != b.line_of(shift(s.end()));
            let mut ns = Sel::new(shift(s.anchor), shift(s.head));
            if multi && add {
                // keep the selection start at column 0 so repeated Tab keeps whole lines
                let first = ns.start();
                let ls = b.line_start(b.line_of(first));
                if first - ls == un {
                    if ns.anchor < ns.head { ns.anchor = ls } else { ns.head = ls }
                }
            }
            ns
        })
        .collect();
    b.commit_sels();
}

fn toggle_comment(b: &mut Buffer, cfg: &EdCfg) {
    let Some(tok) = cfg.line_comment.clone() else {
        return toggle_block_comment(b, cfg);
    };
    let tok_trim = tok.trim_end().to_string();
    let tn = tok_trim.chars().count();
    let old = b.sels.clone();
    let mut edits = Vec::new();
    let mut deltas: Vec<(usize, isize)> = Vec::new(); // (position, delta) applied to positions > pos (or >= for inserts)
    for (l0, l1) in line_blocks(b) {
        let lines: Vec<String> = (l0..=l1).map(|l| b.line_text(l)).collect();
        let live: Vec<&String> = lines.iter().filter(|t| !t.trim().is_empty()).collect();
        let all = !live.is_empty() && live.iter().all(|t| t.trim_start().starts_with(&tok_trim));
        let min_indent = live.iter().map(|t| indent_of(t)).min().unwrap_or(0);
        for (i, t) in lines.iter().enumerate() {
            let l = l0 + i;
            let st = b.line_start(l);
            if all {
                if t.trim().is_empty() {
                    continue;
                }
                let ind = indent_of(t);
                let rest: String = t.chars().skip(ind + tn).collect();
                let n = tn + usize::from(rest.starts_with(' '));
                edits.push(Edit::new(st + ind, st + ind + n, ""));
                deltas.push((st + ind, -(n as isize)));
            } else {
                if t.trim().is_empty() && lines.len() > 1 {
                    continue;
                }
                let at = st + min_indent.min(t.chars().count());
                edits.push(Edit::new(at, at, format!("{tok_trim} ")));
                deltas.push((at, tn as isize + 1));
            }
        }
    }
    if edits.is_empty() {
        return;
    }
    b.edit(edits, EditKind::Other);
    let shift = |p: usize| -> usize {
        let mut d: isize = 0;
        for (at, delta) in &deltas {
            if *delta > 0 {
                if p >= *at {
                    d += *delta;
                }
            } else if p > *at {
                d -= ((p - *at) as isize).min(-*delta);
            }
        }
        (p as isize + d).max(0) as usize
    };
    b.sels = old.iter().map(|s| Sel::new(shift(s.anchor), shift(s.head))).collect();
    b.commit_sels();
}

fn toggle_block_comment(b: &mut Buffer, cfg: &EdCfg) {
    let Some((open, close)) = cfg.block_comment.clone() else { return };
    let (on, cn) = (open.chars().count(), close.chars().count());
    let mut items = Vec::new();
    for s in b.sels.clone() {
        let (a, z) = if s.is_empty() {
            let line = b.line_of(s.head);
            let t = b.line_text(line);
            (b.line_start(line) + indent_of(&t), b.line_start(line) + t.trim_end().chars().count())
        } else {
            (s.start(), s.end())
        };
        if a >= z {
            let text = format!("{open}  {close}");
            items.push((a, a, text, on + 1, on + 1));
            continue;
        }
        let inner = b.slice(a, z);
        let trimmed = inner.trim();
        if trimmed.starts_with(&open) && trimmed.ends_with(&close) && trimmed.len() >= open.len() + close.len() {
            let body = trimmed[open.len()..trimmed.len() - close.len()].trim();
            let n = body.chars().count();
            let lead = inner.chars().take_while(|c| c.is_whitespace()).count();
            items.push((a + lead, a + lead + trimmed.chars().count(), body.to_string(), 0, n));
        } else {
            let n = inner.chars().count();
            items.push((a, z, format!("{open} {inner} {close}"), on + 1, on + 1 + n));
            let _ = cn;
        }
    }
    replace_each(b, items, EditKind::Other);
}

/// Guess the indentation style of a file: `(soft_tabs, width)`.
pub fn detect_indent(text: &str) -> Option<(bool, usize)> {
    let (mut tabs, mut spaces) = (0usize, 0usize);
    let mut widths = [0usize; 9];
    let mut prev = 0usize;
    for line in text.lines().take(2000) {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with('\t') {
            tabs += 1;
            continue;
        }
        let n = line.len() - line.trim_start_matches(' ').len();
        if n > 0 {
            spaces += 1;
        }
        let d = n.abs_diff(prev);
        if (2..=8).contains(&d) {
            widths[d] += 1;
        }
        prev = n;
    }
    if tabs == 0 && spaces == 0 {
        return None;
    }
    if tabs > spaces {
        return Some((false, 4));
    }
    let w = [2usize, 4, 8, 3].into_iter().max_by_key(|w| widths[*w]).filter(|w| widths[*w] > 0).unwrap_or(4);
    Some((true, w))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(s: &str) -> Buffer {
        // `|` marks carets, `[`..`]` would clash with code, so only carets here
        let mut text = String::new();
        let mut sels = Vec::new();
        for c in s.chars() {
            if c == '|' {
                sels.push(Sel::caret(text.chars().count()));
            } else {
                text.push(c);
            }
        }
        let mut b = Buffer::new(&text);
        if !sels.is_empty() {
            b.sels = sels;
        }
        b
    }
    fn show(b: &Buffer) -> String {
        let mut marks: Vec<(usize, char)> = Vec::new();
        for s in &b.sels {
            if s.is_empty() {
                marks.push((s.head, '|'));
            } else {
                marks.push((s.start(), '<'));
                marks.push((s.end(), '>'));
            }
        }
        marks.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut chars: Vec<char> = b.text().chars().collect();
        for (p, m) in marks {
            chars.insert(p, m);
        }
        chars.into_iter().collect()
    }
    fn run(s: &str, cmds: Vec<Cmd>) -> String {
        let mut b = buf(s);
        let cfg = EdCfg::default();
        for c in cmds {
            exec(&mut b, c, &cfg);
        }
        show(&b)
    }

    #[test]
    fn typing_and_pairs() {
        assert_eq!(run("a|", vec![Cmd::Insert("(".into())]), "a(|)");
        assert_eq!(run("a(|)", vec![Cmd::Insert(")".into())]), "a()|");
        assert_eq!(run("a(|)", vec![Cmd::Backspace]), "a|");
        assert_eq!(run("x|\ny|", vec![Cmd::Insert("1".into())]), "x1|\ny1|");
        assert_eq!(run("it|", vec![Cmd::Insert("'".into())]), "it'|");
        assert_eq!(run("fn f() {|}", vec![Cmd::Newline]), "fn f() {\n    |\n}");
        assert_eq!(run("    foo|", vec![Cmd::Newline]), "    foo\n    |");
        assert_eq!(run("    |", vec![Cmd::Backspace]), "|");
    }

    #[test]
    fn multi_cursor_words() {
        assert_eq!(run("f|oo bar foo baz foo", vec![Cmd::SelectWordOrNext]), "<foo> bar foo baz foo");
        assert_eq!(run("f|oo bar foo baz foo", vec![Cmd::SelectWordOrNext, Cmd::SelectWordOrNext, Cmd::SelectWordOrNext]), "<foo> bar <foo> baz <foo>");
        assert_eq!(run("f|oo bar foo baz foo", vec![Cmd::SelectWordOrNext, Cmd::SelectWordOrNext, Cmd::SkipNext]), "<foo> bar foo baz <foo>");
        assert_eq!(run("f|oo food foo", vec![Cmd::SelectAllOccurrences, Cmd::Insert("x".into())]), "x| food x|");
        assert_eq!(run("a|b\ncd\nef", vec![Cmd::CursorBelow, Cmd::CursorBelow, Cmd::Insert("-".into())]), "a-|b\nc-|d\ne-|f");
    }

    #[test]
    fn line_ops() {
        assert_eq!(run("one\ntw|o\nthree", vec![Cmd::MoveLinesUp]), "tw|o\none\nthree");
        assert_eq!(run("one\ntw|o\nthree", vec![Cmd::MoveLinesDown]), "one\nthree\ntw|o");
        assert_eq!(run("one\ntw|o\nthree", vec![Cmd::Duplicate]), "one\ntwo\ntw|o\nthree");
        assert_eq!(run("one\ntw|o\nthree", vec![Cmd::DeleteLine]), "one\n|three");
        assert_eq!(run("one\ntwo\nth|ree", vec![Cmd::DeleteLine]), "one\ntwo|");
        assert_eq!(run("a|\n   b", vec![Cmd::JoinLines]), "a| b");
        assert_eq!(run("x|", vec![Cmd::LineAfter]), "x\n|");
        assert_eq!(run("b\na|\nc", vec![Cmd::SortLines]), "|a\nb\nc");
    }

    #[test]
    fn comments_and_indent() {
        assert_eq!(run("  le|t a;", vec![Cmd::ToggleComment]), "  // le|t a;");
        assert_eq!(run("  // le|t a;", vec![Cmd::ToggleComment]), "  le|t a;");
        assert_eq!(run("a|", vec![Cmd::Indent]), "    a|");
        assert_eq!(run("    a|", vec![Cmd::Unindent]), "a|");
        assert_eq!(run("ab|", vec![Cmd::Tab]), "ab  |");
    }

    #[test]
    fn movement() {
        assert_eq!(run("foo bar|", vec![Cmd::WordLeft(false)]), "foo |bar");
        assert_eq!(run("|foo bar", vec![Cmd::WordRight(true)]), "<foo> bar");
        assert_eq!(run("fooBar|", vec![Cmd::SubwordLeft(false)]), "foo|Bar");
        assert_eq!(run("  ab|", vec![Cmd::Home(false)]), "  |ab");
        assert_eq!(run("  |ab", vec![Cmd::Home(false)]), "|  ab");
        assert_eq!(run("f(a, |b)", vec![Cmd::ExpandToBrackets]), "f(<a, b>)");
        assert_eq!(run("abcdef\nab|\nabcdef", vec![Cmd::Up(false)]), "ab|cdef\nab\nabcdef");
        assert_eq!(run("x (|a) y", vec![Cmd::Left(false), Cmd::MatchBracket]), "x (a|) y");
    }

    #[test]
    fn indent_detection() {
        assert_eq!(detect_indent("a:\n  b:\n    c: 1\n"), Some((true, 2)));
        assert_eq!(detect_indent("a\n\tb\n\tc\n"), Some((false, 4)));
        assert_eq!(detect_indent("a\nb\n"), None);
    }
}
