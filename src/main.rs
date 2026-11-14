#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod data;
mod export;
mod google;
#[cfg(windows)]
mod install;
#[cfg(not(windows))]
#[path = "install_other.rs"]
mod install;
mod render;

use std::borrow::Cow;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use base64::Engine;
use notify::{RecursiveMode, Watcher};
use rust_embed::RustEmbed;
use serde_json::{json, Value};
use tao::dpi::{LogicalSize, PhysicalSize};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::{Icon, Theme, Window, WindowBuilder};
use wry::http::{header, Request, Response, StatusCode};
use wry::{DragDropEvent, NewWindowResponse, WebContext, WebView, WebViewBuilder};
#[cfg(windows)]
use wry::WebViewBuilderExtWindows;

#[derive(RustEmbed)]
#[folder = "assets/"]
struct Assets;

// wry maps custom protocols to http://<name>.localhost on Windows and <name>://localhost elsewhere.
#[cfg(windows)]
const APP_ORIGIN: &str = "http://ink.localhost/";
#[cfg(not(windows))]
const APP_ORIGIN: &str = "ink://localhost/";
const MD_EXTS: &[&str] = &["md", "markdown", "mdown", "mkd", "mkdn", "mdwn", "mdx", "txt"];
const DOC_EXTS: &[&str] = &["adoc", "asciidoc", "asc", "rst", "org", "tex", "html", "htm", "txt", "log"];
const DATA_EXTS: &[&str] = &["json", "jsonc", "json5", "geojson", "yaml", "yml", "toml", "xml", "csv", "tsv", "ini", "cfg", "conf", "env", "properties", "lock"];
const DIAGRAM_EXTS: &[&str] = &["mmd", "mermaid", "dot", "gv", "svg", "puml", "plantuml"];
/// Source files shown in the folder panel (anything else can still be opened).
const CODE_EXTS: &[&str] = &[
    "rs", "py", "js", "mjs", "cjs", "ts", "tsx", "jsx", "c", "h", "cpp", "cc", "cxx", "hpp", "cs", "java", "kt", "kts", "go", "rb", "php",
    "swift", "scala", "sh", "bash", "zsh", "fish", "ps1", "psm1", "bat", "cmd", "sql", "r", "lua", "pl", "pm", "dart", "ex", "exs", "erl",
    "hs", "ml", "fs", "fsx", "clj", "cljs", "lisp", "scm", "jl", "m", "mm", "vb", "pas", "groovy", "gradle", "zig", "nim", "v", "sv", "vhd",
    "asm", "s", "css", "scss", "sass", "less", "vue", "svelte", "astro", "graphql", "gql", "proto", "tf", "hcl", "nix", "cmake", "mk",
    "dockerfile", "diff", "patch", "glsl", "wgsl", "sol", "elm", "cr", "d", "f90", "cob", "tcl", "awk", "sed", "vim", "el", "gd", "bib",
];
const TEXT_NAMES: &[&str] = &["makefile", "dockerfile", "license", "readme", "changelog", "authors", "notice", "cmakelists.txt", "gemfile", "rakefile", "jenkinsfile", "vagrantfile", "procfile"];

/// Formats that are certainly not text; everything else is offered in the folder panel.
const BINARY_EXTS: &[&str] = &[
    "exe", "dll", "so", "dylib", "bin", "obj", "o", "a", "lib", "pdb", "class", "jar", "war", "zip", "7z", "rar", "gz", "tgz", "tar", "xz",
    "bz2", "zst", "iso", "msi", "msix", "appx", "cab", "deb", "rpm", "apk", "ipa", "dmg", "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico",
    "avif", "tif", "tiff", "psd", "heic", "mp3", "mp4", "mkv", "mov", "avi", "wav", "flac", "ogg", "webm", "m4a", "pdf", "doc", "docx",
    "xls", "xlsx", "ppt", "pptx", "odt", "ods", "ttf", "otf", "woff", "woff2", "eot", "db", "sqlite", "sqlite3", "mdb", "dat", "pak",
    "wasm", "pyc", "pyd", "rlib", "rmeta", "node", "ilk", "exp", "res", "lnk", "sys", "tmp",
];

