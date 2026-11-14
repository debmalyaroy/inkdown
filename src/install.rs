//! Per-user install / uninstall: copies the exe, registers the Markdown file
//! associations, Start Menu shortcut and the "Apps & features" uninstall entry.
//! Everything lives under HKCU and %LOCALAPPDATA%, so no elevation is needed.

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows::core::{Interface, PCWSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::{
    ApplicationAssociationRegistration, IApplicationAssociationRegistration, IShellLinkW,
    SHChangeNotify, ShellLink, AL_EFFECTIVE, AT_FILEEXTENSION, SHCNE_ASSOCCHANGED, SHCNF_IDLIST,
};
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK,
};
use winreg::enums::*;
use winreg::RegKey;

pub const APP_NAME: &str = "Inkdown";
pub const PROG_ID: &str = "Inkdown.Markdown";
pub const EXE_NAME: &str = "inkdown.exe";
pub const EXTENSIONS: &[&str] = &[".md", ".markdown", ".mdown", ".mkd", ".mkdn", ".mdwn"];

pub const OPEN_WITH_EXTENSIONS: &[&str] = &[
    ".txt", ".adoc", ".asciidoc", ".json", ".yaml", ".yml", ".toml", ".csv", ".tsv", ".xml", ".html", ".mmd", ".mermaid", ".dot", ".gv", ".svg",
];
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Inkdown";
const FILE_EXTS: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts";

fn env_dir(var: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(var).unwrap_or_default())
}

/// %LOCALAPPDATA%\Programs\Inkdown
pub fn install_dir() -> PathBuf {
    env_dir("LOCALAPPDATA").join("Programs").join(APP_NAME)
}
/// %LOCALAPPDATA%\Inkdown  (WebView2 profile)
pub fn data_dir() -> PathBuf {
    env_dir("LOCALAPPDATA").join(APP_NAME)
}
/// %APPDATA%\Inkdown  (settings)
pub fn config_dir() -> PathBuf {
    // INKDOWN_DATA_DIR gives a fully separate profile (portable use, automated tests)
    match std::env::var_os("INKDOWN_DATA_DIR") {
        Some(d) => PathBuf::from(d).join("config"),
        None => env_dir("APPDATA").join(APP_NAME),
    }
}
fn shortcut_path() -> PathBuf {
    env_dir("APPDATA")
        .join(r"Microsoft\Windows\Start Menu\Programs")
        .join(format!("{APP_NAME}.lnk"))
}

fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

pub fn message_box(text: &str, error: bool) {
    let t = wide(text.as_ref());
    let c = wide(APP_NAME.as_ref());
    let icon = if error { MB_ICONERROR } else { MB_ICONINFORMATION };
    unsafe {
        MessageBoxW(None, PCWSTR(t.as_ptr()), PCWSTR(c.as_ptr()), MB_OK | icon);
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// True when running from a Microsoft Store / MSIX package, which manages install and file types itself.
pub fn is_store_package() -> bool {
    std::env::current_exe().map(|e| e.to_string_lossy().contains("\\WindowsApps\\")).unwrap_or(false)
}

/// True when the running exe is the installed copy.
pub fn is_installed_copy() -> bool {
    std::env::current_exe()
        .map(|e| same_file(&e, &install_dir().join(EXE_NAME)))
        .unwrap_or(false)
}

/// True when Windows will actually open `.md` with Inkdown. Asks the shell for
/// the *effective* default, which honours the user's protected choice.
pub fn is_default_for_md() -> bool {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let reg: windows::core::Result<IApplicationAssociationRegistration> =
            CoCreateInstance(&ApplicationAssociationRegistration, None, CLSCTX_INPROC_SERVER);
        if let Ok(reg) = reg {
            let ext = wide(".md".as_ref());
            if let Ok(p) = reg.QueryCurrentDefault(PCWSTR(ext.as_ptr()), AT_FILEEXTENSION, AL_EFFECTIVE) {
                let current = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as _));
                return current == PROG_ID;
            }
        }
    }
    false
}

fn create_shortcut(target: &Path, lnk: &Path) -> windows::core::Result<()> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        let t = wide(target.as_os_str());
        link.SetPath(PCWSTR(t.as_ptr()))?;
        let d = wide("Markdown viewer and editor".as_ref());
        link.SetDescription(PCWSTR(d.as_ptr()))?;
        link.SetIconLocation(PCWSTR(t.as_ptr()), 0)?;
        if let Some(dir) = target.parent() {
            let w = wide(dir.as_os_str());
            link.SetWorkingDirectory(PCWSTR(w.as_ptr()))?;
        }
        let file: IPersistFile = link.cast()?;
        let l = wide(lnk.as_os_str());
        file.Save(PCWSTR(l.as_ptr()), true)?;
    }
    Ok(())
}

