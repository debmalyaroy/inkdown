//! Live documents: large files loaded off the interface thread, files that grow (logs),
//! and documents read from a web address and refreshed as the server changes them.
//!
//! Everything slow happens on a worker thread that reports back with an [`Update`]:
//! - a large file is decoded in pieces straight into the rope, so its bytes, a decoded
//!   copy and the rope never exist together (which used to cost three times the file);
//! - when a file has only grown, only the new part is read and appended;
//! - a web address is asked for what changed (`If-None-Match`, `Range`) where the server
//!   supports it.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::SystemTime;

use encoding_rs::{DecoderResult, Encoding};
use ropey::{Rope, RopeBuilder};

use crate::app::InkApp;
use crate::buffer::Buffer;
use crate::doc::Doc;
use crate::settings::Settings;

/// Files at least this large are opened on a worker thread, with a progress display.
pub const BACKGROUND_FROM: u64 = 24 << 20;
/// Files at least this large are decoded in pieces instead of in one go.
pub const STREAM_FROM: u64 = 4 << 20;
/// The largest file that is opened at all. A document needs about 1.3 times its size in memory.
pub const MAX_FILE: u64 = if cfg!(any(target_os = "android", target_os = "ios")) { 256 << 20 } else { 2 << 30 };
/// The largest document fetched from a web address.
pub const MAX_REMOTE: u64 = 256 << 20;
const CHUNK: usize = 4 << 20;
const SIG: usize = 64;

/// Decoded text and what is needed to continue reading where it ended.
pub struct Text {
    pub rope: Rope,
    pub encoding: &'static Encoding,
    pub bom: bool,
    pub crlf: bool,
    /// Bytes of the source that the rope accounts for.
    pub consumed: u64,
    /// The last bytes read, to recognise later that the source only grew.
    pub sig: Vec<u8>,
}

#[derive(Default, Clone)]
pub struct Meta {
    pub mtime: Option<SystemTime>,
    pub etag: Option<String>,
    pub modified: Option<String>,
    pub hash: u64,
}

pub enum Update {
    Replace(Box<Text>, Meta),
    Append { text: String, crlf: bool, consumed: u64, sig: Vec<u8>, meta: Meta },
    Unchanged(Meta),
    /// The server keeps the connection open and sends text as it is produced.
    Streaming(bool),
    /// Not text: the caller opens it the ordinary way (as a hex listing).
    Binary,
    Failed(String),
}

/// Where a document's text comes from and how far it has been read.
#[derive(Default)]
pub struct Source {
    /// The web address of a remote document.
    pub url: Option<String>,
    /// Keep reading as the source grows, and keep the end in view.
    pub follow: bool,
    /// The first load of a large file is still running.
    pub loading: bool,
    pub progress: Arc<AtomicU64>,
    pub total: u64,
    pub consumed: u64,
    pub sig: Vec<u8>,
    pub etag: Option<String>,
    pub modified: Option<String>,
    pub hash: u64,
    pub job: Option<Receiver<Update>>,
    pub next_check: f64,
    /// Time of the last answer, for the status bar.
    pub checked_at: f64,
    pub error: Option<String>,
    /// Number of refreshes that brought new text (for tests and the status bar).
    pub updates: u32,
    /// The connection to the server is open and text is shown as it arrives.
    pub streaming: bool,
}

impl Source {
    pub fn remote(&self) -> bool {
        self.url.is_some()
    }

    /// Percentage of the first load, while it runs.
    pub fn percent(&self) -> Option<u32> {
        self.loading.then(|| if self.total == 0 { 0 } else { (self.progress.load(Ordering::Relaxed) * 100 / self.total).min(100) as u32 })
    }
}

// ------------------------------------------------------------------ decoding

struct Decoding {
    decoder: encoding_rs::Decoder,
    strict: bool,
    builder: RopeBuilder,
    pending_cr: bool,
    crlf: bool,
    scratch: String,
}

impl Decoding {
    fn new(encoding: &'static Encoding, strict: bool) -> Self {
        Self { decoder: encoding.new_decoder_without_bom_handling(), strict, builder: RopeBuilder::new(), pending_cr: false, crlf: false, scratch: String::new() }
    }

    /// Decodes one piece and adds it to the rope. `Err` means: not valid in this encoding.
    fn feed(&mut self, input: &[u8], last: bool) -> Result<(), ()> {
        self.scratch.clear();
        let need = if self.strict { self.decoder.max_utf8_buffer_length_without_replacement(input.len()) } else { self.decoder.max_utf8_buffer_length(input.len()) };
        self.scratch.reserve(need.unwrap_or(input.len() * 3 + 16));
        if self.strict {
            let (result, _) = self.decoder.decode_to_string_without_replacement(input, &mut self.scratch, last);
            if let DecoderResult::Malformed(..) = result {
                return Err(());
            }
        } else {
            let _ = self.decoder.decode_to_string(input, &mut self.scratch, last);
        }
        let mut piece = std::mem::take(&mut self.scratch);
        self.push(&mut piece);
        self.scratch = piece;
        Ok(())
    }