fn is_listed_file(p: &Path) -> bool {
    let _ = (DOC_EXTS, DATA_EXTS, DIAGRAM_EXTS, CODE_EXTS, TEXT_NAMES); // used for the Open dialog groups
    !BINARY_EXTS.contains(&ext_of(p).as_str())
}
/// Local files the preview may load through the `/__file/` route.
const MEDIA_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico", "avif", "apng", "tif", "tiff",
    "mp4", "webm", "ogv", "mov", "mp3", "wav", "ogg", "m4a", "flac",
];
/// Never handed to the shell from a link click.
const BLOCKED_OPEN_EXTS: &[&str] = &[
    "exe", "com", "bat", "cmd", "scr", "pif", "msi", "msp", "ps1", "psm1", "vbs", "vbe", "js",
    "jse", "wsf", "wsh", "hta", "lnk", "url", "reg", "cpl", "jar", "dll", "appref-ms", "scf",
];

#[derive(Debug)]
enum UserEvent {
    Ipc(String),
    FileChanged(PathBuf),
    Dropped(Vec<PathBuf>),
    Reply(u64, Result<Value, String>),
}

fn ext_of(p: &Path) -> String {
    p.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn norm(p: &Path) -> String {
    p.to_string_lossy().replace('/', "\\").to_lowercase()
}

// ---------------------------------------------------------------- protocol

fn respond(status: StatusCode, mime: &str, body: Cow<'static, [u8]>) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, "no-store")
        .header("X-Content-Type-Options", "nosniff")
        .body(body)
        .unwrap()
}

fn serve(req: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let path = req.uri().path();
    if let Some(rest) = path.strip_prefix("/__file/") {
        let decoded = percent_encoding::percent_decode_str(rest).decode_utf8_lossy();
        let p = PathBuf::from(decoded.as_ref());
        if !MEDIA_EXTS.contains(&ext_of(&p).as_str()) {
            return respond(StatusCode::FORBIDDEN, "text/plain", Cow::Borrowed(b"forbidden"));
        }
        return match std::fs::read(&p) {
            Ok(bytes) => {
                let mime = mime_guess::from_path(&p).first_or_octet_stream();
                respond(StatusCode::OK, mime.as_ref(), Cow::Owned(bytes))
            }
            Err(_) => respond(StatusCode::NOT_FOUND, "text/plain", Cow::Borrowed(b"not found")),
        };
    }
    let rel = path.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    match Assets::get(rel) {
        Some(f) => {
            let mime = mime_guess::from_path(rel).first_or_octet_stream();
            let mime = if mime.type_() == "text" || mime.subtype() == "javascript" {
                format!("{mime}; charset=utf-8")
            } else {
                mime.to_string()
            };
            respond(StatusCode::OK, &mime, f.data)
        }
        None => respond(StatusCode::NOT_FOUND, "text/plain", Cow::Borrowed(b"not found")),
    }
}

// ------------------------------------------------------------------- files

fn decode_text(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        let le = bytes[0] == 0xFF;
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) })
            .collect();
        return String::from_utf16_lossy(&units);
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

fn read_doc(path: &Path) -> Result<Value, String> {
    if std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) > 64 * 1024 * 1024 {
        return Err(format!("{} is larger than 64 MB and was not opened.", path.display()));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
    let utf16 = bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]);
    if !utf16 && bytes.iter().take(8192).any(|b| *b == 0) {
        return Err(format!("{} is a binary file, not text.", path.display()));
    }
    let text = decode_text(&bytes);
    let crlf = text.contains("\r\n");
    let text = if crlf { text.replace("\r\n", "\n") } else { text };
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let readonly = std::fs::metadata(&abs).map(|m| m.permissions().readonly()).unwrap_or(false);
    Ok(json!({
        "path": abs.to_string_lossy(),
        "name": abs.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        "dir": abs.parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default(),
        "text": text,
        "crlf": crlf,
        "readonly": readonly,
    }))
}

