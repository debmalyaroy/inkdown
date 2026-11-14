//! Incremental syntax highlighting on Sublime grammars (syntect, pure-Rust regex engine).
//!
//! Parser state is checkpointed every `STEP` lines, so an edit only re-parses from the
//! checkpoint before it, and a file is parsed lazily within a per-frame time budget.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

use crate::buffer::Buffer;
use crate::theme::{ScopeColors, TOK_TEXT};

const STEP: usize = 32;
const MAX_LINE: usize = 3000;

/// `(end byte offset within the line, token class)`
pub type Span = (u32, u8);
pub type Spans = Arc<[Span]>;

pub fn set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(|| {
        let base = two_face::syntax::extra_newlines();
        match user_syntax_dir().filter(|d| d.is_dir()) {
            Some(dir) => {
                let mut b = base.into_builder();
                let _ = b.add_from_folder(dir, true);
                b.build()
            }
            None => base,
        }
    })
}

/// `<config>/Packages`: drop `.sublime-syntax` files here to add languages.
pub fn user_syntax_dir() -> Option<std::path::PathBuf> {
    Some(crate::settings::config_dir().join("Packages"))
}

struct Classifier {
    colors: ScopeColors,
    cache: HashMap<Vec<Scope>, u8>,
}

fn classify(stack: &[Scope]) -> u8 {
    static C: OnceLock<Mutex<Classifier>> = OnceLock::new();
    let mut c = C.get_or_init(|| Mutex::new(Classifier { colors: ScopeColors::new(), cache: HashMap::new() })).lock().unwrap();
    if let Some(t) = c.cache.get(stack) {
        return *t;
    }
    let t = c.colors.classify(stack);
    if c.cache.len() > 20_000 {
        c.cache.clear();
    }
    c.cache.insert(stack.to_vec(), t);
    t
}

/// Names of all known syntaxes, sorted, for the syntax picker.
pub fn names() -> Vec<String> {
    let mut v: Vec<String> = set().syntaxes().iter().filter(|s| !s.hidden).map(|s| s.name.clone()).collect();
    v.sort_by_key(|s| s.to_lowercase());
    v.dedup();
    v
}

pub fn find(name: &str, first_line: &str) -> &'static SyntaxReference {
    let ss = set();
    let lower = name.to_lowercase();
    let file = lower.rsplit(['/', '\\']).next().unwrap_or(&lower);
    let ext = file.rsplit('.').next().unwrap_or("");
    let alias = match ext {
        "mdx" | "markdown" | "mdown" | "mkd" => Some("md"),
        "jsonc" | "json5" | "geojson" | "ipynb" | "webmanifest" | "avsc" | "avro" => Some("json"),
        "mjs" | "cjs" => Some("js"),
        "mts" | "cts" => Some("ts"),
        "zsh" | "fish" | "ksh" | "bashrc" | "zshrc" | "profile" | "env" => Some("sh"),
        "h" => Some("c"),
        "hpp" | "hh" | "hxx" | "cc" | "cxx" | "ino" => Some("cpp"),
        "htm" | "xhtml" | "vue" | "svelte" => Some("html"),
        "svg" | "xsl" | "xsd" | "plist" | "csproj" | "vcxproj" | "resx" | "xaml" => Some("xml"),
        "yml" => Some("yaml"),
        "lock" if file == "cargo.lock" => Some("toml"),
        "gradle" => Some("groovy"),
        "kts" => Some("kt"),
        "conf" | "cfg" | "properties" | "editorconfig" => Some("ini"),
        "psm1" | "psd1" => Some("ps1"),
        "tf" | "tfvars" => Some("hcl"),
        "mmd" | "mermaid" | "gv" | "adoc" | "asciidoc" => None,
        _ => None,
    };
    ss.find_syntax_by_extension(file)
        .or_else(|| ss.find_syntax_by_extension(ext))
        .or_else(|| alias.and_then(|a| ss.find_syntax_by_extension(a)))
        .or_else(|| ss.find_syntax_by_first_line(first_line))
        .unwrap_or_else(|| ss.find_syntax_plain_text())
}

pub fn by_name(name: &str) -> Option<&'static SyntaxReference> {
    let ss = set();
    ss.find_syntax_by_name(name).or_else(|| ss.find_syntax_by_token(name))
}