    /// Adds decoded text, turning `\r\n` and lone `\r` into `\n` across piece borders.
    fn push(&mut self, piece: &mut String) {
        if self.pending_cr {
            self.pending_cr = false;
            if piece.starts_with('\n') {
                self.crlf = true;
            } else {
                self.builder.append("\n");
            }
        }
        if piece.ends_with('\r') {
            piece.pop();
            self.pending_cr = true;
        }
        if piece.contains('\r') {
            if piece.contains("\r\n") {
                self.crlf = true;
            }
            let fixed = piece.replace("\r\n", "\n").replace('\r', "\n");
            self.builder.append(&fixed);
        } else {
            self.builder.append(piece);
        }
    }

    fn finish(mut self) -> (Rope, bool) {
        if self.pending_cr {
            self.builder.append("\n");
        }
        (self.builder.finish(), self.crlf)
    }
}

/// What a file's first bytes say about its encoding: `(encoding, BOM length, strict)`.
/// `None` for binary data.
fn sniff(head: &[u8]) -> Option<(&'static Encoding, usize, bool)> {
    if let Some((enc, bom)) = Encoding::for_bom(head) {
        return Some((enc, bom, false));
    }
    if head.iter().take(8192).any(|b| *b == 0) {
        let n = head.len().min(4096) / 2;
        let zeros_odd = (0..n).filter(|i| head[i * 2 + 1] == 0).count();
        let zeros_even = (0..n).filter(|i| head[i * 2] == 0).count();
        if n > 8 && zeros_odd > n * 7 / 10 {
            return Some((encoding_rs::UTF_16LE, 0, false));
        }
        if n > 8 && zeros_even > n * 7 / 10 {
            return Some((encoding_rs::UTF_16BE, 0, false));
        }
        return None;
    }
    Some((encoding_rs::UTF_8, 0, true))
}

enum ReadError {
    Binary,
    NotUtf8,
    Io(String),
}

fn read_text_as(mut reader: impl Read, forced: Option<&'static Encoding>, progress: &AtomicU64) -> Result<Text, ReadError> {
    let mut chunk = vec![0u8; CHUNK];
    let mut fill = |reader: &mut dyn Read, chunk: &mut [u8]| -> Result<usize, ReadError> {
        let mut n = 0;
        while n < chunk.len() {
            match reader.read(&mut chunk[n..]) {
                Ok(0) => break,
                Ok(k) => n += k,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(ReadError::Io(e.to_string())),
            }
        }
        Ok(n)
    };
    let mut n = fill(&mut reader, &mut chunk)?;
    let (encoding, bom, strict) = match forced {
        Some(e) => (e, 0, false),
        None => sniff(&chunk[..n]).ok_or(ReadError::Binary)?,
    };
    let mut d = Decoding::new(encoding, strict);
    let mut consumed = 0u64;
    let mut sig = Vec::new();
    let mut first = true;
    loop {
        let last = n < chunk.len();
        let body = if first { &chunk[bom.min(n)..n] } else { &chunk[..n] };
        first = false;
        d.feed(body, last).map_err(|_| ReadError::NotUtf8)?;
        consumed += n as u64;
        progress.store(consumed, Ordering::Relaxed);
        if n > 0 {
            sig = sig_after(&sig, &chunk[..n]);
        }
        if last {
            break;
        }
        n = fill(&mut reader, &mut chunk)?;
    }
    let (rope, crlf) = d.finish();
    Ok(Text { rope, encoding, bom: bom > 0, crlf, consumed, sig })
}

/// Decodes text from bytes already in memory (a download).
pub fn text_from_bytes(bytes: &[u8]) -> Option<Text> {
    let progress = AtomicU64::new(0);
    match read_text_as(bytes, None, &progress) {
        Ok(t) => Some(t),
        Err(ReadError::NotUtf8) => read_text_as(bytes, Some(encoding_rs::WINDOWS_1252), &progress).ok(),
        Err(_) => None,
    }
}

/// Reads a text file in pieces. `Ok(None)` means the file is binary.
pub fn load_file(path: &Path, progress: &AtomicU64) -> Result<Option<Text>, String> {
    let open = || std::fs::File::open(path).map_err(|e| format!("Cannot open {}: {e}", path.display()));
    match read_text_as(open()?, None, progress) {
        Ok(t) => Ok(Some(t)),
        Err(ReadError::Binary) => Ok(None),
        // not UTF-8 after all: read it again as Western European, as small files are
        Err(ReadError::NotUtf8) => match read_text_as(open()?, Some(encoding_rs::WINDOWS_1252), progress) {
            Ok(t) => Ok(Some(t)),
            Err(ReadError::Io(e)) => Err(format!("Cannot read {}: {e}", path.display())),
            Err(_) => Ok(None),
        },
        Err(ReadError::Io(e)) => Err(format!("Cannot read {}: {e}", path.display())),
    }
}