fn write_text(path: &Path, text: &str, crlf: bool) -> Result<(), String> {
    let out = if crlf { text.replace('\n', "\r\n") } else { text.to_string() };
    std::fs::write(path, out).map_err(|e| format!("Cannot save {}: {e}", path.display()))
}

fn list_dir(dir: &Path) -> Result<Value, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('$') || matches!(name.as_str(), ".git" | ".svn" | ".hg" | ".DS_Store" | "Thumbs.db" | "desktop.ini") {
            continue;
        }
        let p = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            if matches!(name.as_str(), "node_modules" | "target" | "System Volume Information") {
                continue;
            }
            dirs.push(json!({ "name": name, "path": p.to_string_lossy(), "dir": true }));
        } else if is_listed_file(&p) {
            files.push(json!({ "name": name, "path": p.to_string_lossy(), "dir": false }));
        }
    }
    let key = |v: &Value| v["name"].as_str().unwrap_or("").to_lowercase();
    dirs.sort_by_key(key);
    files.sort_by_key(key);
    dirs.extend(files);
    Ok(json!({
        "path": dir.to_string_lossy(),
        "name": dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| dir.to_string_lossy().to_string()),
        "parent": dir.parent().map(|d| d.to_string_lossy().to_string()),
        "entries": dirs,
    }))
}

#[cfg(not(windows))]
pub(crate) fn shell_open(target: &str) {
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let _ = std::process::Command::new(opener).arg(target).spawn();
}

#[cfg(windows)]
pub(crate) fn shell_open(target: &str) {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let w: Vec<u16> = std::ffi::OsStr::new(target).encode_wide().chain(Some(0)).collect();
    let verb: Vec<u16> = "open\0".encode_utf16().collect();
    unsafe {
        ShellExecuteW(None, PCWSTR(verb.as_ptr()), PCWSTR(w.as_ptr()), None, None, SW_SHOWNORMAL);
    }
}

fn open_external(target: &str) -> Result<(), String> {
    let lower = target.to_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:") {
        shell_open(target);
        return Ok(());
    }
    let p = Path::new(target);
    if !p.is_absolute() || !p.exists() {
        return Err(format!("Not found: {target}"));
    }
    if BLOCKED_OPEN_EXTS.contains(&ext_of(p).as_str()) {
        return Err("Inkdown does not launch programs or scripts from links.".into());
    }
    shell_open(target);
    Ok(())
}

// ------------------------------------------------------------------ export

fn asset_map(v: &Value) -> std::collections::HashMap<String, std::rc::Rc<export::Img>> {
    let mut map = std::collections::HashMap::new();
    for item in v.as_array().map(Vec::as_slice).unwrap_or(&[]) {
        let (Some(key), Some(data)) = (item["key"].as_str(), item["data"].as_str()) else { continue };
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) else { continue };
        map.insert(
            key.to_string(),
            std::rc::Rc::new(export::Img {
                jpeg: item["mime"].as_str() == Some("image/jpeg"),
                data: bytes,
                w: item["w"].as_f64().unwrap_or(0.0) as f32,
                h: item["h"].as_f64().unwrap_or(0.0) as f32,
                depth: item["depth"].as_f64().unwrap_or(0.0) as f32,
            }),
        );
    }
    map
}

/// Word document bytes for the Markdown and rendered graphics in an IPC message.
fn docx_bytes(a: &Value) -> Result<Vec<u8>, String> {
    let assets = export::Assets {
        diagrams: asset_map(&a["assets"]["diagrams"]),
        math: asset_map(&a["assets"]["math"]),
        images: asset_map(&a["assets"]["images"]),
    };
    export::write_docx(&export::build(a["text"].as_str().unwrap_or(""), &assets, a["title"].as_str().unwrap_or("Document")))
}

