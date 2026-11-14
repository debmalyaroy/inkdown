//! Text buffer: a rope with multiple selections and grouped undo.
//! All positions are char indices into the rope. Text is stored with `\n` line ends.

use ropey::Rope;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sel {
    pub anchor: usize,
    pub head: usize,
    /// Preferred visual column for vertical movement.
    pub goal: Option<usize>,
}

impl Sel {
    pub fn caret(p: usize) -> Self {
        Self { anchor: p, head: p, goal: None }
    }
    pub fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head, goal: None }
    }
    pub fn start(&self) -> usize {
        self.anchor.min(self.head)
    }
    pub fn end(&self) -> usize {
        self.anchor.max(self.head)
    }
    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
}

#[derive(Clone, Debug)]
struct Change {
    pos: usize,
    removed: String,
    inserted: String,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EditKind {
    Type,
    Delete,
    Other,
}

struct Txn {
    id: u64,
    /// Each group was applied back to front; positions are in the coordinates before that group.
    groups: Vec<Vec<Change>>,
    before: Vec<Sel>,
    after: Vec<Sel>,
    kind: EditKind,
    time: f64,
}

/// One replacement: chars `start..end` become `text`.
#[derive(Clone, Debug)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

impl Edit {
    pub fn new(start: usize, end: usize, text: impl Into<String>) -> Self {
        Self { start, end, text: text.into() }
    }
}

pub struct Buffer {
    pub rope: Rope,
    pub sels: Vec<Sel>,
    undo: Vec<Txn>,
    redo: Vec<Txn>,
    next_id: u64,
    saved_id: u64,
    /// Bumped on every text change.
    pub version: u64,
    /// Lowest line touched since the last `take_dirty_line`.
    dirty_line: Option<usize>,
    pub now: f64,
    /// Ctrl+D started from a caret: later matches must be whole words.
    pub occur_word: bool,
    pub primary_last: bool,
}

impl Buffer {
    pub fn new(text: &str) -> Self {
        Self { rope: Rope::from_str(text), sels: vec![Sel::caret(0)], undo: Vec::new(), redo: Vec::new(), next_id: 1, saved_id: 0, version: 0, dirty_line: None, now: 0.0, occur_word: false, primary_last: false }
    }

    pub fn len(&self) -> usize {
        self.rope.len_chars()
    }
    pub fn lines(&self) -> usize {
        self.rope.len_lines()
    }
    pub fn text(&self) -> String {
        self.rope.to_string()
    }
    pub fn slice(&self, a: usize, b: usize) -> String {
        let (a, b) = (a.min(self.len()), b.min(self.len()));
        self.rope.slice(a..b.max(a)).to_string()
    }
    pub fn char_at(&self, p: usize) -> Option<char> {
        (p < self.len()).then(|| self.rope.char(p))
    }
    pub fn line_of(&self, p: usize) -> usize {
        self.rope.char_to_line(p.min(self.len()))
    }
    pub fn line_start(&self, line: usize) -> usize {
        self.rope.line_to_char(line.min(self.lines().saturating_sub(1)))
    }
    /// End of the line, before its newline.
    pub fn line_end(&self, line: usize) -> usize {
        let line = line.min(self.lines().saturating_sub(1));
        let l = self.rope.line(line);
        let mut n = l.len_chars();
        if n > 0 && l.char(n - 1) == '\n' {
            n -= 1;
        }
        self.rope.line_to_char(line) + n
    }
    pub fn line_len(&self, line: usize) -> usize {
        self.line_end(line) - self.line_start(line)
    }
    /// Line text without the newline.
    pub fn line_text(&self, line: usize) -> String {
        self.slice(self.line_start(line), self.line_end(line))
    }
    pub fn pos_of(&self, line: usize, col: usize) -> usize {
        let line = line.min(self.lines().saturating_sub(1));
        self.line_start(line) + col.min(self.line_len(line))
    }
    pub fn line_col(&self, p: usize) -> (usize, usize) {
        let l = self.line_of(p);
        (l, p.min(self.len()) - self.line_start(l))
    }