fn notify_shell() {
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
}

fn register(exe: &Path) -> io::Result<()> {
    let exe_s = exe.to_string_lossy().to_string();
    let open_cmd = format!("\"{exe_s}\" \"%1\"");
    let icon = format!("\"{exe_s}\",0");
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let classes = hkcu.create_subkey(r"Software\Classes")?.0;

    // ProgID
    let prog = classes.create_subkey(PROG_ID)?.0;
    prog.set_value("", &"Markdown Document")?;
    prog.set_value("FriendlyTypeName", &"Markdown Document")?;
    prog.create_subkey("DefaultIcon")?.0.set_value("", &icon)?;
    let open = prog.create_subkey(r"shell\open")?.0;
    open.set_value("", &format!("Open with {APP_NAME}"))?;
    open.create_subkey("command")?.0.set_value("", &open_cmd)?;

    // Application registration (drives the "Open with" list)
    let app = classes.create_subkey(format!(r"Applications\{EXE_NAME}"))?.0;
    app.set_value("FriendlyAppName", &APP_NAME)?;
    app.create_subkey("DefaultIcon")?.0.set_value("", &icon)?;
    app.create_subkey(r"shell\open\command")?.0.set_value("", &open_cmd)?;

    // Default-apps capabilities (so Inkdown is listed in Settings > Default apps)
    let caps = hkcu.create_subkey(r"Software\Inkdown\Capabilities")?.0;
    caps.set_value("ApplicationName", &APP_NAME)?;
    caps.set_value("ApplicationDescription", &"Markdown viewer and editor")?;
    let caps_assoc = caps.create_subkey("FileAssociations")?.0;
    hkcu.create_subkey(r"Software\RegisteredApplications")?
        .0
        .set_value(APP_NAME, &r"Software\Inkdown\Capabilities")?;

    // Inkdown is a general editor: without a SupportedTypes list Windows offers it under
    // "Open with" for every file type, and "Edit with Inkdown" appears on every file.
    // Neither changes any default app.
    let _ = OPEN_WITH_EXTENSIONS;
    let _ = app.delete_subkey_all("SupportedTypes");
    let verb = classes.create_subkey(r"*\shell\Inkdown")?.0;
    verb.set_value("", &"Edit with Inkdown")?;
    verb.set_value("Icon", &icon)?;
    verb.create_subkey("command")?.0.set_value("", &open_cmd)?;

    let backup = hkcu.create_subkey(r"Software\Inkdown\PreviousAssociations")?.0;

    for ext in EXTENSIONS {
        caps_assoc.set_value(ext, &PROG_ID)?;

        let k = classes.create_subkey(ext)?.0;
        let prev: String = k.get_value("").unwrap_or_default();
        if prev != PROG_ID && backup.get_value::<String, _>(*ext).is_err() {
            backup.set_value(ext, &prev)?;
        }
        k.set_value("", &PROG_ID)?;
        k.set_value("Content Type", &"text/markdown")?;
        k.set_value("PerceivedType", &"text")?;
        k.create_subkey("OpenWithProgids")?.0.set_value(PROG_ID, &"")?;
    }

    // App Paths: lets "inkdown" resolve from Win+R / ShellExecute
    hkcu.create_subkey(format!(
        r"Software\Microsoft\Windows\CurrentVersion\App Paths\{EXE_NAME}"
    ))?
    .0
    .set_value("", &exe_s)?;

    // Uninstall entry
    let un = hkcu.create_subkey(UNINSTALL_KEY)?.0;
    un.set_value("DisplayName", &APP_NAME)?;
    un.set_value("DisplayVersion", &env!("CARGO_PKG_VERSION"))?;
    un.set_value("Publisher", &APP_NAME)?;
    un.set_value("DisplayIcon", &exe_s)?;
    un.set_value(
        "InstallLocation",
        &exe.parent().unwrap_or(exe).to_string_lossy().to_string(),
    )?;
    un.set_value("UninstallString", &format!("\"{exe_s}\" --uninstall"))?;
    un.set_value("QuietUninstallString", &format!("\"{exe_s}\" --uninstall --quiet"))?;
    un.set_value("NoModify", &1u32)?;
    un.set_value("NoRepair", &1u32)?;
    let size_kb = std::fs::metadata(exe).map(|m| m.len() / 1024).unwrap_or(0) as u32;
    un.set_value("EstimatedSize", &size_kb)?;

    Ok(())
}

