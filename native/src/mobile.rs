//! Android / iOS glue: where documents live, and (on Android) reading and writing the
//! document another app handed us through an "Open with" intent.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The app's own document folder (shown in the sidebar on phones).
pub fn documents_dir() -> PathBuf {
    crate::settings::config_dir().join("Documents")
}

/// Files opened from other apps: local working copy -> content URI to write back to.
static ORIGINS: Mutex<Option<HashMap<PathBuf, String>>> = Mutex::new(None);

pub fn request_open() {}

#[cfg(target_os = "android")]
static APP: std::sync::OnceLock<winit::platform::android::activity::AndroidApp> = std::sync::OnceLock::new();

#[cfg(target_os = "android")]
pub fn set_app(app: winit::platform::android::activity::AndroidApp) {
    let _ = APP.set(app);
}

/// Space taken by the system bars at the top and bottom of the window, in pixels.
pub fn insets_px() -> (f32, f32) {
    #[cfg(target_os = "android")]
    if let Some(app) = APP.get() {
        let r = app.content_rect();
        let h = app.native_window().map(|w| w.height()).unwrap_or(0);
        if r.bottom > r.top && h > 0 {
            return (r.top.max(0) as f32, (h - r.bottom).max(0) as f32);
        }
    }
    (0.0, 0.0)
}

/// Text on the system clipboard, if any.
pub fn clipboard_get() -> Option<String> {
    #[cfg(target_os = "android")]
    {
        android::clipboard_get()
    }
    #[cfg(not(target_os = "android"))]
    {
        None
    }
}

/// Puts text on the system clipboard.
pub fn clipboard_set(text: &str) {
    #[cfg(target_os = "android")]
    android::clipboard_set(text);
    #[cfg(not(target_os = "android"))]
    let _ = text;
}

/// Whether the device is in dark mode. Asked at most once a second; `None` when unknown.
pub fn system_dark(now: f64) -> Option<bool> {
    static CACHE: Mutex<(f64, Option<bool>)> = Mutex::new((-10.0, None));
    let mut c = CACHE.lock().unwrap();
    if now - c.0 >= 1.0 {
        c.0 = now;
        #[cfg(target_os = "android")]
        {
            c.1 = android::night_mode().or(c.1);
        }
    }
    c.1
}

pub fn open_url(url: &str) {
    #[cfg(target_os = "android")]
    android::view_url(url);
    #[cfg(not(target_os = "android"))]
    let _ = url;
}

/// Copies a document another app gave us into app storage and remembers where it came
/// from, so that saving writes back to it.
#[cfg(target_os = "android")]
fn import_uri(uri: &str) -> Option<PathBuf> {
    let bytes = android::read_uri(uri);
    log(&format!("read {:?} bytes from {uri}", bytes.as_ref().map(Vec::len)));
    let bytes = bytes?;
    let name = android::display_name(uri).unwrap_or_else(|| uri.rsplit('/').next().unwrap_or("document.md").to_string());
    let name: String = name.chars().map(|c| if c.is_alphanumeric() || "._- ".contains(c) { c } else { '_' }).collect();
    let dir = crate::settings::config_dir().join("Opened");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(if name.is_empty() { "document.md".into() } else { name });
    std::fs::write(&path, bytes).ok()?;
    ORIGINS.lock().unwrap().get_or_insert_with(HashMap::new).insert(canonical(&path), uri.to_string());
    Some(path)
}

/// The document this launch was asked to open, copied into app storage.
pub fn startup_file() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    {
        let uri = android::intent_uri();
        log(&format!("launch intent data: {uri:?}"));
        import_uri(&uri?)
    }
    #[cfg(not(target_os = "android"))]
    {
        None
    }
}

/// Shows the system file picker. Returns false when this build cannot (then the caller
/// falls back to the app's own documents). Picked files arrive through [`take_opened`].
pub fn pick_file() -> bool {
    #[cfg(target_os = "android")]
    {
        android::call_void("pickFile")
    }
    #[cfg(not(target_os = "android"))]
    {
        false
    }
}

/// The next document picked, or sent by another app while Inkdown was running.
pub fn take_opened() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    {
        let uri = android::take_string("takeOpened")?;
        log(&format!("document arrived: {uri}"));
        import_uri(&uri)
    }
    #[cfg(not(target_os = "android"))]
    {
        None
    }
}

/// Shows the system "save as" picker for a file called `name`. False when unavailable.
pub fn pick_save(name: &str) -> bool {
    #[cfg(target_os = "android")]
    {
        android::pick_save(name, mime_of(name))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = name;
        false
    }
}

