//! The few things that differ per operating system: file dialogs and shell integration.

use std::path::{Path, PathBuf};

#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod imp {
    use super::*;

    pub fn open_dialog(dir: Option<PathBuf>) -> Vec<PathBuf> {
        // tests answer the dialog with INKDOWN_DIALOG_OPEN (paths separated by `|`)
        if scripted() || std::env::var_os("INKDOWN_DIALOG_DIR").is_some() {
            return std::env::var("INKDOWN_DIALOG_OPEN").map(|s| s.split('|').filter(|p| !p.is_empty()).map(PathBuf::from).collect()).unwrap_or_default();
        }
        let mut d = rfd::FileDialog::new();
        if let Some(dir) = dir.filter(|d| d.is_dir()) {
            d = d.set_directory(dir);
        }
        d.pick_files().unwrap_or_default()
    }

    pub fn save_dialog(name: &str, dir: Option<PathBuf>) -> Option<PathBuf> {
        // tests answer dialogs with a folder instead of a window
        if let Some(d) = std::env::var_os("INKDOWN_DIALOG_DIR") {
            return Some(PathBuf::from(d).join(name));
        }
        let mut d = rfd::FileDialog::new().set_file_name(name);
        if let Some(dir) = dir.filter(|d| d.is_dir()) {
            d = d.set_directory(dir);
        }
        if let Some(ext) = Path::new(name).extension().map(|e| e.to_string_lossy().to_string()) {
            d = d.add_filter(ext.to_uppercase(), &[ext.as_str()]).add_filter("All files", &["*"]);
        }
        d.save_file()
    }

    pub fn folder_dialog(dir: Option<PathBuf>) -> Option<PathBuf> {
        if scripted() {
            return std::env::var_os("INKDOWN_DIALOG_FOLDER").map(PathBuf::from);
        }
        let mut d = rfd::FileDialog::new();
        if let Some(dir) = dir.filter(|d| d.is_dir()) {
            d = d.set_directory(dir);
        }
        d.pick_folder()
    }

    pub fn shell_open(target: &str) {
        if note_launch(target) {
            return;
        }
        #[cfg(windows)]
        {
            // ShellExecute, not `cmd /C start`: URLs contain `&`, which cmd would split on
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
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg(target).spawn();
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let _ = std::process::Command::new("xdg-open").arg(target).spawn();
        }
    }

    pub fn reveal(path: &Path) {
        if note_launch(&format!("reveal:{}", path.display())) {
            return;
        }
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
        }
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            if let Some(dir) = path.parent() {
                shell_open(&dir.to_string_lossy());
            }
        }
    }

    pub fn new_window() {
        if note_launch("new-window") {
            return;
        }
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::process::Command::new(exe).env("INKDOWN_NO_SESSION", "1").spawn();
        }
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
mod imp {
    use super::*;

    pub fn open_dialog(_dir: Option<PathBuf>) -> Vec<PathBuf> {
        crate::mobile::request_open();
        Vec::new()
    }
    pub fn save_dialog(name: &str, _dir: Option<PathBuf>) -> Option<PathBuf> {
        // documents are kept in the app's own folder; sharing goes through the system sheet
        let dir = crate::mobile::documents_dir();
        let _ = std::fs::create_dir_all(&dir);
        Some(dir.join(name))
    }
    pub fn folder_dialog(_dir: Option<PathBuf>) -> Option<PathBuf> {
        Some(crate::mobile::documents_dir())
    }
    pub fn shell_open(target: &str) {
        crate::mobile::open_url(target);
    }
    pub fn reveal(_path: &Path) {}
    pub fn new_window() {}
}

pub use imp::*;

/// True in a scripted test run, where nothing may open a system dialog or another program.
pub fn scripted() -> bool {
    std::env::var_os("INKDOWN_AUTOMATION").is_some()
}

static LAUNCHED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// In a scripted run, records what would have been opened and returns true (do not open).
#[allow(dead_code)]
fn note_launch(target: &str) -> bool {
    if scripted() {
        LAUNCHED.lock().unwrap().push(target.to_string());
        return true;
    }
    false
}

/// Everything a scripted run asked the system to open (pages, folders, windows).
pub fn launched() -> Vec<String> {
    LAUNCHED.lock().unwrap().clone()
}

/// True when the user is working with a finger rather than a mouse.
///
/// Always true on phones and tablets. Elsewhere it stays true for a few seconds after the
/// last touch: egui decides what a press hits from the widgets of the frame before, so the
/// answer must already be right when the finger lands.
pub fn touch(ctx: &egui::Context) -> bool {
    if cfg!(any(target_os = "android", target_os = "ios")) {
        return true;
    }
    use std::sync::atomic::{AtomicU64, Ordering};
    static LAST_TOUCH: AtomicU64 = AtomicU64::new(0);
    let (now, touching) = ctx.input(|i| (i.time, i.any_touches()));
    if touching {
        LAST_TOUCH.store((now + 1.0).to_bits(), Ordering::Relaxed);
    }
    let last = f64::from_bits(LAST_TOUCH.load(Ordering::Relaxed));
    last > 0.0 && now + 1.0 - last < 5.0
}

/// Cheap check used before opening an unknown linked file inside the editor.
pub fn looks_like_text(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else { return false };
    let mut buf = [0u8; 2048];
    let n = f.read(&mut buf).unwrap_or(0);
    !buf[..n].contains(&0)
}

/// Total resident memory of this process in megabytes (for the status bar and tests).
pub fn memory_mb() -> Option<f32> {
    #[cfg(windows)]
    {
        use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
        use windows::Win32::System::Threading::GetCurrentProcess;
        let mut c = PROCESS_MEMORY_COUNTERS::default();
        unsafe {
            if GetProcessMemoryInfo(GetCurrentProcess(), &mut c, std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32).is_ok() {
                return Some(c.WorkingSetSize as f32 / 1048576.0);
            }
        }
        None
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let s = std::fs::read_to_string("/proc/self/statm").ok()?;
        let pages: f32 = s.split_whitespace().nth(1)?.parse().ok()?;
        Some(pages * 4096.0 / 1048576.0)
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "android")))]
    {
        None
    }
}