fn parse_line(state: &mut ParseState, stack: &mut ScopeStack, line: &str) -> Spans {
    if line.len() > MAX_LINE {
        return Arc::from([(line.len() as u32, TOK_TEXT)]);
    }
    let Ok(ops) = state.parse_line(line, set()) else {
        return Arc::from([(line.len() as u32, TOK_TEXT)]);
    };
    let mut out: Vec<Span> = Vec::with_capacity(ops.len() + 1);
    let mut last = 0usize;
    let mut push = |end: usize, tok: u8, out: &mut Vec<Span>| {
        if let Some(l) = out.last_mut() {
            if l.1 == tok {
                l.0 = end as u32;
                return;
            }
        }
        out.push((end as u32, tok));
    };
    for (pos, op) in ops {
        if pos > last {
            push(pos, classify(stack.as_slice()), &mut out);
            last = pos;
        }
        let _ = stack.apply(&op);
    }
    if line.len() > last {
        push(line.len(), classify(stack.as_slice()), &mut out);
    }
    out.into()
}

struct Worker {
    rx: std::sync::mpsc::Receiver<Vec<(ParseState, ScopeStack)>>,
    generation: u64,
}

pub struct Highlighter {
    pub syntax: &'static SyntaxReference,
    plain: bool,
    /// Parser state at the start of every `STEP`-line block, as far as it is known.
    checkpoints: Vec<(ParseState, ScopeStack)>,
    cache: HashMap<usize, Spans>,
    /// Lines beyond the known state, highlighted from a fresh state at their block start.
    /// Right unless the block begins inside a multi-line construct; replaced when the
    /// background parse reaches them.
    approx: HashMap<usize, Spans>,
    worker: Option<Worker>,
    generation: Arc<std::sync::atomic::AtomicU64>,
    spent: Duration,
    /// True while some visible lines are approximate or not highlighted yet.
    pub pending: bool,
}

