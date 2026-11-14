//! Inkdown with a pure-Rust UI and renderer: no web view, no JavaScript.
//!
//! One code base for Windows, Linux, macOS, Android and iOS (egui).

pub mod adoc;
pub mod app;
pub mod assets;
pub mod automation;
pub mod buffer;
pub mod chart;
#[path = "../../src/data.rs"]
pub mod data;
pub mod designer;
#[path = "../../src/export.rs"]
pub mod docx;
pub mod doc;
pub mod edit;
pub mod editor;
pub mod export_ui;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[path = "../../src/google.rs"]
pub mod google;
pub mod html;
pub mod icons;
pub mod install;
/// Per-user install, file associations and uninstall (shared with the older build).
#[cfg(windows)]
#[path = "../../src/install.rs"]
pub mod winstall;
pub mod keymap;
pub mod md;
pub mod mdfmt;
#[cfg(any(target_os = "android", target_os = "ios"))]
pub mod mobile;
pub mod picker;
pub mod platform;
pub mod preview;
pub mod project;
pub mod rich;
pub mod settings;
pub mod shell;
pub mod sidebar;
pub mod snippets;
pub mod syntax;
pub mod theme;
pub mod tools;

use std::path::PathBuf;

/// Parser options, under the name the shared exporter expects.
pub mod render {
    pub use crate::md::options;
}

#[allow(dead_code)]
pub(crate) fn shell_open(target: &str) {
    platform::shell_open(target);
}

fn icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(include_bytes!("../../assets/icon.png")).ok()?.to_rgba8();
    Some(egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() })
}

/// Handles `--install`, `--uninstall` and a copy named "...Setup.exe". Returns the exit code
/// when the process should end instead of opening the editor.
#[cfg(windows)]
pub fn run_cli(args: &[String]) -> Option<i32> {
    use winstall as install;
    let quiet = args.iter().any(|a| a == "--quiet" || a == "/quiet" || a == "/S");
    let setup = std::env::current_exe().ok().and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_lowercase())).is_some_and(|s| s.contains("setup"));
    let removing = args.iter().any(|a| a == "--uninstall");
    let installing = args.iter().any(|a| a == "--install") || (setup && !removing);
    let action = if installing {
        install::install()
    } else if removing {
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
        let _ = std::fs::write(std::env::temp_dir().join("inkdown-install.log"), action.unwrap_or_else(|e| format!("ERROR: {e}")));
    }
    // a silent install must not open a window
    if setup && installing && code == 0 && !quiet {
        let _ = std::process::Command::new(install::install_dir().join(install::EXE_NAME)).spawn();
    }
    Some(code)
}

/// Desktop entry point.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn run_desktop() -> eframe::Result {
    let files: Vec<PathBuf> = std::env::args().skip(1).filter(|a| !a.starts_with("--")).map(PathBuf::from).collect();
    let mut viewport = egui::ViewportBuilder::default().with_inner_size([1280.0, 840.0]).with_min_inner_size([360.0, 300.0]).with_title("Inkdown").with_drag_and_drop(true).with_app_id("inkdown");
    if let Some(i) = icon() {
        viewport = viewport.with_icon(i);
    }
    if let Some(size) = std::env::var("INKDOWN_WINDOW").ok().and_then(|s| s.split_once('x').and_then(|(w, h)| Some([w.parse::<f32>().ok()?, h.parse::<f32>().ok()?]))) {
        viewport = viewport.with_inner_size(size);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("Inkdown", options, Box::new(move |cc| Ok(Box::new(app::InkApp::new(cc, files)))))
}

/// Android entry point (NativeActivity).
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    mobile::set_app(app.clone());
    // settings, session and the app's own documents live in the app's private storage
    if let Some(dir) = app.internal_data_path() {
        std::env::set_var("INKDOWN_ANDROID_DIR", dir);
    }
    let docs = mobile::documents_dir();
    let _ = std::fs::create_dir_all(&docs);
    // the document another app asked us to open, then our own folder for the drawer
    let mut files: Vec<PathBuf> = mobile::startup_file().into_iter().collect();
    files.push(docs);
    let options = eframe::NativeOptions { android_app: Some(app), ..Default::default() };
    let _ = eframe::run_native("Inkdown", options, Box::new(move |cc| Ok(Box::new(app::InkApp::new(cc, files)))));
}

/// iOS entry point (called from `main`).
#[cfg(target_os = "ios")]
pub fn run_ios() -> eframe::Result {
    let docs = mobile::documents_dir();
    let _ = std::fs::create_dir_all(&docs);
    eframe::run_native("Inkdown", eframe::NativeOptions::default(), Box::new(move |cc| Ok(Box::new(app::InkApp::new(cc, vec![docs])))))
}