//! Unsaved work, kept on disk.
//!
//! Every buffer with unsaved text (an untitled note, or a file with changes) is written to
//! its own small file a moment after typing stops, and removed again when the document is
//! saved or deliberately closed. The next start restores them, however the last run ended:
//! a normal exit, a crash, a forced shutdown, or the system killing the app on a phone.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Draft {
    pub id: String,
    /// Tab title.
    pub name: String,
    /// The file this is an edited version of, if any.
    pub path: Option<String>,
    pub text: String,
    pub mode: String,
    pub caret: usize,
    /// The process that wrote it, so that a second window does not adopt the drafts of a
    /// window that is still open.
    pub pid: u32,
}

pub fn dir() -> PathBuf {
    crate::settings::config_dir().join("drafts")
}

fn file(dir: &std::path::Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.json"))
}

/// A new identifier for a document's draft file.
pub fn new_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    format!("{:x}-{:x}-{:x}", nanos, std::process::id(), N.fetch_add(1, Ordering::Relaxed))
}

/// Writes the draft so that a crash in the middle never leaves half a file.
pub fn write(d: &Draft) -> std::io::Result<()> {
    write_in(&dir(), d)
}

fn write_in(dir: &std::path::Path, d: &Draft) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let text = serde_json::to_string(d).map_err(std::io::Error::other)?;
    let tmp = dir.join(format!("{}.tmp", d.id));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, file(dir, &d.id))
}

pub fn remove(id: &str) {
    let _ = std::fs::remove_file(file(&dir(), id));
}

/// Every draft on disk, oldest first.
pub fn load_all() -> Vec<Draft> {
    load_from(&dir())
}

fn load_from(dir: &std::path::Path) -> Vec<Draft> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut found: Vec<(std::time::SystemTime, Draft)> = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        let Ok(mut d) = serde_json::from_str::<Draft>(&text) else { continue };
        if d.id.is_empty() {
            d.id = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        }
        let when = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
        found.push((when, d));
    }
    found.sort_by_key(|f| f.0);
    found.into_iter().map(|f| f.1).collect()
}

/// Whether the process that wrote a draft is still running (then the draft is its to keep).
pub fn writer_alive(pid: u32) -> bool {
    if pid == 0 || pid == std::process::id() {
        return false;
    }
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        unsafe {
            let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
            let mut code = 0u32;
            let running = GetExitCodeProcess(h, &mut code).is_ok() && code == 259; // STILL_ACTIVE
            let _ = CloseHandle(h);
            running
        }
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "android")))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let base = std::env::temp_dir().join(format!("ink-drafts-{}", std::process::id()));
        let a = Draft { id: new_id(), name: "Untitled-1.md".into(), path: None, text: "first\nline".into(), mode: "write".into(), caret: 3, pid: 0 };
        let b = Draft { id: new_id(), name: "b.txt".into(), path: Some("/x/b.txt".into()), text: "edited".into(), mode: "code".into(), caret: 0, pid: std::process::id() };
        assert_ne!(a.id, b.id);
        write_in(&base, &a).unwrap();
        write_in(&base, &b).unwrap();
        // writing again replaces, it does not add
        write_in(&base, &Draft { text: "first\nline two".into(), ..a.clone() }).unwrap();
        let all = load_from(&base);
        assert_eq!(all.len(), 2);
        let ra = all.iter().find(|d| d.id == a.id).unwrap();
        assert_eq!((ra.name.as_str(), ra.text.as_str(), ra.mode.as_str(), ra.caret), ("Untitled-1.md", "first\nline two", "write", 3));
        assert_eq!(all.iter().find(|d| d.id == b.id).unwrap().path.as_deref(), Some("/x/b.txt"));
        std::fs::remove_file(file(&base, &a.id)).unwrap();
        assert_eq!(load_from(&base).len(), 1);
        assert!(!writer_alive(std::process::id()) && !writer_alive(0));
        let _ = std::fs::remove_dir_all(&base);
    }
}