/// The complete characters at the start of `bytes`, as text with `\n` line ends, and how
/// many bytes that used. `None` when the bytes are not UTF-8.
fn utf8_prefix(bytes: &[u8]) -> Option<(String, bool, usize)> {
    let valid = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) if e.error_len().is_none() => std::str::from_utf8(&bytes[..e.valid_up_to()]).ok()?,
        Err(_) => return None,
    };
    // a `\r` at the very end may be half of a `\r\n`: leave it for the next read
    let valid = valid.strip_suffix('\r').unwrap_or(valid);
    let crlf = valid.contains("\r\n");
    let text = if valid.contains('\r') { valid.replace("\r\n", "\n").replace('\r', "\n") } else { valid.to_string() };
    Some((text, crlf, valid.len()))
}

fn sig_after(old: &[u8], new: &[u8]) -> Vec<u8> {
    let mut all = old.to_vec();
    all.extend_from_slice(&new[new.len().saturating_sub(SIG)..]);
    all.split_off(all.len().saturating_sub(SIG))
}

/// Looks at a file that changed on disk: appends when it only grew, reloads otherwise.
pub fn refresh_file(path: &Path, consumed: u64, sig: &[u8], utf8: bool, progress: &AtomicU64) -> Update {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) => return Update::Failed(format!("Cannot read {}: {e}", path.display())),
    };
    let info = Meta { mtime: meta.modified().ok(), ..Default::default() };
    let len = meta.len();
    let grown = utf8 && consumed > 0 && len > consumed && len - consumed < MAX_REMOTE;
    if grown {
        let appended = (|| -> Option<Update> {
            let mut f = std::fs::File::open(path).ok()?;
            let back = sig.len() as u64;
            f.seek(SeekFrom::Start(consumed - back)).ok()?;
            let mut before = vec![0u8; sig.len()];
            f.read_exact(&mut before).ok()?;
            if before != sig {
                return None;
            }
            let mut fresh = Vec::with_capacity((len - consumed) as usize);
            f.take(len - consumed).read_to_end(&mut fresh).ok()?;
            let (text, crlf, used) = utf8_prefix(&fresh)?;
            if used == 0 {
                return Some(Update::Unchanged(info.clone()));
            }
            Some(Update::Append { text, crlf, consumed: consumed + used as u64, sig: sig_after(sig, &fresh[..used]), meta: info.clone() })
        })();
        if let Some(u) = appended {
            return u;
        }
    }
    if len > MAX_FILE {
        return Update::Failed(format!("{} is too large to open ({} MB).", path.display(), len >> 20));
    }
    match load_file(path, progress) {
        Ok(Some(t)) => Update::Replace(Box::new(t), info),
        Ok(None) => Update::Binary,
        Err(e) => Update::Failed(e),
    }
}

// ---------------------------------------------------------------------- web

/// True for the addresses Inkdown opens: `http://` and `https://`.
pub fn is_url(text: &str) -> bool {
    let t = text.trim();
    let lower = t.get(..8).unwrap_or(t).to_ascii_lowercase();
    (lower.starts_with("http://") || lower.starts_with("https://")) && t.len() > 10 && !t.contains(char::is_whitespace)
}

/// A file name for the document behind a web address.
pub fn name_of_url(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let path = rest.split(['?', '#']).next().unwrap_or(rest);
    let mut parts = path.split('/').filter(|p| !p.is_empty());
    let host = parts.next().unwrap_or("document");
    let last = parts.last().unwrap_or("");
    let clean: String = last.chars().filter(|c| !c.is_control() && !"<>:\"\\|?*".contains(*c)).collect();
    if clean.is_empty() {
        host.split(':').next().unwrap_or(host).to_string()
    } else {
        clean
    }
}

fn parse_range_start(header: &str) -> Option<u64> {
    // `bytes 100-199/200`
    header.trim().strip_prefix("bytes")?.trim().split('-').next()?.trim().parse().ok()
}

/// The start of an answer: status and headers. The body follows from the reader.
pub struct Head {
    pub status: u16,
    pub etag: Option<String>,
    pub modified: Option<String>,
    pub range_start: Option<u64>,
}

/// Sends the request and returns as soon as the headers are in; the body is read from
/// the returned reader as it arrives, which may be for as long as the server keeps sending.
#[cfg(not(target_os = "android"))]
pub fn http_open(url: &str, from: u64, etag: Option<&str>, modified: Option<&str>) -> Result<(Head, Box<dyn Read + Send>), String> {
    // no limit on the time the body may take: a stream never ends
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_connect(Some(std::time::Duration::from_secs(15))).timeout_recv_response(Some(std::time::Duration::from_secs(30))).http_status_as_error(false).build().into();
    let mut req = agent.get(url).header("User-Agent", "Inkdown").header("Accept", "text/*, application/json, */*;q=0.8");
    if from > 0 {
        req = req.header("Range", format!("bytes={from}-"));
        if let Some(v) = etag.or(modified) {
            req = req.header("If-Range", v);
        }
    } else {
        if let Some(e) = etag {
            req = req.header("If-None-Match", e);
        }
        if let Some(m) = modified {
            req = req.header("If-Modified-Since", m);
        }
    }
    let resp = req.call().map_err(|e| format!("Could not reach {} ({e}).", host_of(url)))?;
    let header = |name: &str| resp.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
    let head = Head { status: resp.status().as_u16(), etag: header("etag"), modified: header("last-modified"), range_start: header("content-range").as_deref().and_then(parse_range_start) };
    Ok((head, Box::new(resp.into_body().into_reader())))
}