/// Result of the "save as" picker: `None` while it is open, `Some(None)` when cancelled,
/// `Some(Some(place))` when a document was created.
pub fn take_saved() -> Option<Option<String>> {
    #[cfg(target_os = "android")]
    {
        android::take_string("takeSaved").map(|s| if s.is_empty() { None } else { Some(s) })
    }
    #[cfg(not(target_os = "android"))]
    {
        None
    }
}

/// Writes `bytes` to a document chosen with [`pick_save`] and ties the working copy at
/// `path` to it, so that later saves go there too.
pub fn save_to(place: &str, path: &Path, bytes: &[u8]) -> Result<String, String> {
    #[cfg(target_os = "android")]
    {
        if !android::write_uri(place, bytes) {
            return Err("Could not write to the chosen location.".into());
        }
        ORIGINS.lock().unwrap().get_or_insert_with(HashMap::new).insert(canonical(path), place.to_string());
        Ok(android::display_name(place).unwrap_or_else(|| "the chosen location".into()))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (place, path, bytes);
        Err("Not available on this device.".into())
    }
}

/// Sends an HTML page to the system print dialog (which can save a PDF). False when unavailable.
pub fn print_html(html: &str, name: &str) -> bool {
    #[cfg(target_os = "android")]
    {
        android::print_html(html, name)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (html, name);
        false
    }
}

fn mime_of(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("").to_lowercase().as_str() {
        "html" | "htm" => "text/html",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "rtf" => "application/rtf",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "yaml" | "yml" => "application/x-yaml",
        "xml" => "text/xml",
        "avro" | "bin" => "application/octet-stream",
        "svg" => "image/svg+xml",
        "json" | "avsc" => "application/json",
        "md" | "markdown" => "text/markdown",
        "csv" => "text/csv",
        _ => "text/plain",
    }
}

/// After saving the working copy, write the same bytes back to where the file came from.
pub fn after_save(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let uri = ORIGINS.lock().unwrap().as_ref().and_then(|m| m.get(&canonical(path)).cloned());
    match uri {
        #[cfg(target_os = "android")]
        Some(uri) => {
            let ok = android::write_uri(&uri, bytes);
            log(&format!("write back {} bytes to {uri}: {ok}", bytes.len()));
            if ok {
                Ok(())
            } else {
                Err("Saved a copy in Inkdown, but the original app did not allow writing back.".into())
            }
        }
        _ => {
            let _ = bytes;
            Ok(())
        }
    }
}

/// The app's data folder is reachable under two names (`/data/user/0` and `/data/data`).
fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Puts a file where other apps can reach it: `Download/Inkdown` in shared storage.
/// Returns the place to tell the user about.
pub fn save_to_downloads(name: &str, bytes: &[u8]) -> Result<String, String> {
    #[cfg(target_os = "android")]
    {
        let mime = mime_of(name);
        if android::insert_download(name, mime, bytes) {
            Ok(format!("Downloads/Inkdown/{name}"))
        } else {
            Err("Could not write to the Downloads folder on this device.".into())
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (name, bytes);
        Err("Not available on this device.".into())
    }
}

/// The body of a web answer, read through the activity as it arrives.
#[cfg(target_os = "android")]
pub struct HttpStream(i32);

#[cfg(target_os = "android")]
impl std::io::Read for HttpStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match android::http_read(self.0, buf.len().min(64 * 1024)) {
            Some(bytes) => {
                let n = bytes.len().min(buf.len());
                buf[..n].copy_from_slice(&bytes[..n]);
                Ok(n)
            }
            None => Err(std::io::Error::other("the connection was lost")),
        }
    }
}

#[cfg(target_os = "android")]
impl Drop for HttpStream {
    fn drop(&mut self) {
        android::http_close(self.0);
    }
}

/// Asks for a web address with the system's HTTP client. Returns the head of the answer
/// as `status\netag\nlast-modified\ncontent-range` and the body as a stream.
#[cfg(target_os = "android")]
pub fn http_open(url: &str, from: u64, etag: &str, modified: &str) -> Option<(String, HttpStream)> {
    let (head, id) = android::http_open(url, from, etag, modified)?;
    Some((head, HttpStream(id)))
}

/// Write a line to the Android log (`adb logcat -s Inkdown`).
pub fn log(msg: &str) {
    #[cfg(target_os = "android")]
    {
        #[link(name = "log")]
        extern "C" {
            fn __android_log_write(prio: i32, tag: *const std::ffi::c_char, text: *const std::ffi::c_char) -> i32;
        }
        if let Ok(text) = std::ffi::CString::new(msg) {
            unsafe {
                __android_log_write(4, c"Inkdown".as_ptr(), text.as_ptr());
            }
        }
    }
    #[cfg(not(target_os = "android"))]
    let _ = msg;
}