fn export_document(a: &Value) -> Result<(), String> {
    let assets = export::Assets {
        diagrams: asset_map(&a["assets"]["diagrams"]),
        math: asset_map(&a["assets"]["math"]),
        images: asset_map(&a["assets"]["images"]),
    };
    let doc = export::build(a["text"].as_str().unwrap_or(""), &assets, a["title"].as_str().unwrap_or("Document"));
    let bytes = match a["format"].as_str() {
        Some("docx") => export::write_docx(&doc)?,
        Some("rtf") => export::write_rtf(&doc),
        other => return Err(format!("unknown export format: {other:?}")),
    };
    let path = a["path"].as_str().unwrap_or("");
    std::fs::write(path, bytes).map_err(|e| format!("Cannot write {path}: {e}"))
}

// ---------------------------------------------------------------- settings

fn settings_path() -> PathBuf {
    install::config_dir().join("settings.json")
}
fn window_state_path() -> PathBuf {
    install::config_dir().join("window.json")
}
fn read_json(p: &Path) -> Value {
    std::fs::read_to_string(p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null)
}
fn write_json(p: &Path, v: &Value) {
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(p, serde_json::to_vec_pretty(v).unwrap_or_default());
}

// ----------------------------------------------------------------- watcher

struct FileWatch {
    watcher: Option<notify::RecommendedWatcher>,
    files: Arc<Mutex<HashSet<String>>>,
    dirs: HashSet<PathBuf>,
}

impl FileWatch {
    fn new(proxy: EventLoopProxy<UserEvent>) -> Self {
        let files: Arc<Mutex<HashSet<String>>> = Arc::default();
        let f2 = files.clone();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(ev) = res else { return };
            if ev.kind.is_access() {
                return;
            }
            let set = f2.lock().unwrap();
            for p in ev.paths {
                if set.contains(&norm(&p)) {
                    let _ = proxy.send_event(UserEvent::FileChanged(p));
                }
            }
        })
        .ok();
        Self { watcher, files, dirs: HashSet::new() }
    }

    fn set(&mut self, paths: Vec<PathBuf>) {
        let Some(w) = self.watcher.as_mut() else { return };
        let wanted: HashSet<PathBuf> =
            paths.iter().filter_map(|p| p.parent().map(Path::to_path_buf)).collect();
        for d in self.dirs.difference(&wanted) {
            let _ = w.unwatch(d);
        }
        for d in wanted.difference(&self.dirs) {
            let _ = w.watch(d, RecursiveMode::NonRecursive);
        }
        self.dirs = wanted;
        *self.files.lock().unwrap() = paths.iter().map(|p| norm(p)).collect();
    }
}

// --------------------------------------------------------------------- app

struct App {
    window: Window,
    webview: WebView,
    watch: FileWatch,
    proxy: EventLoopProxy<UserEvent>,
    initial_files: Vec<PathBuf>,
    ready: bool,
}

impl App {
    fn eval(&self, js: &str) {
        let _ = self.webview.evaluate_script(js);
    }

    fn reply(&self, id: u64, result: Result<Value, String>) {
        let (ok, payload) = match result {
            Ok(v) => (true, v),
            Err(e) => (false, Value::String(e)),
        };
        self.eval(&format!("window.__inkReply({id},{ok},{payload})"));
    }

    fn file_filters(d: rfd::FileDialog) -> rfd::FileDialog {
        let all: Vec<&str> = MD_EXTS.iter().chain(DOC_EXTS).chain(DATA_EXTS).chain(DIAGRAM_EXTS).chain(CODE_EXTS).copied().collect();
        d.add_filter("All files", &["*"])
            .add_filter("Common text and code files", &all)
            .add_filter("Markdown", &MD_EXTS[..7])
            .add_filter("Documents (AsciiDoc, HTML, text)", DOC_EXTS)
            .add_filter("Data (JSON, YAML, TOML, CSV, XML)", DATA_EXTS)
            .add_filter("Diagrams (Mermaid, Graphviz, SVG)", DIAGRAM_EXTS)
    }