/// Android's own HTTP client, through the activity.
#[cfg(target_os = "android")]
pub fn http_open(url: &str, from: u64, etag: Option<&str>, modified: Option<&str>) -> Result<(Head, Box<dyn Read + Send>), String> {
    let (packed, stream) = crate::mobile::http_open(url, from, etag.unwrap_or(""), modified.unwrap_or("")).ok_or_else(|| format!("Could not reach {}.", host_of(url)))?;
    // `status\netag\nlast-modified\ncontent-range`; status 0 carries the error text instead
    let mut lines = packed.split('\n');
    let status: u16 = lines.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    let mut field = || lines.next().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let (etag, modified, range) = (field(), field(), field());
    if status == 0 {
        return Err(format!("Could not reach {} ({}).", host_of(url), etag.unwrap_or_default()));
    }
    Ok((Head { status, etag, modified, range_start: range.as_deref().and_then(parse_range_start) }, Box::new(stream)))
}

fn host_of(url: &str) -> String {
    url.split("://").nth(1).unwrap_or(url).split(['/', '?', '#']).next().unwrap_or(url).to_string()
}

fn new_hasher() -> std::collections::hash_map::DefaultHasher {
    std::collections::hash_map::DefaultHasher::new()
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    use std::hash::Hasher;
    let mut h = new_hasher();
    h.write(bytes);
    h.finish() | 1
}

/// What is known about a web document from earlier looks at it.
#[derive(Clone, Default)]
pub struct Known {
    pub consumed: u64,
    pub sig: Vec<u8>,
    pub etag: Option<String>,
    pub modified: Option<String>,
    pub hash: u64,
}

/// How long an answer may pause before it is treated as a stream and shown as it comes.
const STREAM_AFTER: std::time::Duration = std::time::Duration::from_millis(700);

/// Reads the document at `url`, or what was added to it, and reports through `send`
/// (which returns false when nobody listens any more).
///
/// An answer that arrives in one go is a document: it replaces the text, or is appended
/// when it only grew. An answer that keeps coming (a log endpoint that never closes, a
/// slow download) is a stream: what has arrived is shown at once and every further piece
/// is appended as it arrives, for as long as the server keeps the connection open.
pub fn run_url(url: &str, known: &Known, send: &mut dyn FnMut(Update) -> bool) {
    let Known { consumed, sig, etag, modified, hash } = known.clone();
    let (etag, modified) = (etag.as_deref(), modified.as_deref());
    // only a strong validator makes a partial request safe
    let strong = etag.filter(|e| !e.starts_with("W/"));
    let from = if consumed > 0 && (strong.is_some() || modified.is_some()) { consumed } else { 0 };
    let asked = if from > 0 { http_open(url, from, strong, if strong.is_none() { modified } else { None }) } else { http_open(url, 0, etag, modified) };
    let (head, mut reader) = match asked {
        Ok(r) => r,
        Err(e) => {
            send(Update::Failed(e));
            return;
        }
    };
    let meta = |hash: u64| Meta { mtime: None, etag: head.etag.clone(), modified: head.modified.clone(), hash };
    match head.status {
        200 | 206 => {}
        304 | 416 => {
            send(Update::Unchanged(Meta { etag: etag.map(str::to_string), modified: modified.map(str::to_string), hash, ..Default::default() }));
            return;
        }
        401 | 403 => {
            send(Update::Failed(format!("{} does not allow reading this address ({}).", host_of(url), head.status)));
            return;
        }
        404 | 410 => {
            send(Update::Failed(format!("There is no document at this address ({}).", head.status)));
            return;
        }
        s => {
            send(Update::Failed(format!("{} answered with an error ({s}).", host_of(url))));
            return;
        }
    }
    let partial = head.status == 206 && from > 0 && head.range_start == Some(from);
    // an answer from the start, for a document we already hold part of: wait for as much
    // as we hold before deciding whether it is the same document, longer
    let again = !partial && consumed > 0;
    let started = std::time::Instant::now();
    let mut body: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    let mut ended = false;
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => {
                ended = true;
                break;
            }
            Ok(n) => body.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                if body.is_empty() {
                    send(Update::Failed(format!("The download from {} was interrupted ({e}).", host_of(url))));
                    return;
                }
                ended = true;
                break;
            }
        }
        if body.len() as u64 > MAX_REMOTE {
            send(Update::Failed(format!("The document at {} is too large to open.", host_of(url))));
            return;
        }
        let waited = started.elapsed();
        let enough = if again { body.len() as u64 > consumed || waited > STREAM_AFTER * 6 } else { true };
        if waited > STREAM_AFTER && enough {
            break;
        }
    }
    if ended {
        // the whole answer is here: a document
        send(finite(url, &body, partial, consumed, &sig, hash, &meta));
        return;
    }
    // a stream: show what is here and keep appending
    if !send(Update::Streaming(true)) {
        return;
    }
    use std::hash::Hasher;
    let mut hasher = new_hasher();
    let mut hashed = !partial;
    let mut total = if partial { consumed } else { 0 };
    let mut tail = if partial { sig.clone() } else { Vec::new() };
    // bytes not yet shown: the end of a character may still be on its way
    let mut carry: Vec<u8>;
    if partial {
        carry = body;
    } else if again && body.len() as u64 > consumed && body[..consumed as usize].ends_with(&sig) {
        // the same document from its start: skip what we hold
        hasher.write(&body[..consumed as usize]);
        total = consumed;
        tail = sig.clone();
        carry = body.split_off(consumed as usize);
    } else if again && (body.len() as u64) < consumed {
        // an endpoint that sends only what is new each time it is asked: continue
        hashed = false;
        total = consumed;
        tail = sig.clone();
        carry = body;
    } else {
        // new content: replace
        let used = match utf8_prefix(&body) {
            Some((_, _, used)) => {
                // keep a trailing `\r` out of the first part, like the incomplete character
                used
            }
            None => body.len(),
        };
        let Some(text) = text_from_bytes(&body[..used]) else {
            send(Update::Failed(format!("The document at {} is not text.", host_of(url))));
            return;
        };
        hasher.write(&body[..used]);
        total = used as u64;
        tail = sig_after(&[], &body[..used]);
        let mut t = text;
        t.consumed = total;
        t.sig = tail.clone();
        if !send(Update::Replace(Box::new(t), meta(0))) {
            return;
        }
        carry = body.split_off(used);
    }
    loop {
        if !carry.is_empty() {
            let (text, crlf, used) = match utf8_prefix(&carry) {
                Some(p) => p,
                // not UTF-8: show it as well as possible instead of stopping
                None => (String::from_utf8_lossy(&carry).replace("\r\n", "\n").replace('\r', "\n"), false, carry.len()),
            };
            if used > 0 {
                if hashed {
                    hasher.write(&carry[..used]);
                }
                total += used as u64;
                tail = sig_after(&tail, &carry[..used]);
                carry.drain(..used);
                if !send(Update::Append { text, crlf, consumed: total, sig: tail.clone(), meta: meta(0) }) {
                    return;
                }
            }
        }
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => carry.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
        if total > MAX_REMOTE {
            send(Update::Failed(format!("The stream from {} grew too large to keep open.", host_of(url))));
            break;
        }
    }
    // the server closed the connection: from now on, look again from time to time
    send(Update::Streaming(false));
    send(Update::Unchanged(meta(if hashed { hasher.finish() | 1 } else { 0 })));
}

