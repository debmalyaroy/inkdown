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

pub fn open_url(url: &str) {
    #[cfg(target_os = "android")]
    android::view_url(url);
    #[cfg(not(target_os = "android"))]
    let _ = url;
}

/// The document this launch was asked to open, copied into app storage.
pub fn startup_file() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    {
        let uri = android::intent_uri();
        log(&format!("launch intent data: {uri:?}"));
        let uri = uri?;
        let bytes = android::read_uri(&uri);
        log(&format!("read {:?} bytes from {uri}", bytes.as_ref().map(Vec::len)));
        let bytes = bytes?;
        let name = android::display_name(&uri).unwrap_or_else(|| uri.rsplit('/').next().unwrap_or("document.md").to_string());
        let name: String = name.chars().map(|c| if c.is_alphanumeric() || "._- ".contains(c) { c } else { '_' }).collect();
        let dir = crate::settings::config_dir().join("Opened");
        std::fs::create_dir_all(&dir).ok()?;
        let path = dir.join(if name.is_empty() { "document.md".into() } else { name });
        std::fs::write(&path, bytes).ok()?;
        ORIGINS.lock().unwrap().get_or_insert_with(HashMap::new).insert(canonical(&path), uri);
        Some(path)
    }
    #[cfg(not(target_os = "android"))]
    {
        None
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