    /// Returns `false` when the app should exit.
    fn handle_ipc(&mut self, raw: &str) -> bool {
        let Ok(msg) = serde_json::from_str::<Value>(raw) else { return true };
        let id = msg["id"].as_u64().unwrap_or(0);
        let cmd = msg["cmd"].as_str().unwrap_or("");
        let a = &msg["args"];
        let s = |k: &str| a[k].as_str().unwrap_or("").to_string();

        // Network work runs off the UI thread and answers through the event loop.
        if matches!(cmd, "google_connect" | "google_upload" | "google_disconnect") {
            let (proxy, cmd, a) = (self.proxy.clone(), cmd.to_string(), a.clone());
            std::thread::spawn(move || {
                let result = match cmd.as_str() {
                    "google_connect" => google::connect(a["clientId"].as_str().unwrap_or(""), a["clientSecret"].as_str().unwrap_or("")),
                    "google_disconnect" => google::disconnect().map(|_| google::status()),
                    _ => docx_bytes(&a).and_then(|bytes| google::upload_docx(&bytes, a["title"].as_str().unwrap_or("Document"))),
                };
                let _ = proxy.send_event(UserEvent::Reply(id, result));
            });
            return true;
        }
        let result: Result<Value, String> = match cmd {
            "google_status" => Ok(google::status()),
            "ready" => {
                self.ready = true;
                let docs: Vec<Value> = self
                    .initial_files
                    .drain(..)
                    .map(|p| read_doc(&p).unwrap_or_else(|e| json!({ "error": e })))
                    .collect();
                Ok(json!({
                    "settings": read_json(&settings_path()),
                    "docs": docs,
                    "version": env!("CARGO_PKG_VERSION"),
                    "sep": std::path::MAIN_SEPARATOR.to_string(),
                    "packaged": install::is_store_package(),
                    "installed": install::is_installed_copy(),
                    "isDefault": install::is_default_for_md(),
                }))
            }
            "show" => {
                self.window.set_visible(true);
                self.window.set_focus();
                let _ = self.webview.focus();
                Ok(Value::Null)
            }
            "render" => {
                let r = render::render(a["text"].as_str().unwrap_or(""));
                Ok(json!({ "html": r.html, "frontMatter": r.front_matter }))
            }
            "render_rich" => {
                let r = render::render_rich(a["text"].as_str().unwrap_or(""));
                Ok(json!({ "html": r.html, "frontMatter": r.front_matter }))
            }
            "export" => export_document(a).map(|_| Value::Null),
            "data_parse" => Ok(match data::parse(&s("format"), a["text"].as_str().unwrap_or("")) {
                Ok(v) => json!({ "value": v }),
                Err(e) => json!({ "error": e.message, "line": e.line }),
            }),
            "data_convert" => data::parse(&s("from"), a["text"].as_str().unwrap_or(""))
                .map_err(|e| e.message)
                .and_then(|v| data::emit(&s("to"), &v, a["compact"].as_bool().unwrap_or(false)))
                .map(Value::String),
            "open_dialog" => {
                let mut d = Self::file_filters(rfd::FileDialog::new()).set_parent(&self.window);
                let dir = s("dir");
                if !dir.is_empty() {
                    d = d.set_directory(dir);
                }
                let docs: Vec<Value> = d
                    .pick_files()
                    .unwrap_or_default()
                    .iter()
                    .map(|p| read_doc(p).unwrap_or_else(|e| json!({ "error": e })))
                    .collect();
                Ok(Value::Array(docs))
            }
            "open_folder_dialog" => Ok(rfd::FileDialog::new()
                .set_parent(&self.window)
                .pick_folder()
                .map(|p| Value::String(p.to_string_lossy().to_string()))
                .unwrap_or(Value::Null)),
            "read" => read_doc(Path::new(&s("path"))),
            "exists" => Ok(Value::Bool(Path::new(&s("path")).is_file())),
            "save" => write_text(Path::new(&s("path")), &s("text"), a["crlf"].as_bool().unwrap_or(false))
                .map(|_| Value::Null),
            "save_dialog" => {
                let mut d = rfd::FileDialog::new().set_parent(&self.window).set_file_name(s("name"));
                d = match s("kind").as_str() {
                    "html" => d.add_filter("HTML", &["html"]),
                    "docx" => d.add_filter("Word document", &["docx"]),
                    "rtf" => d.add_filter("Rich Text Format", &["rtf"]),
                    "file" => {
                        let ext = s("ext");
                        if ext.is_empty() { d.add_filter("All files", &["*"]) } else { d.add_filter(ext.to_uppercase(), &[ext.as_str()]).add_filter("All files", &["*"]) }
                    }
                    "svg" => d.add_filter("SVG image", &["svg"]),
                    "png" => d.add_filter("PNG image", &["png"]),
                    _ => d.add_filter("Markdown", &["md", "markdown"]).add_filter("All files", &["*"]),
                };
                let dir = s("dir");
                if !dir.is_empty() {
                    d = d.set_directory(dir);
                }
                Ok(d.save_file()
                    .map(|p| Value::String(p.to_string_lossy().to_string()))
                    .unwrap_or(Value::Null))
            }
            "write_binary" => base64::engine::general_purpose::STANDARD
                .decode(s("data"))
                .map_err(|e| e.to_string())
                .and_then(|b| std::fs::write(s("path"), b).map_err(|e| e.to_string()))
                .map(|_| Value::Null),
            "pick_image" => {
                let mut d = rfd::FileDialog::new()
                    .set_parent(&self.window)
                    .add_filter("Images", &MEDIA_EXTS[..12]);
                let dir = s("dir");
                if !dir.is_empty() {
                    d = d.set_directory(dir);
                }
                Ok(d.pick_file()
                    .map(|p| Value::String(p.to_string_lossy().to_string()))
                    .unwrap_or(Value::Null))
            }
            "save_image" => (|| {
                let dir = PathBuf::from(s("dir")).join("assets");
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(s("data"))
                    .map_err(|e| e.to_string())?;
                let ext: String = s("ext").chars().filter(|c| c.is_ascii_alphanumeric()).collect();
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                let name = format!("image-{stamp}.{}", if ext.is_empty() { "png" } else { &ext });
                std::fs::write(dir.join(&name), bytes).map_err(|e| e.to_string())?;
                Ok(Value::String(format!("assets/{name}")))
            })(),
            "list_dir" => list_dir(Path::new(&s("path"))),
            "watch" => {
                let paths = a["paths"]
                    .as_array()
                    .map(|v| v.iter().filter_map(|p| p.as_str().map(PathBuf::from)).collect())
                    .unwrap_or_default();
                self.watch.set(paths);
                Ok(Value::Null)
            }
            "set_title" => {
                self.window.set_title(&s("title"));
                Ok(Value::Null)
            }
            "set_theme" => {
                let dark = a["dark"].as_bool().unwrap_or(false);
                self.window.set_theme(Some(if dark { Theme::Dark } else { Theme::Light }));
                Ok(Value::Null)
            }
            "save_settings" => {
                write_json(&settings_path(), &a["settings"]);
                Ok(Value::Null)
            }
            "open_external" => open_external(&s("target")).map(|_| Value::Null),
            "reveal" => {
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    let _ = std::process::Command::new("explorer")
                        .raw_arg(format!("/select,\"{}\"", s("path")))
                        .spawn();
                }
                #[cfg(not(windows))]
                if let Some(dir) = Path::new(&s("path")).parent() {
                    shell_open(&dir.to_string_lossy());
                }
                Ok(Value::Null)
            }
            "new_window" => {
                let mut c = std::process::Command::new(std::env::current_exe().unwrap_or_default());
                let p = s("path");
                if !p.is_empty() {
                    c.arg(p);
                }
                c.spawn().map(|_| Value::Null).map_err(|e| e.to_string())
            }
            "print" => self.webview.print().map(|_| Value::Null).map_err(|e| e.to_string()),
            "devtools" => {
                self.webview.open_devtools();
                Ok(Value::Null)
            }
            "install" => install::install().map(|m| {
                json!({ "message": m, "isDefault": install::is_default_for_md() })
            }),
            "default_apps" => {
                install::open_default_apps_settings();
                Ok(Value::Null)
            }
            "close" => {
                self.save_window_state();
                return false;
            }
            _ => Err(format!("unknown command: {cmd}")),
        };
        if id != 0 {
            self.reply(id, result);
        }
        true
    }

    fn save_window_state(&self) {
        let scale = self.window.scale_factor();
        let size = self.window.inner_size().to_logical::<f64>(scale);
        let maximized = self.window.is_maximized();
        let mut v = read_json(&window_state_path());
        if !v.is_object() {
            v = json!({});
        }
        v["maximized"] = json!(maximized);
        if !maximized && size.width > 200.0 {
            v["width"] = json!(size.width);
            v["height"] = json!(size.height);
        }
        write_json(&window_state_path(), &v);
    }
}