/// A complete answer: unchanged, grown, or different.
fn finite(url: &str, body: &[u8], partial: bool, consumed: u64, sig: &[u8], hash: u64, meta: &dyn Fn(u64) -> Meta) -> Update {
    if partial {
        return match utf8_prefix(body) {
            Some((text, crlf, used)) if used > 0 => Update::Append { text, crlf, consumed: consumed + used as u64, sig: sig_after(sig, &body[..used]), meta: meta(0) },
            Some(_) => Update::Unchanged(meta(hash)),
            // the new part is not text that can be appended: ask for everything next time
            None => Update::Unchanged(Meta { hash, ..Default::default() }),
        };
    }
    let h = hash_bytes(body);
    if h == hash {
        return Update::Unchanged(meta(h));
    }
    // a server without partial answers that only added to the end: append, to keep the place
    if consumed > 0 && body.len() as u64 > consumed && body[..consumed as usize].ends_with(sig) {
        if let Some((text, crlf, used)) = utf8_prefix(&body[consumed as usize..]).filter(|p| p.2 > 0) {
            let end = consumed as usize + used;
            return Update::Append { text, crlf, consumed: end as u64, sig: sig_after(sig, &body[consumed as usize..end]), meta: meta(if end == body.len() { h } else { 0 }) };
        }
    }
    match text_from_bytes(body) {
        Some(t) => Update::Replace(Box::new(t), meta(h)),
        None => Update::Failed(format!("The document at {} is not text.", host_of(url))),
    }
}

// ------------------------------------------------------------- in the app

/// Runs `work` on a thread of its own. It reports any number of updates through the
/// function it is given; the channel closing tells the app that the work has ended.
fn spawn(name: &str, wake: impl Fn() + Send + 'static, work: impl FnOnce(&mut dyn FnMut(Update) -> bool) + Send + 'static) -> Option<Receiver<Update>> {
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let tx2 = tx.clone();
            let wake2 = &wake;
            let mut send = move |u: Update| {
                let ok = tx2.send(u).is_ok();
                wake2();
                ok
            };
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&mut send))).is_err() {
                let _ = tx.send(Update::Failed("Reading failed.".into()));
            }
            drop(send);
            drop(tx);
            wake();
        })
        .ok()?;
    Some(rx)
}

