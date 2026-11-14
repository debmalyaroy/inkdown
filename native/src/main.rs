#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    #[cfg(windows)]
    {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if let Some(code) = inkdown_native::run_cli(&args) {
            std::process::exit(code);
        }
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    if let Err(e) = inkdown_native::run_desktop() {
        eprintln!("Inkdown could not start: {e}");
        std::process::exit(1);
    }
    #[cfg(target_os = "ios")]
    {
        let _ = inkdown_native::run_ios();
    }
}