fn run_cli(args: &[String]) -> Option<i32> {
    let quiet = args.iter().any(|a| a == "--quiet" || a == "/quiet" || a == "/S");
    // A copy named "...Setup.exe" behaves as an installer when double-clicked.
    let setup = cfg!(windows)
        && std::env::current_exe()
            .ok()
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_lowercase()))
            .is_some_and(|s| s.contains("setup"));
    let removing = args.iter().any(|a| a == "--uninstall");
    let installing = args.iter().any(|a| a == "--install") || (setup && !removing);
    let action = if installing {
        install::install()
    } else if args.iter().any(|a| a == "--uninstall") {
        install::uninstall()
    } else {
        return None;
    };
    let code = match &action {
        Ok(msg) => {
            if !quiet {
                install::message_box(msg, false);
                if installing && !install::is_default_for_md() {
                    install::open_default_apps_settings();
                }
            }
            0
        }
        Err(msg) => {
            if !quiet {
                install::message_box(msg, true);
            }
            1
        }
    };
    if quiet {
        // Leave a trace for scripted installs.
        let log = std::env::temp_dir().join("inkdown-install.log");
        let _ = std::fs::write(log, action.unwrap_or_else(|e| format!("ERROR: {e}")));
    }
    #[cfg(windows)]
    if setup && installing && code == 0 {
        let _ = std::process::Command::new(install::install_dir().join(install::EXE_NAME)).spawn();
    }
    Some(code)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = run_cli(&args) {
        std::process::exit(code);
    }
    let files: Vec<PathBuf> = args
        .iter()
        .filter(|a| !a.starts_with("--"))
        .map(|a| std::path::absolute(a).unwrap_or_else(|_| PathBuf::from(a)))
        .collect();

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let state = read_json(&window_state_path());
    let width = state["width"].as_f64().unwrap_or(1320.0).clamp(480.0, 6000.0);
    let height = state["height"].as_f64().unwrap_or(860.0).clamp(360.0, 4000.0);
    #[cfg(windows)]
    let icon = |px: u32| {
        use tao::platform::windows::IconExtWindows;
        Icon::from_resource(1, Some(PhysicalSize::new(px, px))).ok()
    };
    #[cfg(not(windows))]
    let icon = |_px: u32| -> Option<Icon> { let _ = PhysicalSize::new(0u32, 0u32); None };

    let window = WindowBuilder::new()
        .with_title(install::APP_NAME)
        .with_inner_size(LogicalSize::new(width, height))
        .with_min_inner_size(LogicalSize::new(520.0, 360.0))
        .with_maximized(state["maximized"].as_bool().unwrap_or(false))
        .with_window_icon(icon(32))
        .with_visible(false)
        .build(&event_loop)
        .expect("failed to create window");
    #[cfg(windows)]
    {
        use tao::platform::windows::WindowExtWindows;
        window.set_taskbar_icon(icon(256));
    }

    let data_dir = std::env::var_os("INKDOWN_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(install::data_dir)
        .join("WebView2");
    let _ = std::fs::create_dir_all(&data_dir);
    let mut context = WebContext::new(Some(data_dir));

    let ipc_proxy = proxy.clone();
    let drop_proxy = proxy.clone();
    let builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_custom_protocol("ink".into(), |_id, req| serve(req))
        .with_url(format!("{APP_ORIGIN}index.html"))
        .with_ipc_handler(move |req: Request<String>| {
            let _ = ipc_proxy.send_event(UserEvent::Ipc(req.into_body()));
        })
        .with_drag_drop_handler(move |ev| {
            if let DragDropEvent::Drop { paths, .. } = ev {
                if !paths.is_empty() {
                    let _ = drop_proxy.send_event(UserEvent::Dropped(paths));
                }
            }
            true
        })
        // The app is a single page; anything else goes to the system browser.
        .with_navigation_handler(|url| {
            if url.starts_with(APP_ORIGIN) {
                return true;
            }
            let _ = open_external(&url);
            false
        })
        .with_new_window_req_handler(|url, _features| {
            let _ = open_external(&url);
            NewWindowResponse::Deny
        })
        .with_background_color((24, 26, 31, 255))
        .with_clipboard(true)
        .with_hotkeys_zoom(false)
        .with_devtools(true);
    // The page suppresses the native menu everywhere except the rich editor,
    // where it provides spelling suggestions.
    #[cfg(windows)]
    let mut builder = builder.with_browser_accelerator_keys(false).with_default_context_menus(true);
    #[cfg(windows)]
    if let Ok(port) = std::env::var("INKDOWN_DEBUG_PORT") {
        builder = builder.with_additional_browser_args(format!(
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
             --remote-debugging-port={port} --remote-debugging-address=127.0.0.1"
        ));
    }
    #[cfg(any(windows, target_os = "macos"))]
    let built = builder.build(&window);
    #[cfg(not(any(windows, target_os = "macos")))]
    let built = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(window.default_vbox().expect("gtk window"))
    };
    let webview = match built {
        Ok(w) => w,
        Err(e) => {
            install::message_box(
                &format!("Inkdown could not start its web view (on Windows this needs the Microsoft Edge WebView2 Runtime).\n\n{e}"),
                true,
            );
            std::process::exit(1);
        }
    };

    let mut app = App {
        window,
        webview,
        proxy: proxy.clone(),
        watch: FileWatch::new(proxy),
        initial_files: files,
        ready: false,
    };

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(UserEvent::Ipc(raw)) => {
                if !app.handle_ipc(&raw) {
                    *control_flow = ControlFlow::Exit;
                }
            }
            Event::UserEvent(UserEvent::Reply(id, result)) => app.reply(id, result),
            Event::UserEvent(UserEvent::FileChanged(p)) => {
                let path = Value::String(p.to_string_lossy().to_string());
                app.eval(&format!("window.ink&&ink.onFileChanged({path})"));
            }
            Event::UserEvent(UserEvent::Dropped(paths)) => {
                let list: Vec<String> = paths.iter().map(|p| p.to_string_lossy().to_string()).collect();
                app.eval(&format!("window.ink&&ink.onDrop({})", json!(list)));
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                if app.ready {
                    // The page decides (unsaved changes prompt) and answers with `close`.
                    app.eval("ink.requestClose()");
                } else {
                    *control_flow = ControlFlow::Exit;
                }
            }
            Event::WindowEvent { event: WindowEvent::Focused(true), .. } => {
                let _ = app.webview.focus();
            }
            _ => {}
        }
    });
}