/// True when opening `path` should happen on a worker thread.
pub fn opens_in_background(path: &Path, len: u64) -> bool {
    (BACKGROUND_FROM..=MAX_FILE).contains(&len) && text_candidate(path)
}

/// False for files that have their own readers (pictures, PDF, Avro).
pub fn text_candidate(path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    if crate::doc::Kind::of(&name, "").viewed_only() {
        return false;
    }
    // Avro and PDF files are recognised by their first bytes and have their own readers
    let mut head = [0u8; 8];
    let n = std::fs::File::open(path).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
    !(head[..n].starts_with(b"Obj\x01") || head[..n].starts_with(b"%PDF-"))
}

/// An empty, read-only tab for `path` whose text arrives later.
pub fn placeholder(path: &Path, len: u64, set: &Settings, wake: impl Fn() + Send + 'static) -> Doc {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mut d = Doc::new(Some(path.to_path_buf()), name, "", set);
    d.mode = crate::doc::Mode::Code;
    d.read_only = true;
    d.live.loading = true;
    d.live.total = len;
    let progress = d.live.progress.clone();
    let p = path.to_path_buf();
    d.live.job = spawn("ink-load", wake, move |send| {
        send(refresh_file(&p, 0, &[], false, &progress));
    });
    d
}

/// A tab for the document at `url`; its text arrives later.
pub fn remote_doc(url: &str, set: &Settings, wake: impl Fn() + Send + 'static) -> Doc {
    let mut d = Doc::new(None, name_of_url(url), "", set);
    d.read_only = true;
    d.live.url = Some(url.to_string());
    d.live.loading = true;
    start(&mut d, wake);
    d
}

/// Starts looking for new content, unless a look is already under way.
pub fn start(d: &mut Doc, wake: impl Fn() + Send + 'static) {
    if d.live.job.is_some() {
        return;
    }
    let known = Known { consumed: d.live.consumed, sig: d.live.sig.clone(), etag: d.live.etag.clone(), modified: d.live.modified.clone(), hash: d.live.hash };
    if let Some(url) = d.live.url.clone() {
        d.live.job = spawn("ink-fetch", wake, move |send| run_url(&url, &known, send));
    } else if let Some(path) = d.path.clone() {
        let utf8 = d.encoding == encoding_rs::UTF_8 && !d.buf.dirty();
        let progress = d.live.progress.clone();
        d.live.job = spawn("ink-tail", wake, move |send| {
            send(refresh_file(&path, known.consumed, &known.sig, utf8, &progress));
        });
    }
}

/// Puts the result of a look into the document. Returns a message for the user, if any.
pub fn apply(d: &mut Doc, set: &Settings, update: Update, now: f64) -> Option<String> {
    if let Update::Streaming(on) = update {
        d.live.streaming = on;
        return None;
    }
    let first = d.live.loading;
    d.live.loading = false;
    d.live.checked_at = now;
    let caret = d.buf.primary().head;
    let at_end = d.buf.line_of(caret) + 1 >= d.buf.lines();
    fn meta_in(d: &mut Doc, m: Meta) {
        if m.mtime.is_some() {
            d.mtime = m.mtime;
        }
        d.live.etag = m.etag;
        d.live.modified = m.modified;
        d.live.hash = m.hash;
    }
    match update {
        Update::Unchanged(m) => {
            d.live.error = None;
            meta_in(d, m);
            None
        }
        Update::Append { text, crlf, consumed, sig, meta } => {
            d.live.error = None;
            d.buf.append_external(&text);
            d.crlf |= crlf;
            d.live.consumed = consumed;
            d.live.sig = sig;
            d.live.updates += 1;
            meta_in(d, meta);
            if d.live.follow && at_end {
                d.buf.set_caret(d.buf.len());
                d.ed.reveal = true;
            }
            None
        }
        Update::Replace(text, meta) => {
            d.live.error = None;
            let t = *text;
            d.encoding = t.encoding;
            d.bom = t.bom;
            d.crlf = t.crlf;
            d.live.consumed = t.consumed;
            d.live.sig = t.sig;
            d.live.updates += 1;
            meta_in(d, meta);
            // a small document keeps its undo history; a large one gets a fresh buffer
            if !first && t.rope.len_bytes() < 2 << 20 {
                d.buf.set_text(&t.rope.to_string());
                d.buf.mark_saved();
            } else {
                d.buf = Buffer::from_rope(t.rope);
                d.hl = crate::syntax::Highlighter::new(d.hl.syntax);
                d.ed.folds.clear();
            }
            if first {
                if d.live.url.is_none() {
                    d.read_only = false;
                }
                d.redetect(set);
                if d.live.url.is_some() && d.hl.name().eq_ignore_ascii_case("log") {
                    // a log on a server is read from its end, as it grows
                    d.live.follow = true;
                }
                d.buf.set_caret(if d.live.follow { d.buf.len() } else { 0 });
                d.ed.reveal = true;
            } else if d.live.follow && at_end {
                d.buf.set_caret(d.buf.len());
                d.ed.reveal = true;
            } else {
                d.buf.set_caret(caret.min(d.buf.len()));
            }
            d.edited_at = None;
            None
        }
        Update::Streaming(_) => None,
        Update::Binary => {
            // not text: open it the ordinary way, which shows a hex listing
            let path = d.path.clone()?;
            match Doc::open(&path, set) {
                Ok(mut fresh) => {
                    fresh.id = d.id;
                    *d = fresh;
                    None
                }
                Err(e) => Some(e),
            }
        }
        Update::Failed(e) => {
            let repeat = d.live.error.as_deref() == Some(e.as_str());
            d.live.error = Some(e.clone());
            // say it once, not at every refresh
            (!repeat).then_some(e)
        }
    }
}