impl Drop for Highlighter {
    fn drop(&mut self) {
        // stops the background parse
        self.generation.store(u64::MAX, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Highlighter {
    pub fn new(syntax: &'static SyntaxReference) -> Self {
        let plain = syntax.name == "Plain Text";
        Self { syntax, plain, checkpoints: Vec::new(), cache: HashMap::new(), approx: HashMap::new(), worker: None, generation: Arc::new(std::sync::atomic::AtomicU64::new(1)), spent: Duration::ZERO, pending: false }
    }

    pub fn name(&self) -> &str {
        &self.syntax.name
    }

    pub fn invalidate(&mut self, line: usize) {
        self.checkpoints.truncate(line / STEP + 1);
        self.cache.retain(|l, _| *l < line);
        self.approx.clear();
        // results of a parse of the old text must not be used
        self.generation.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.worker = None;
    }

    /// Call once per frame before asking for spans.
    pub fn begin_frame(&mut self) {
        self.spent = Duration::ZERO;
        self.pending = false;
        let Some(w) = &self.worker else { return };
        let current = self.generation.load(std::sync::atomic::Ordering::Relaxed);
        let mut got = false;
        let mut done = false;
        loop {
            match w.rx.try_recv() {
                Ok(batch) => {
                    if w.generation == current {
                        self.checkpoints.extend(batch);
                        got = true;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(_) => {
                    done = true;
                    break;
                }
            }
        }
        if got {
            let frontier = (self.checkpoints.len().saturating_sub(1)) * STEP;
            self.approx.retain(|l, _| *l >= frontier);
        }
        if done {
            self.worker = None;
        }
    }

    fn line(buf: &Buffer, i: usize) -> String {
        buf.rope.line(i).to_string()
    }

    /// Parse the rest of the file on another thread, starting from the last known state.
    fn start_worker(&mut self, buf: &Buffer) {
        if self.worker.is_some() || self.checkpoints.is_empty() {
            return;
        }
        let rope = buf.rope.clone(); // cheap: ropes share their data
        let (mut st, mut stack) = self.checkpoints.last().cloned().unwrap();
        let mut block = self.checkpoints.len() - 1;
        let shared = self.generation.clone();
        let mine = shared.load(std::sync::atomic::Ordering::Relaxed);
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new().name("ink-syntax".into()).spawn(move || {
            let lines = rope.len_lines();
            let mut batch = Vec::with_capacity(8);
            loop {
                let from = block * STEP;
                if from >= lines {
                    break;
                }
                for i in from..(from + STEP).min(lines) {
                    let l = rope.line(i).to_string();
                    if l.len() <= MAX_LINE {
                        if let Ok(ops) = st.parse_line(&l, set()) {
                            for (_, op) in ops {
                                let _ = stack.apply(&op);
                            }
                        }
                    }
                }
                batch.push((st.clone(), stack.clone()));
                block += 1;
                if batch.len() >= 8 {
                    if shared.load(std::sync::atomic::Ordering::Relaxed) != mine || tx.send(std::mem::take(&mut batch)).is_err() {
                        return;
                    }
                }
            }
            let _ = tx.send(batch);
        });
        if spawned.is_ok() {
            self.worker = Some(Worker { rx, generation: mine });
        }
    }

    /// Spans for `line`. Returns `None` when there is no time left this frame.
    pub fn spans(&mut self, buf: &Buffer, line: usize, budget: Duration) -> Option<Spans> {
        if self.plain || line >= buf.lines() {
            return None;
        }
        if let Some(s) = self.cache.get(&line) {
            return Some(s.clone());
        }
        let block = line / STEP;
        if self.checkpoints.is_empty() {
            self.checkpoints.push((ParseState::new(self.syntax), ScopeStack::new()));
        }
        let from = block * STEP;
        let exact = block < self.checkpoints.len();
        if !exact {
            self.pending = true;
            self.start_worker(buf);
            if let Some(s) = self.approx.get(&line) {
                return Some(s.clone());
            }
        }
        if self.spent > budget {
            self.pending = true;
            return None;
        }
        let started = Instant::now();
        let (mut st, mut stack) = if exact { self.checkpoints[block].clone() } else { (ParseState::new(self.syntax), ScopeStack::new()) };
        let mut found = None;
        for i in from..(from + STEP).min(buf.lines()) {
            let spans = parse_line(&mut st, &mut stack, &Self::line(buf, i));
            if i == line {
                found = Some(spans.clone());
            }
            if exact {
                self.cache.insert(i, spans);
            } else {
                self.approx.insert(i, spans);
            }
        }
        // the state after an exact block is the next checkpoint: small files never need the worker
        if exact && block + 1 == self.checkpoints.len() && self.worker.is_none() && from + STEP < buf.lines() {
            self.checkpoints.push((st, stack));
        }
        self.spent += started.elapsed();
        if self.cache.len() > 6000 {
            let keep_from = line.saturating_sub(1500);
            self.cache.retain(|l, _| *l >= keep_from && *l <= line + 1500);
        }
        if self.approx.len() > 4000 {
            let keep_from = line.saturating_sub(600);
            self.approx.retain(|l, _| *l >= keep_from && *l <= line + 600);
        }
        found
    }
    /// Scope names at a position (for comment detection and symbol lists).
    pub fn symbols(&self, buf: &Buffer, limit: Duration) -> Vec<(usize, String)> {
        let mut out = Vec::new();
        if self.plain {
            return out;
        }
        let start = Instant::now();
        let mut st = ParseState::new(self.syntax);
        let mut stack = ScopeStack::new();
        let wanted = |s: &Scope| {
            let n = s.build_string();
            n.starts_with("entity.name.function") || n.starts_with("entity.name.class") || n.starts_with("entity.name.struct") || n.starts_with("entity.name.enum") || n.starts_with("entity.name.trait") || n.starts_with("entity.name.type") || n.starts_with("entity.name.section") || n.starts_with("entity.name.namespace") || n.starts_with("entity.name.impl") || n.starts_with("entity.name.interface") || n.starts_with("markup.heading")
        };
        for i in 0..buf.lines() {
            let line = Self::line(buf, i);
            if line.len() > MAX_LINE {
                continue;
            }
            let Ok(ops) = st.parse_line(&line, set()) else { continue };
            let mut last = 0;
            let mut name = String::new();
            for (pos, op) in ops {
                if pos > last && stack.as_slice().iter().any(wanted) {
                    name.push_str(&line[last..pos]);
                }
                last = pos;
                let _ = stack.apply(&op);
            }
            if line.len() > last && stack.as_slice().iter().any(wanted) {
                name.push_str(&line[last..]);
            }
            let name = name.trim().trim_start_matches('#').trim();
            if !name.is_empty() {
                out.push((i, name.to_string()));
            }
            if i % 256 == 0 && start.elapsed() > limit {
                break;
            }
        }
        out
    }
}

/// Highlight a standalone snippet (fenced code blocks). Returns spans per line.
pub fn highlight_snippet(lang: &str, code: &str) -> Vec<Spans> {
    let ss = set();
    let token = lang.split([',', ' ', '{']).next().unwrap_or("").trim().to_lowercase();
    let syntax = ss.find_syntax_by_token(&token).or_else(|| ss.find_syntax_by_extension(&token)).unwrap_or_else(|| find(&format!("x.{token}"), code.lines().next().unwrap_or("")));
    let mut st = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let plain = syntax.name == "Plain Text" || code.len() > 400_000;
    code.split_inclusive('\n')
        .map(|l| if plain { Arc::from([(l.len() as u32, TOK_TEXT)]) } else { parse_line(&mut st, &mut stack, l) })
        .collect()
}

/// Line-comment and block-comment markers for a syntax, by name.
pub fn comment_tokens(syntax_name: &str) -> (Option<&'static str>, Option<(&'static str, &'static str)>) {
    let n = syntax_name.to_lowercase();
    let has = |xs: &[&str]| xs.iter().any(|x| n.contains(x));
    if has(&["html", "xml", "markdown", "svg", "vue", "svelte"]) {
        (None, Some(("<!--", "-->")))
    } else if has(&["css"]) && !has(&["scss", "less", "sass"]) {
        (None, Some(("/*", "*/")))
    } else if has(&["python", "shell", "bash", "yaml", "toml", "ruby", "perl", "makefile", "dockerfile", "powershell", "r ", "ini", "conf", "elixir", "nix", "cmake", "git", "tcl", "julia", "crystal", "graphql", "properties", "requirements", "nginx", "fish", "awk", "terraform", "hcl", "plain text"]) || n == "r" {
        (Some("#"), None)
    } else if has(&["sql", "lua", "haskell", "ada", "elm"]) {
        (Some("--"), if n.contains("lua") { Some(("--[[", "]]")) } else { None })
    } else if has(&["lisp", "clojure", "scheme", "assembly", "racket"]) {
        (Some(";"), None)
    } else if has(&["latex", "tex", "erlang", "matlab", "prolog"]) {
        (Some("%"), None)
    } else if has(&["batch", "dos"]) {
        (Some("REM "), None)
    } else if has(&["vim"]) {
        (Some("\""), None)
    } else if has(&["json"]) && !has(&["jsonc", "json5"]) {
        (Some("//"), None)
    } else {
        (Some("//"), Some(("/*", "*/")))
    }
}

#[cfg(test)]
mod bench {
    use super::*;

    #[test]
    #[ignore = "timing only"]
    fn parse_speed() {
        let mut text = String::new();
        for i in 0..4000 {
            text.push_str(&format!("fn item_{i}(x: u32) -> u32 {{ x + {i} }} // line {i}\n"));
        }
        for name in ["x.rs", "x.py", "x.js", "x.md", "x.json"] {
            let syntax = find(name, "");
            let mut st = ParseState::new(syntax);
            let mut stack = ScopeStack::new();
            let t = Instant::now();
            let mut ops = 0usize;
            for line in text.split_inclusive('\n') {
                ops += st.parse_line(line, set()).map(|o| o.len()).unwrap_or(0);
            }
            let raw = t.elapsed();
            let mut st = ParseState::new(syntax);
            let t = Instant::now();
            for line in text.split_inclusive('\n') {
                parse_line(&mut st, &mut stack, line);
            }
            println!("{:>12}: raw {:>6.1} ms, with classify {:>6.1} ms for 4000 lines ({} ops)", syntax.name, raw.as_secs_f64() * 1000.0, t.elapsed().as_secs_f64() * 1000.0, ops);
        }
    }
}