/// Install for the current user. Returns a human-readable summary.
pub fn install() -> Result<String, String> {
    let src = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = install_dir();
    let dst = dir.join(EXE_NAME);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    if !same_file(&src, &dst) {
        std::fs::copy(&src, &dst).map_err(|e| {
            format!(
                "Cannot copy to {}: {e}\nClose any running Inkdown windows and try again.",
                dst.display()
            )
        })?;
    }
    register(&dst).map_err(|e| format!("Registry update failed: {e}"))?;
    let lnk = shortcut_path();
    let shortcut = create_shortcut(&dst, &lnk);
    notify_shell();

    let mut msg = format!(
        "Inkdown is installed.\n\nLocation: {}\nFile types: {}",
        dst.display(),
        EXTENSIONS.join("  ")
    );
    if let Err(e) = shortcut {
        msg.push_str(&format!("\n\nStart Menu shortcut could not be created: {e}"));
    }
    if !is_default_for_md() {
        msg.push_str(
            "\n\nOne step is left, and Windows only accepts it from you: another app is \
             still set as the default for .md. In Settings > Apps > Default apps > Inkdown, \
             choose Inkdown for .md (and any other Markdown type you want).",
        );
    }
    Ok(msg)
}

pub fn open_default_apps_settings() {
    let _ = std::process::Command::new("cmd")
        .args(["/c", "start", "", "ms-settings:defaultapps?registeredAppUser=Inkdown"])
        .spawn();
}

pub fn uninstall() -> Result<String, String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(classes) = hkcu.open_subkey_with_flags(r"Software\Classes", KEY_ALL_ACCESS) {
        let backup = hkcu.open_subkey(r"Software\Inkdown\PreviousAssociations").ok();
        for ext in EXTENSIONS {
            if let Ok(k) = classes.open_subkey_with_flags(ext, KEY_ALL_ACCESS) {
                if k.get_value::<String, _>("").map(|v| v == PROG_ID).unwrap_or(false) {
                    let prev: String = backup
                        .as_ref()
                        .and_then(|b| b.get_value(*ext).ok())
                        .unwrap_or_default();
                    if prev.is_empty() {
                        let _ = k.delete_value("");
                    } else {
                        let _ = k.set_value("", &prev);
                    }
                }
                if let Ok(o) = k.open_subkey_with_flags("OpenWithProgids", KEY_ALL_ACCESS) {
                    let _ = o.delete_value(PROG_ID);
                }
            }
            if let Ok(fe) = hkcu.open_subkey_with_flags(format!(r"{FILE_EXTS}\{ext}"), KEY_ALL_ACCESS)
            {
                for sub in ["UserChoice", "UserChoiceLatest"] {
                    let ours = fe
                        .open_subkey(sub)
                        .and_then(|u| u.get_value::<String, _>("ProgId"))
                        .map(|p| p == PROG_ID)
                        .unwrap_or(false);
                    if ours {
                        let _ = fe.delete_subkey_all(sub);
                    }
                }
            }
        }
        let _ = classes.delete_subkey_all(PROG_ID);
        let _ = classes.delete_subkey_all(r"*\shell\Inkdown");
        let _ = classes.delete_subkey_all(format!(r"Applications\{EXE_NAME}"));
    }
    let _ = hkcu.delete_subkey_all(r"Software\Inkdown");
    if let Ok(k) = hkcu.open_subkey_with_flags(r"Software\RegisteredApplications", KEY_ALL_ACCESS) {
        let _ = k.delete_value(APP_NAME);
    }
    let _ = hkcu.delete_subkey_all(format!(
        r"Software\Microsoft\Windows\CurrentVersion\App Paths\{EXE_NAME}"
    ));
    let _ = hkcu.delete_subkey_all(UNINSTALL_KEY);
    let _ = std::fs::remove_file(shortcut_path());
    notify_shell();

    // The running exe cannot delete itself; hand the folders to a detached
    // cmd that waits for this process to exit first.
    let dirs = [install_dir(), data_dir(), config_dir()];
    let mut script = String::from("ping -n 3 127.0.0.1 >nul");
    for d in dirs.iter().filter(|d| d.ends_with(APP_NAME) && d.exists()) {
        script.push_str(&format!(" & rmdir /s /q \"{}\"", d.display()));
    }
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    let _ = std::process::Command::new("cmd")
        .raw_arg(format!("/c {script}"))
        .current_dir(env_dir("TEMP"))
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .spawn();

    Ok("Inkdown has been removed.".to_string())
}