/// Once per frame: collects finished work and starts the next look where one is due.
pub fn tick(app: &mut InkApp, ctx: &egui::Context, now: f64) {
    let set = app.set.clone();
    let every = set.live_refresh_seconds.clamp(1.0, 3600.0);
    let mut messages = Vec::new();
    let mut soonest: Option<f64> = None;
    let mut loaded = false;
    for d in app.docs.iter_mut() {
        if d.live.job.is_some() {
            // everything that arrived since the last frame; the work ends when its channel closes
            let mut ended = false;
            let mut budget = 400;
            loop {
                let got = d.live.job.as_ref().map(|rx| rx.try_recv());
                match got {
                    Some(Ok(update)) => {
                        loaded |= d.live.loading;
                        if let Some(m) = apply(d, &set, update, now) {
                            messages.push(m);
                        }
                        budget -= 1;
                        if budget == 0 {
                            // a fast stream: the rest next frame
                            soonest = Some(0.0);
                            break;
                        }
                    }
                    Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) | None => {
                        ended = true;
                        break;
                    }
                    Some(Err(std::sync::mpsc::TryRecvError::Empty)) => break,
                }
            }
            if ended {
                d.live.job = None;
                d.live.loading = false;
                d.live.streaming = false;
                d.live.next_check = now + if d.live.remote() { every } else { 1.0 };
            } else {
                if d.live.loading {
                    // keep the progress display moving
                    soonest = Some(soonest.map_or(0.1, |s: f64| s.min(0.1)));
                }
                continue;
            }
        }
        let live = d.live.remote() && set.live_refresh || d.live.follow;
        if !live {
            continue;
        }
        if now >= d.live.next_check {
            let due = if d.live.remote() { true } else { d.changed_on_disk() || d.path.as_ref().and_then(|p| std::fs::metadata(p).ok()).is_some_and(|m| m.len() != d.live.consumed && d.live.consumed > 0) };
            d.live.next_check = now + if d.live.remote() { every } else { 1.0 };
            if due {
                let c = ctx.clone();
                start(d, move || c.request_repaint());
            }
        }
        let wait = (d.live.next_check - now).max(0.05);
        soonest = Some(soonest.map_or(wait, |s: f64| s.min(wait)));
    }
    if loaded {
        ctx.request_repaint();
    }
    for m in messages {
        app.toast(m);
    }
    if let Some(s) = soonest {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(s));
    }
}

