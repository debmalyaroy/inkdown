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
    let found = find_by_name(ss, file, ext, alias, first_line);
    // text that reads like a log is coloured as one, whatever the file is called
    if found.name == "Plain Text" && crate::logs::looks_like_log(first_line) {
        return ss.find_syntax_by_extension("log").unwrap_or(found);
    }
    found
}

fn find_by_name(ss: &'static SyntaxSet, file: &str, ext: &str, alias: Option<&str>, first_line: &str) -> &'static SyntaxReference {
    ss.find_syntax_by_extension(file)
        .or_else(|| ss.find_syntax_by_extension(ext))
        .or_else(|| alias.and_then(|a| ss.find_syntax_by_extension(a)))
        .or_else(|| ss.find_syntax_by_first_line(first_line))
        .or_else(|| if matches!(ext, "out" | "err" | "trace") || file.contains(".log.") || crate::logs::looks_like_log(first_line) { ss.find_syntax_by_extension("log") } else { None })
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

/// Runs the parser over block `block` of the rope, moving `st` and `stack` to its end.
fn parse_block(rope: &ropey::Rope, block: usize, st: &mut ParseState, stack: &mut ScopeStack) {
    let lines = rope.len_lines();
    let from = block * STEP;
    for line in rope.lines_at(from.min(lines)).take(STEP.min(lines.saturating_sub(from))) {
        // most lines live in one chunk of the rope and need no copy
        let owned;
        let l: &str = match line.as_str() {
            Some(s) => s,
            None => {
                owned = line.to_string();
                &owned
            }
        };
        if l.len() <= MAX_LINE {
            if let Ok(ops) = st.parse_line(l, set()) {
                for (_, op) in ops {
                    let _ = stack.apply(&op);
                }
            }
        }
    }
}

struct Worker {
    rx: std::sync::mpsc::Receiver<Vec<(ParseState, ScopeStack)>>,
    generation: u64,
}

pub struct Highlighter {
    pub syntax: &'static SyntaxReference,
    plain: bool,
    /// Log files are coloured line by line from their severity, without the parser.
    log: bool,
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
        let log = syntax.name.eq_ignore_ascii_case("log");
        Self { syntax, plain, log, checkpoints: Vec::new(), cache: HashMap::new(), approx: HashMap::new(), worker: None, generation: Arc::new(std::sync::atomic::AtomicU64::new(1)), spent: Duration::ZERO, pending: false }
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
        let syntax = self.syntax;
        let spawned = std::thread::Builder::new().name("ink-syntax".into()).spawn(move || {
            let lines = rope.len_lines();
            let blocks = lines.div_ceil(STEP);
            let cancelled = || shared.load(std::sync::atomic::Ordering::Relaxed) != mine;
            let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(8);
            let remaining = blocks.saturating_sub(block);
            if remaining < 4000 || threads < 2 {
                // a file of ordinary size: one pass, results handed over as they come
                let mut batch = Vec::with_capacity(8);
                while block < blocks {
                    parse_block(&rope, block, &mut st, &mut stack);
                    batch.push((st.clone(), stack.clone()));
                    block += 1;
                    if batch.len() >= 8 && (cancelled() || tx.send(std::mem::take(&mut batch)).is_err()) {
                        return;
                    }
                }
                let _ = tx.send(batch);
                return;
            }
            // A very large file is parsed in several parts at once. Only the first part
            // knows the true state it starts in; the others guess "outside everything",
            // which is right unless the part begins inside a comment or string that
            // started earlier. Afterwards the parts are joined in order: where a guess
            // was wrong, that part is parsed again from the true state until both agree,
            // which usually takes a few lines.
            let per = remaining.div_ceil(threads);
            let bounds: Vec<(usize, usize)> = (0..threads).map(|t| (block + t * per, (block + (t + 1) * per).min(blocks))).filter(|(a, b)| a < b).collect();
            let fresh = (ParseState::new(syntax), ScopeStack::new());
            std::thread::scope(|scope| {
                let guessed: Vec<_> = bounds[1..]
                    .iter()
                    .map(|&(a, b)| {
                        let (rope, fresh, cancelled) = (&rope, fresh.clone(), &cancelled);
                        scope.spawn(move || {
                            let (mut st, mut stack) = fresh;
                            let mut out = Vec::with_capacity(b - a);
                            for blk in a..b {
                                if blk % 64 == 0 && cancelled() {
                                    break;
                                }
                                parse_block(rope, blk, &mut st, &mut stack);
                                out.push((st.clone(), stack.clone()));
                            }
                            out
                        })
                    })
                    .collect();
                // the first part, from the known state, handed over as it comes
                let (a, b) = bounds[0];
                let mut batch = Vec::with_capacity(64);
                for blk in a..b {
                    parse_block(&rope, blk, &mut st, &mut stack);
                    batch.push((st.clone(), stack.clone()));
                    if batch.len() >= 64 && (cancelled() || tx.send(std::mem::take(&mut batch)).is_err()) {
                        return;
                    }
                }
                if tx.send(batch).is_err() {
                    return;
                }
                for (part, handle) in guessed.into_iter().enumerate() {
                    let Ok(mut guess) = handle.join() else { return };
                    let (a, b) = bounds[part + 1];
                    if cancelled() || guess.len() != b - a {
                        return;
                    }
                    if (st.clone(), stack.clone()) != fresh {
                        // the guess was wrong at the start: parse again until both agree
                        let mut agreed = None;
                        for (k, blk) in (a..b).enumerate() {
                            if k % 64 == 0 && cancelled() {
                                return;
                            }
                            parse_block(&rope, blk, &mut st, &mut stack);
                            if guess[k].0 == st && guess[k].1 == stack {
                                agreed = Some(k);
                                break;
                            }
                            guess[k] = (st.clone(), stack.clone());
                        }
                        if agreed.is_none() && guess.is_empty() {
                            return;
                        }
                    }
                    if let Some(last) = guess.last() {
                        st = last.0.clone();
                        stack = last.1.clone();
                    }
                    if tx.send(guess).is_err() {
                        return;
                    }
                }
            });
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
        if self.log {
            let text = Self::line(buf, line);
            let text = text.trim_end_matches(['\n', '\r']);
            // a stack frame or wrapped line takes the colour of the entry above it
            let inherited = if crate::logs::continues(text) {
                (line.saturating_sub(300)..line).rev().map(|i| Self::line(buf, i)).find(|l| !crate::logs::continues(l.trim_end_matches(['\n', '\r']))).and_then(|l| crate::logs::find_level(&l).map(|f| f.2))
            } else {
                None
            };
            let spans: Spans = crate::logs::spans(text, inherited).into();
            if self.cache.len() > 6000 {
                self.cache.clear();
            }
            self.cache.insert(line, spans.clone());
            return Some(spans);
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

    /// A very large file is parsed in parts at once; the joined result must be exactly
    /// what one pass from the top gives, also where a part begins inside a comment.
    #[test]
    fn large_files_parse_in_parts_like_in_one_pass() {
        let mut text = String::new();
        let mut i = 0;
        // 160 000 lines: ordinary code, with block comments and strings of every length,
        // some of them thousands of lines long so that they cross the borders of the parts
        while i < 160_000 {
            text.push_str(&format!("pub fn item_{i}(x: u32) -> u32 {{ x + {i} }} // tail\n"));
            i += 1;
            if i % 9973 == 0 {
                text.push_str("/* a long comment starts\n");
                for k in 0..7000 {
                    text.push_str(&format!("   fn not_code_{k}() {{ \"still comment\" }}\n"));
                }
                text.push_str("   and ends */ let s = \"a string\n");
                for k in 0..300 {
                    text.push_str(&format!("   that goes on {k} // not a comment\n"));
                }
                text.push_str("   until here\";\n");
                i += 7303;
            }
        }
        let buf = Buffer::new(&text);
        let syntax = find("big.rs", "");
        assert_eq!(syntax.name, "Rust");
        let mut h = Highlighter::new(syntax);
        // asking for the last line starts the background parse
        let last = buf.lines() - 1;
        h.begin_frame();
        let _ = h.spans(&buf, last, Duration::from_millis(50));
        assert!(h.worker.is_some());
        let started = Instant::now();
        while h.worker.is_some() && started.elapsed() < Duration::from_secs(300) {
            std::thread::sleep(Duration::from_millis(20));
            h.begin_frame();
        }
        let blocks = buf.lines().div_ceil(STEP);
        assert_eq!(h.checkpoints.len(), blocks + 1, "every block has its state");
        // one pass from the top
        let (mut st, mut stack) = (ParseState::new(syntax), ScopeStack::new());
        let mut inside_comment = 0;
        for b in 0..blocks {
            assert!(h.checkpoints[b].0 == st && h.checkpoints[b].1 == stack, "state at block {b} of {blocks} differs");
            if stack.as_slice().iter().any(|s| format!("{s}").starts_with("comment")) {
                inside_comment += 1;
            }
            parse_block(&buf.rope, b, &mut st, &mut stack);
        }
        assert!(h.checkpoints[blocks].0 == st && h.checkpoints[blocks].1 == stack);
        assert!(inside_comment > 1000, "the test text really has long comments ({inside_comment} blocks start inside one)");
    }

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