#[cfg(target_os = "android")]
mod android {
    use super::log;
    use jni::objects::{JObject, JString, JValue};
    use jni::{JNIEnv, JavaVM};

    fn with_env<R>(f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<R>) -> Option<R> {
        let ctx = ndk_context::android_context();
        let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) }.ok()?;
        let mut env = vm.attach_current_thread().ok()?;
        // `ndk_context` holds the Application; intents and `startActivity` need the Activity
        let raw = super::APP.get().map(|a| a.activity_as_ptr()).unwrap_or_else(|| ctx.context());
        let activity = unsafe { JObject::from_raw(raw.cast()) };
        let r = f(&mut env, &activity);
        if env.exception_check().unwrap_or(false) {
            let ex = env.exception_occurred();
            let _ = env.exception_clear();
            if let Ok(ex) = ex {
                let text = env.call_method(&ex, "toString", "()Ljava/lang/String;", &[]).and_then(|v| v.l()).ok();
                if let Some(text) = text {
                    let text: Option<String> = env.get_string(&JString::from(text)).ok().map(Into::into);
                    log(&format!("Java exception: {}", text.unwrap_or_default()));
                }
                let _ = env.exception_clear();
            }
        }
        if let Err(e) = &r {
            log(&format!("JNI call failed: {e}"));
        }
        r.ok()
    }

    fn parse_uri<'a>(env: &mut JNIEnv<'a>, uri: &str) -> jni::errors::Result<JObject<'a>> {
        let s = env.new_string(uri)?;
        env.call_static_method("android/net/Uri", "parse", "(Ljava/lang/String;)Landroid/net/Uri;", &[JValue::Object(&s)])?.l()
    }

    /// Calls a no-argument method of our Java activity. False when the activity is the plain
    /// NativeActivity (an older package) and has no such method.
    pub fn call_void(method: &str) -> bool {
        with_env(|env, act| {
            env.call_method(act, method, "()V", &[])?;
            Ok(())
        })
        .is_some()
    }

    /// Calls a method of our Java activity that returns a string or null.
    pub fn take_string(method: &str) -> Option<String> {
        with_env(|env, act| {
            let s = env.call_method(act, method, "()Ljava/lang/String;", &[])?.l()?;
            if s.is_null() {
                return Ok(None);
            }
            let s: String = env.get_string(&JString::from(s))?.into();
            Ok(Some(s))
        })
        .flatten()
    }

    pub fn pick_save(name: &str, mime: &str) -> bool {
        with_env(|env, act| {
            let n = env.new_string(name)?;
            let m = env.new_string(mime)?;
            env.call_method(act, "pickSave", "(Ljava/lang/String;Ljava/lang/String;)V", &[JValue::Object(&n), JValue::Object(&m)])?;
            Ok(())
        })
        .is_some()
    }

    pub fn print_html(html: &str, name: &str) -> bool {
        with_env(|env, act| {
            let h = env.new_string(html)?;
            let n = env.new_string(name)?;
            env.call_method(act, "printHtml", "(Ljava/lang/String;Ljava/lang/String;)V", &[JValue::Object(&h), JValue::Object(&n)])?;
            Ok(())
        })
        .is_some()
    }

    fn clipboard<'a>(env: &mut JNIEnv<'a>, act: &JObject) -> jni::errors::Result<JObject<'a>> {
        let name = env.new_string("clipboard")?;
        env.call_method(act, "getSystemService", "(Ljava/lang/String;)Ljava/lang/Object;", &[JValue::Object(&name)])?.l()
    }

    pub fn clipboard_set(text: &str) {
        let _ = with_env(|env, act| {
            let cm = clipboard(env, act)?;
            let label = env.new_string("Inkdown")?;
            let value = env.new_string(text)?;
            let clip = env.call_static_method("android/content/ClipData", "newPlainText", "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Landroid/content/ClipData;", &[JValue::Object(&label), JValue::Object(&value)])?.l()?;
            env.call_method(&cm, "setPrimaryClip", "(Landroid/content/ClipData;)V", &[JValue::Object(&clip)])?;
            Ok(())
        });
    }

    pub fn clipboard_get() -> Option<String> {
        with_env(|env, act| {
            let cm = clipboard(env, act)?;
            let clip = env.call_method(&cm, "getPrimaryClip", "()Landroid/content/ClipData;", &[])?.l()?;
            if clip.is_null() || env.call_method(&clip, "getItemCount", "()I", &[])?.i()? == 0 {
                return Ok(None);
            }
            let item = env.call_method(&clip, "getItemAt", "(I)Landroid/content/ClipData$Item;", &[JValue::Int(0)])?.l()?;
            let chars = env.call_method(&item, "coerceToText", "(Landroid/content/Context;)Ljava/lang/CharSequence;", &[JValue::Object(act)])?.l()?;
            if chars.is_null() {
                return Ok(None);
            }
            let s = env.call_method(&chars, "toString", "()Ljava/lang/String;", &[])?.l()?;
            let s: String = env.get_string(&JString::from(s))?.into();
            Ok(Some(s))
        })
        .flatten()
    }

    /// `Configuration.uiMode`: the night bits say whether the system theme is dark.
    pub fn night_mode() -> Option<bool> {
        const UI_MODE_NIGHT_MASK: i32 = 0x30;
        const UI_MODE_NIGHT_YES: i32 = 0x20;
        with_env(|env, act| {
            let res = env.call_method(act, "getResources", "()Landroid/content/res/Resources;", &[])?.l()?;
            let cfg = env.call_method(&res, "getConfiguration", "()Landroid/content/res/Configuration;", &[])?.l()?;
            let mode = env.get_field(&cfg, "uiMode", "I")?.i()?;
            Ok(mode & UI_MODE_NIGHT_MASK == UI_MODE_NIGHT_YES)
        })
    }

    pub fn intent_uri() -> Option<String> {
        with_env(|env, act| {
            let intent = env.call_method(act, "getIntent", "()Landroid/content/Intent;", &[])?.l()?;
            if intent.is_null() {
                return Ok(None);
            }
            let uri = env.call_method(&intent, "getData", "()Landroid/net/Uri;", &[])?.l()?;
            if uri.is_null() {
                return Ok(None);
            }
            let s = env.call_method(&uri, "toString", "()Ljava/lang/String;", &[])?.l()?;
            let s: String = env.get_string(&JString::from(s))?.into();
            Ok(Some(s))
        })
        .flatten()
    }

    /// `(head, connection id)`; the head starts with status 0 when the request failed.
    pub fn http_open(url: &str, from: u64, etag: &str, modified: &str) -> Option<(String, i32)> {
        with_env(|env, act| {
            let u = env.new_string(url)?;
            let e = env.new_string(etag)?;
            let m = env.new_string(modified)?;
            let id = env.call_method(act, "httpOpen", "(Ljava/lang/String;JLjava/lang/String;Ljava/lang/String;)I", &[JValue::Object(&u), JValue::Long(from as i64), JValue::Object(&e), JValue::Object(&m)])?.i()?;
            let head = env.call_method(act, "httpHead", "(I)Ljava/lang/String;", &[JValue::Int(id)])?.l()?;
            let head: String = env.get_string(&JString::from(head))?.into();
            Ok((head, id))
        })
    }

    /// The next piece of the body: empty at its end, `None` when the connection failed.
    pub fn http_read(id: i32, max: usize) -> Option<Vec<u8>> {
        with_env(|env, act| {
            let out = env.call_method(act, "httpRead", "(II)[B", &[JValue::Int(id), JValue::Int(max as i32)])?.l()?;
            if out.is_null() {
                return Ok(None);
            }
            Ok(Some(env.convert_byte_array(jni::objects::JByteArray::from(out))?))
        })
        .flatten()
    }

    pub fn http_close(id: i32) {
        with_env(|env, act| {
            env.call_method(act, "httpClose", "(I)V", &[JValue::Int(id)])?;
            Ok(())
        });
    }

    pub fn read_uri(uri: &str) -> Option<Vec<u8>> {
        with_env(|env, act| {
            let juri = parse_uri(env, uri)?;
            let resolver = env.call_method(act, "getContentResolver", "()Landroid/content/ContentResolver;", &[])?.l()?;
            let stream = env.call_method(&resolver, "openInputStream", "(Landroid/net/Uri;)Ljava/io/InputStream;", &[JValue::Object(&juri)])?.l()?;
            let buf = env.new_byte_array(64 * 1024)?;
            let mut out = Vec::new();
            loop {
                let n = env.call_method(&stream, "read", "([B)I", &[JValue::Object(&buf)])?.i()?;
                if n <= 0 || out.len() > 64 << 20 {
                    break;
                }
                let mut chunk = vec![0i8; n as usize];
                env.get_byte_array_region(&buf, 0, &mut chunk)?;
                out.extend(chunk.iter().map(|b| *b as u8));
            }
            env.call_method(&stream, "close", "()V", &[])?;
            Ok(out)
        })
    }

    pub fn write_uri(uri: &str, bytes: &[u8]) -> bool {
        with_env(|env, act| {
            let juri = parse_uri(env, uri)?;
            let mode = env.new_string("wt")?;
            let resolver = env.call_method(act, "getContentResolver", "()Landroid/content/ContentResolver;", &[])?.l()?;
            let stream = env.call_method(&resolver, "openOutputStream", "(Landroid/net/Uri;Ljava/lang/String;)Ljava/io/OutputStream;", &[JValue::Object(&juri), JValue::Object(&mode)])?.l()?;
            let arr = env.byte_array_from_slice(bytes)?;
            env.call_method(&stream, "write", "([B)V", &[JValue::Object(&arr)])?;
            env.call_method(&stream, "close", "()V", &[])?;
            Ok(true)
        })
        .unwrap_or(false)
    }

    /// Creates `Download/Inkdown/<name>` through MediaStore (Android 10 and later; needs no
    /// permission) and writes the bytes to it.
    pub fn insert_download(name: &str, mime: &str, bytes: &[u8]) -> bool {
        with_env(|env, act| {
            let sdk = env.get_static_field("android/os/Build$VERSION", "SDK_INT", "I")?.i()?;
            if sdk < 29 {
                return Ok(false);
            }
            let values = env.new_object("android/content/ContentValues", "()V", &[])?;
            for (key, value) in [("_display_name", name), ("mime_type", mime), ("relative_path", "Download/Inkdown")] {
                let k = env.new_string(key)?;
                let v = env.new_string(value)?;
                env.call_method(&values, "put", "(Ljava/lang/String;Ljava/lang/String;)V", &[JValue::Object(&k), JValue::Object(&v)])?;
            }
            let collection = env.get_static_field("android/provider/MediaStore$Downloads", "EXTERNAL_CONTENT_URI", "Landroid/net/Uri;")?.l()?;
            let resolver = env.call_method(act, "getContentResolver", "()Landroid/content/ContentResolver;", &[])?.l()?;
            let item = env.call_method(&resolver, "insert", "(Landroid/net/Uri;Landroid/content/ContentValues;)Landroid/net/Uri;", &[JValue::Object(&collection), JValue::Object(&values)])?.l()?;
            if item.is_null() {
                return Ok(false);
            }
            let stream = env.call_method(&resolver, "openOutputStream", "(Landroid/net/Uri;)Ljava/io/OutputStream;", &[JValue::Object(&item)])?.l()?;
            let arr = env.byte_array_from_slice(bytes)?;
            env.call_method(&stream, "write", "([B)V", &[JValue::Object(&arr)])?;
            env.call_method(&stream, "close", "()V", &[])?;
            Ok(true)
        })
        .unwrap_or(false)
    }

    pub fn display_name(uri: &str) -> Option<String> {
        with_env(|env, act| {
            let juri = parse_uri(env, uri)?;
            let resolver = env.call_method(act, "getContentResolver", "()Landroid/content/ContentResolver;", &[])?.l()?;
            let null = JObject::null();
            let cursor = env
                .call_method(&resolver, "query", "(Landroid/net/Uri;[Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;Ljava/lang/String;)Landroid/database/Cursor;", &[JValue::Object(&juri), JValue::Object(&null), JValue::Object(&null), JValue::Object(&null), JValue::Object(&null)])?
                .l()?;
            if cursor.is_null() {
                return Ok(None);
            }
            let mut name = None;
            if env.call_method(&cursor, "moveToFirst", "()Z", &[])?.z()? {
                let col = env.new_string("_display_name")?;
                let idx = env.call_method(&cursor, "getColumnIndex", "(Ljava/lang/String;)I", &[JValue::Object(&col)])?.i()?;
                if idx >= 0 {
                    let s = env.call_method(&cursor, "getString", "(I)Ljava/lang/String;", &[JValue::Int(idx)])?.l()?;
                    if !s.is_null() {
                        let s: String = env.get_string(&JString::from(s))?.into();
                        name = Some(s);
                    }
                }
            }
            env.call_method(&cursor, "close", "()V", &[])?;
            Ok(name)
        })
        .flatten()
    }

    pub fn view_url(url: &str) {
        let _ = with_env(|env, act| {
            let juri = parse_uri(env, url)?;
            let action = env.new_string("android.intent.action.VIEW")?;
            let intent = env.new_object("android/content/Intent", "(Ljava/lang/String;Landroid/net/Uri;)V", &[JValue::Object(&action), JValue::Object(&juri)])?;
            env.call_method(act, "startActivity", "(Landroid/content/Intent;)V", &[JValue::Object(&intent)])?;
            Ok(())
        });
    }
}