pub fn command(app: &mut InkApp, ctx: &egui::Context, name: &str, arg: &str) {
    match name {
        "open_url" => {
            if is_url(arg) {
                app.open_url(ctx, arg.trim());
            } else {
                app.dialog = Some(crate::app::Dialog::OpenUrl(String::new()));
            }
        }
        "toggle_follow" => {
            let Some(d) = app.docs.get_mut(app.active) else { return };
            if d.path.is_none() && d.live.url.is_none() {
                app.toast("Follow works for files and web addresses");
                return;
            }
            d.live.follow = !d.live.follow;
            if d.live.follow {
                d.live.next_check = 0.0;
                d.buf.set_caret(d.buf.len());
                d.ed.reveal = true;
            }
            let on = d.live.follow;
            app.toast(if on { "Following: new lines appear as they are written" } else { "Follow is off" });
        }
        "reload_now" => {
            let Some(d) = app.docs.get_mut(app.active) else { return };
            d.live.next_check = 0.0;
            let c = ctx.clone();
            start(d, move || c.request_repaint());
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ink-live-{}-{name}", std::process::id()))
    }

    #[test]
    fn text_is_decoded_in_pieces_like_in_one_go() {
        // line ends and multi-byte characters across the borders between pieces
        let mut src = String::new();
        while src.len() < CHUNK * 2 + 1000 {
            src.push_str("grüße 😀 line\r\nnext\rlast\n");
        }
        for cut in [0usize, 1, 2, 3] {
            let bytes = &src.as_bytes()[..src.len() - cut * 7];
            let Ok(expect) = std::str::from_utf8(bytes) else { continue };
            let t = text_from_bytes(bytes).unwrap();
            assert_eq!(t.encoding, encoding_rs::UTF_8);
            assert!(t.crlf);
            assert_eq!(t.consumed, bytes.len() as u64);
            assert_eq!(t.sig, &bytes[bytes.len() - SIG..]);
            assert!(t.rope.to_string() == expect.replace("\r\n", "\n").replace('\r', "\n"));
        }
        // a byte order mark, UTF-16, Western European and binary
        let t = text_from_bytes(b"\xEF\xBB\xBFhello\r\n").unwrap();
        assert!(t.bom && t.crlf && t.rope.to_string() == "hello\n");
        let utf16: Vec<u8> = "wide text here\n".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(text_from_bytes(&utf16).unwrap().rope.to_string(), "wide text here\n");
        let t = text_from_bytes(b"caf\xe9 au lait").unwrap();
        assert_eq!((t.encoding, t.rope.to_string().as_str()), (encoding_rs::WINDOWS_1252, "café au lait"));
        assert!(text_from_bytes(b"\x00\x01\x02binary\x00\x00\x03").is_none());
        assert_eq!(text_from_bytes(b"").unwrap().rope.len_chars(), 0);
    }

    #[test]
    fn a_growing_file_is_appended_and_a_rewritten_one_reloaded() {
        let p = temp("grow.log");
        let progress = AtomicU64::new(0);
        std::fs::write(&p, "one\ntwo\n").unwrap();
        let Update::Replace(t, _) = refresh_file(&p, 0, &[], false, &progress) else { panic!("first load") };
        assert_eq!((t.rope.to_string().as_str(), t.consumed), ("one\ntwo\n", 8));
        // grown, ending in half a character and half a line end
        let mut more = b"one\ntwo\nthree \xe2\x82".to_vec();
        std::fs::write(&p, &more).unwrap();
        let Update::Append { text, consumed, sig, .. } = refresh_file(&p, t.consumed, &t.sig, true, &progress) else { panic!("append") };
        assert_eq!((text.as_str(), consumed), ("three ", 14));
        more.extend_from_slice(b"\xac\r");
        std::fs::write(&p, &more).unwrap();
        let Update::Append { text, consumed, sig, .. } = refresh_file(&p, consumed, &sig, true, &progress) else { panic!("append 2") };
        assert_eq!((text.as_str(), consumed), ("€", 17));
        more.extend_from_slice(b"\nfour\r\n");
        std::fs::write(&p, &more).unwrap();
        let Update::Append { text, crlf, consumed, sig, .. } = refresh_file(&p, consumed, &sig, true, &progress) else { panic!("append 3") };
        assert_eq!((text.as_str(), crlf, consumed as usize), ("\nfour\n", true, more.len()));
        // nothing new
        assert!(matches!(refresh_file(&p, consumed, &sig, true, &progress), Update::Replace(..)), "same size: read again to be sure");
        // rewritten with other content, longer than before
        std::fs::write(&p, "completely different content, and longer than it was\n").unwrap();
        let Update::Replace(t, _) = refresh_file(&p, consumed, &sig, true, &progress) else { panic!("reload") };
        assert!(t.rope.to_string().starts_with("completely"));
        // truncated (log rotation)
        std::fs::write(&p, "new\n").unwrap();
        assert!(matches!(refresh_file(&p, t.consumed, &t.sig, true, &progress), Update::Replace(..)));
        let _ = std::fs::remove_file(&p);
        assert!(matches!(refresh_file(&p, 0, &[], true, &progress), Update::Failed(_)));
    }

    #[test]
    fn web_addresses() {
        assert!(is_url("https://example.com/a.log") && is_url("HTTP://host:8080/x") && !is_url("ftp://x/y.txt") && !is_url("C:/a/b.txt") && !is_url("https://a b") && !is_url("file:///etc/passwd"));
        assert_eq!(name_of_url("https://example.com/logs/app.log?tail=1#x"), "app.log");
        assert_eq!(name_of_url("http://localhost:8080/"), "localhost");
        assert_eq!(name_of_url("https://example.com/docs/README.md"), "README.md");
        assert_eq!(parse_range_start("bytes 100-199/200"), Some(100));
        // complete answers: unchanged, grown, different
        let meta = |h: u64| Meta { hash: h, ..Default::default() };
        let Update::Replace(t, m) = finite("http://h/x", b"one\ntwo\n", false, 0, &[], 0, &meta) else { panic!("first") };
        assert_eq!((t.rope.to_string().as_str(), t.consumed), ("one\ntwo\n", 8));
        assert!(matches!(finite("http://h/x", b"one\ntwo\n", false, 8, &t.sig, m.hash, &meta), Update::Unchanged(_)));
        let Update::Append { text, consumed, .. } = finite("http://h/x", b"one\ntwo\nthree\n", false, 8, &t.sig, m.hash, &meta) else { panic!("grown") };
        assert_eq!((text.as_str(), consumed), ("three\n", 14));
        assert!(matches!(finite("http://h/x", b"ONE\nTWO\nthree\n", false, 8, &t.sig, m.hash, &meta), Update::Replace(..)));
        let Update::Append { text, consumed, .. } = finite("http://h/x", b"three\n", true, 8, &t.sig, m.hash, &meta) else { panic!("partial") };
        assert_eq!((text.as_str(), consumed), ("three\n", 14));
    }
}