    pub fn dirty(&self) -> bool {
        self.current_id() != self.saved_id
    }
    pub fn mark_saved(&mut self) {
        self.saved_id = self.current_id();
        // typing after a save must not merge into the saved transaction
        if let Some(t) = self.undo.last_mut() {
            t.kind = EditKind::Other;
        }
    }
    fn current_id(&self) -> u64 {
        self.undo.last().map(|t| t.id).unwrap_or(0)
    }
    pub fn take_dirty_line(&mut self) -> Option<usize> {
        self.dirty_line.take()
    }
    fn touch(&mut self, line: usize) {
        self.version += 1;
        self.dirty_line = Some(self.dirty_line.map_or(line, |d| d.min(line)));
    }

    pub fn primary(&self) -> Sel {
        *self.sels.last().unwrap_or(&Sel::caret(0))
    }
    pub fn set_caret(&mut self, p: usize) {
        self.sels = vec![Sel::caret(p.min(self.len()))];
    }
    pub fn set_sel(&mut self, anchor: usize, head: usize) {
        self.sels = vec![Sel::new(anchor.min(self.len()), head.min(self.len()))];
    }

    /// Sort selections and merge the ones that overlap or touch as carets.
    pub fn normalize(&mut self) {
        let len = self.len();
        for s in &mut self.sels {
            s.anchor = s.anchor.min(len);
            s.head = s.head.min(len);
        }
        if self.sels.len() < 2 {
            if self.sels.is_empty() {
                self.sels.push(Sel::caret(0));
            }
            return;
        }
        self.sels.sort_by_key(|s| (s.start(), s.end()));
        let mut out: Vec<Sel> = Vec::with_capacity(self.sels.len());
        for s in self.sels.drain(..) {
            if let Some(last) = out.last_mut() {
                let overlap = s.start() < last.end() || (s.start() == last.end() && (s.is_empty() || last.is_empty()));
                if overlap {
                    let (a, b) = (last.start().min(s.start()), last.end().max(s.end()));
                    let forward = s.head >= s.anchor;
                    *last = if forward { Sel::new(a, b) } else { Sel::new(b, a) };
                    continue;
                }
            }
            out.push(s);
        }
        self.sels = out;
    }

    fn apply_group(&mut self, group: &[Change]) {
        for c in group {
            let n = c.removed.chars().count();
            if n > 0 {
                self.rope.remove(c.pos..c.pos + n);
            }
            if !c.inserted.is_empty() {
                self.rope.insert(c.pos, &c.inserted);
            }
        }
    }
    fn revert_group(&mut self, group: &[Change]) {
        for c in group.iter().rev() {
            let n = c.inserted.chars().count();
            if n > 0 {
                self.rope.remove(c.pos..c.pos + n);
            }
            if !c.removed.is_empty() {
                self.rope.insert(c.pos, &c.removed);
            }
        }
    }

    /// Apply replacements given in current coordinates. Overlapping edits are dropped.
    /// Selections are mapped through the edit; callers usually set them afterwards.
    pub fn edit(&mut self, mut edits: Vec<Edit>, kind: EditKind) -> bool {
        let len = self.len();
        edits.retain(|e| e.start <= e.end && e.end <= len && !(e.start == e.end && e.text.is_empty()));
        if edits.is_empty() {
            return false;
        }
        edits.sort_by_key(|e| (e.start, e.end));
        let mut clean: Vec<Edit> = Vec::with_capacity(edits.len());
        for e in edits {
            if clean.last().is_some_and(|l| e.start < l.end) {
                continue;
            }
            clean.push(e);
        }
        let before = self.sels.clone();
        let first_line = self.line_of(clean[0].start);
        // map selections
        let map = |p: usize, after: bool| -> usize {
            let mut delta: isize = 0;
            for e in &clean {
                let ins = e.text.chars().count() as isize;
                if p > e.end || (p == e.end && (after || e.start < e.end)) {
                    delta += ins - (e.end - e.start) as isize;
                } else if p >= e.start {
                    return (e.start as isize + delta + ins) as usize;
                } else {
                    break;
                }
            }
            (p as isize + delta) as usize
        };
        let mapped: Vec<Sel> = self.sels.iter().map(|s| Sel::new(map(s.anchor, true), map(s.head, true))).collect();
        let group: Vec<Change> = clean.iter().rev().map(|e| Change { pos: e.start, removed: self.rope.slice(e.start..e.end).to_string(), inserted: e.text.clone() }).collect();
        self.apply_group(&group);
        self.sels = mapped;
        self.touch(first_line);
        self.redo.clear();
        let id = self.next_id;
        self.next_id += 1;
        let mergeable = kind != EditKind::Other
            && self.undo.last().is_some_and(|t| t.kind == kind && self.now - t.time < 1.2 && t.id != self.saved_id && t.groups.len() < 200)
            && group.iter().all(|c| !c.inserted.contains('\n') && !c.removed.contains('\n') && c.inserted != " ");
        if mergeable {
            let t = self.undo.last_mut().unwrap();
            t.groups.push(group);
            t.time = self.now;
            t.id = id;
        } else {
            self.undo.push(Txn { id, groups: vec![group], before, after: Vec::new(), kind, time: self.now });
            if self.undo.len() > 5000 {
                self.undo.remove(0);
            }
        }
        true
    }

