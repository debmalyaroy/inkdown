//! Non-Windows stand-in for `install.rs`: where data lives, and no-op installer hooks.
//! Linux and macOS packages register file types through their own packaging
//! (.desktop files, Info.plist), not at run time.

use std::path::PathBuf;

pub const APP_NAME: &str = "Inkdown";

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

pub fn data_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        return home().join("Library/Application Support/Inkdown");
    }
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/share")).join("inkdown")
}

pub fn config_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("INKDOWN_DATA_DIR") {
        return PathBuf::from(d).join("config");
    }
    if cfg!(target_os = "macos") {
        return home().join("Library/Application Support/Inkdown");
    }
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config")).join("inkdown")
}

pub fn message_box(text: &str, _error: bool) {
    eprintln!("{APP_NAME}: {text}");
}

pub fn is_store_package() -> bool {
    true // packaged by the platform; hides the Windows self-install prompt
}
pub fn is_installed_copy() -> bool {
    true
}
pub fn is_default_for_md() -> bool {
    true
}
pub fn install() -> Result<String, String> {
    Err("Installation is handled by the system package on this platform.".into())
}
pub fn uninstall() -> Result<String, String> {
    Err("Remove Inkdown with your system's package manager.".into())
}
pub fn open_default_apps_settings() {}