    /// Record the selections that belong with the last edit (restored by redo).
    pub fn commit_sels(&mut self) {
        self.normalize();
        if let Some(t) = self.undo.last_mut() {
            t.after = self.sels.clone();
        }
    }

    pub fn undo(&mut self) -> bool {
        let Some(t) = self.undo.pop() else { return false };
        let mut line = usize::MAX;
        for g in t.groups.iter().rev() {
            self.revert_group(g);
            if let Some(c) = g.last() {
                line = line.min(self.line_of(c.pos.min(self.len())));
            }
        }
        self.sels = t.before.clone();
        self.touch(if line == usize::MAX { 0 } else { line });
        self.redo.push(t);
        self.normalize();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(t) = self.redo.pop() else { return false };
        let mut line = usize::MAX;
        for g in &t.groups {
            if let Some(c) = g.last() {
                line = line.min(self.line_of(c.pos.min(self.len())));
            }
            self.apply_group(g);
        }
        if !t.after.is_empty() {
            self.sels = t.after.clone();
        }
        self.touch(if line == usize::MAX { 0 } else { line });
        self.undo.push(t);
        self.normalize();
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Replace the whole text as one undoable step (used by formatters and external reloads).
    pub fn set_text(&mut self, text: &str) {
        let len = self.len();
        let caret = self.primary().head;
        self.edit(vec![Edit::new(0, len, text)], EditKind::Other);
        self.set_caret(caret.min(self.len()));
        self.commit_sels();
    }
}

pub fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_undo_redo() {
        let mut b = Buffer::new("hello world\nsecond");
        b.sels = vec![Sel::caret(5), Sel::caret(11)];
        b.edit(vec![Edit::new(5, 5, "!"), Edit::new(11, 11, "?")], EditKind::Other);
        assert_eq!(b.text(), "hello! world?\nsecond");
        assert_eq!(b.sels.iter().map(|s| s.head).collect::<Vec<_>>(), vec![6, 13]);
        assert!(b.dirty());
        b.undo();
        assert_eq!(b.text(), "hello world\nsecond");
        assert!(!b.dirty());
        b.redo();
        assert_eq!(b.text(), "hello! world?\nsecond");
        b.edit(vec![Edit::new(0, 6, "")], EditKind::Other);
        assert_eq!(b.text(), " world?\nsecond");
        b.undo();
        b.undo();
        assert_eq!(b.text(), "hello world\nsecond");
        assert_eq!(b.line_col(14), (1, 2));
        assert_eq!(b.line_end(0), 11);
        assert_eq!(b.line_text(1), "second");
    }

    #[test]
    fn typing_merges() {
        let mut b = Buffer::new("");
        for (i, ch) in "abc".chars().enumerate() {
            b.edit(vec![Edit::new(i, i, ch.to_string())], EditKind::Type);
        }
        assert_eq!(b.text(), "abc");
        b.undo();
        assert_eq!(b.text(), "");
    }

    #[test]
    fn normalize_merges() {
        let mut b = Buffer::new("abcdefgh");
        b.sels = vec![Sel::new(4, 6), Sel::new(0, 5), Sel::caret(8)];
        b.normalize();
        assert_eq!(b.sels, vec![Sel::new(0, 6), Sel::caret(8)]);
    }
}
