//! Update check and, on Windows, in-place update.
//!
//! Releases are published on GitHub. The check reads the "latest release" record, compares
//! its version with this build, and offers the installer. Nothing about the user or their
//! documents is sent: the request is a plain GET with a generic client name.
//!
//! `INKDOWN_UPDATE_URL` replaces the address of the release record (used by the tests).

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, TryRecvError};

/// `owner/name` of the repository that publishes releases.
pub const REPOSITORY: &str = "debmalyaroy/inkdown";
pub const INSTALLER_ASSET: &str = "Inkdown-Setup.exe";
pub const CHECKSUMS_ASSET: &str = "SHA256SUMS.txt";
/// The automatic check runs at most this often.
const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn project_url() -> String {
    format!("https://github.com/{REPOSITORY}")
}
pub fn releases_url() -> String {
    format!("{}/releases/latest", project_url())
}
pub fn docs_url() -> String {
    format!("{}#readme", project_url())
}
pub fn issues_url() -> String {
    format!("{}/issues", project_url())
}

fn latest_release_api() -> String {
    std::env::var("INKDOWN_UPDATE_URL").ok().filter(|u| !u.is_empty()).unwrap_or_else(|| format!("https://api.github.com/repos/{REPOSITORY}/releases/latest"))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    /// Version without the leading `v`.
    pub version: String,
    /// Page describing the release.
    pub page: String,
    pub installer: Option<String>,
    pub checksums: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading(Release),
    /// The verified installer is on disk.
    Ready(Release, PathBuf),
    Failed(String),
}

enum Msg {
    Checked(Result<Release, String>),
    Downloaded(Result<PathBuf, String>),
}

pub struct Updater {
    pub state: State,
    rx: Option<Receiver<Msg>>,
    /// The user asked (show the result even when there is nothing new).
    manual: bool,
    auto_done: bool,
}

impl Default for Updater {
    fn default() -> Self {
        Self { state: State::Idle, rx: None, manual: false, auto_done: false }
    }
}

/// `1.2.3` or `v1.2.3` (anything after the numbers, such as `-beta`, is ignored).
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let core = s.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let major = parts.next()??;
    let minor = parts.next().unwrap_or(Some(0))?;
    let patch = parts.next().unwrap_or(Some(0))?;
    Some((major, minor, patch))
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    matches!((parse_version(candidate), parse_version(current)), (Some(a), Some(b)) if a > b)
}

/// Reads the GitHub "release" record.
pub fn parse_release(json: &str) -> Result<Release, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|_| "The update server sent an unexpected answer.".to_string())?;
    let tag = v.get("tag_name").and_then(|t| t.as_str()).ok_or_else(|| v.get("message").and_then(|m| m.as_str()).map(|m| format!("The update server answered: {m}")).unwrap_or_else(|| "No release has been published yet.".into()))?;
    parse_version(tag).ok_or_else(|| format!("The latest release has an unreadable version ({tag})."))?;
    let asset = |name: &str| {
        v.get("assets").and_then(|a| a.as_array()).and_then(|a| a.iter().find(|x| x.get("name").and_then(|n| n.as_str()) == Some(name))).and_then(|x| x.get("browser_download_url")).and_then(|u| u.as_str()).map(str::to_string)
    };
    Ok(Release { version: tag.trim_start_matches(['v', 'V']).to_string(), page: v.get("html_url").and_then(|u| u.as_str()).map(str::to_string).unwrap_or_else(releases_url), installer: asset(INSTALLER_ASSET), checksums: asset(CHECKSUMS_ASSET) })
}

/// The checksum listed for `file` in a `sha256sum` style list.
pub fn checksum_for(list: &str, file: &str) -> Option<String> {
    list.lines().find_map(|line| {
        let (hash, name) = line.split_once(char::is_whitespace)?;
        (name.trim().trim_start_matches('*') == file && hash.len() == 64).then(|| hash.to_lowercase())
    })
}

/// True when this process is the installed copy, which the installer can replace.
pub fn can_self_update() -> bool {
    #[cfg(windows)]
    {
        let installed = crate::winstall::install_dir().join(crate::winstall::EXE_NAME);
        match (std::env::current_exe().ok().and_then(|p| p.canonicalize().ok()), installed.canonicalize().ok()) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        }
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn stamp_path() -> PathBuf {
    crate::settings::config_dir().join("update-check.json")
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn checked_recently() -> bool {
    let last = std::fs::read_to_string(stamp_path()).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).and_then(|v| v.get("last_check").and_then(|x| x.as_u64())).unwrap_or(0);
    now_secs().saturating_sub(last) < CHECK_INTERVAL_SECS
}

fn remember_check() {
    let path = stamp_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, format!("{{\"last_check\": {}}}\n", now_secs()));
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod net {
    use super::*;
    use std::io::Read;

    fn agent(secs: u64) -> ureq::Agent {
        ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(secs))).build().into()
    }

    fn get(url: &str, secs: u64, limit: u64) -> Result<Vec<u8>, String> {
        let mut resp = agent(secs).get(url).header("User-Agent", "Inkdown-updater").header("Accept", "application/vnd.github+json, */*").call().map_err(|e| match e {
            ureq::Error::StatusCode(404) => "No release has been published yet.".to_string(),
            ureq::Error::StatusCode(403 | 429) => "The update server is busy. Try again in a few minutes.".to_string(),
            other => format!("Could not reach the update server ({other})."),
        })?;
        let mut buf = Vec::new();
        resp.body_mut().as_reader().take(limit).read_to_end(&mut buf).map_err(|e| format!("The download was interrupted ({e})."))?;
        Ok(buf)
    }

    pub fn latest() -> Result<Release, String> {
        let body = get(&latest_release_api(), 20, 4 << 20)?;
        parse_release(&String::from_utf8_lossy(&body))
    }

    /// Downloads the installer and checks it against the published checksum.
    pub fn download(release: &Release) -> Result<PathBuf, String> {
        use sha2::{Digest, Sha256};
        let url = release.installer.as_deref().ok_or("This release has no Windows installer.")?;
        let sums_url = release.checksums.as_deref().ok_or("This release has no checksum list, so the download cannot be verified.")?;
        let sums = String::from_utf8_lossy(&get(sums_url, 30, 1 << 20)?).into_owned();
        let expected = checksum_for(&sums, INSTALLER_ASSET).ok_or("The checksum list does not mention the installer.")?;
        let bytes = get(url, 600, 400 << 20)?;
        let actual: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        if actual != expected {
            return Err("The downloaded installer does not match its published checksum. It was not run.".into());
        }
        let dir = std::env::temp_dir().join("inkdown-update");
        std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot write to the temporary folder ({e})."))?;
        let path = dir.join(INSTALLER_ASSET);
        std::fs::write(&path, &bytes).map_err(|e| format!("Cannot save the installer ({e})."))?;
        Ok(path)
    }
}

impl Updater {
    pub fn busy(&self) -> bool {
        matches!(self.state, State::Checking | State::Downloading(_))
    }

    /// A short word for tests and the status bar.
    pub fn label(&self) -> &'static str {
        match self.state {
            State::Idle => "idle",
            State::Checking => "checking",
            State::UpToDate => "up-to-date",
            State::Available(_) => "available",
            State::Downloading(_) => "downloading",
            State::Ready(..) => "ready",
            State::Failed(_) => "failed",
        }
    }

    pub fn available(&self) -> Option<&Release> {
        match &self.state {
            State::Available(r) | State::Downloading(r) | State::Ready(r, _) => Some(r),
            _ => None,
        }
    }

    /// Starts a check. `manual` is true when the user asked for it.
    pub fn check(&mut self, ctx: &egui::Context, manual: bool) {
        if self.busy() {
            self.manual |= manual;
            return;
        }
        self.manual = manual;
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            self.state = State::Checking;
            let (tx, rx) = channel();
            self.rx = Some(rx);
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Msg::Checked(net::latest()));
                ctx.request_repaint();
            });
        }
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            let _ = ctx;
            self.state = State::Failed("Updates for this device come from the store or the releases page.".into());
        }
    }

    /// The once-a-day check at start-up. Does nothing in test runs.
    pub fn auto_check(&mut self, ctx: &egui::Context, enabled: bool) {
        if self.auto_done {
            return;
        }
        self.auto_done = true;
        let test_run = std::env::var_os("INKDOWN_AUTOMATION").is_some() && std::env::var_os("INKDOWN_UPDATE_URL").is_none();
        // a check the user already asked for must not be replaced by the quiet one
        if self.state != State::Idle || !enabled || test_run || cfg!(any(target_os = "android", target_os = "ios")) || checked_recently() {
            return;
        }
        self.check(ctx, false);
    }

    /// Downloads the installer for the available release.
    pub fn download(&mut self, ctx: &egui::Context) {
        let State::Available(release) = self.state.clone() else { return };
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            self.state = State::Downloading(release.clone());
            let (tx, rx) = channel();
            self.rx = Some(rx);
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Msg::Downloaded(net::download(&release)));
                ctx.request_repaint();
            });
        }
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            let _ = (ctx, release);
        }
    }

    /// Collects finished background work. Returns true when a dialog should be shown.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.rx else { return false };
        let msg = match rx.try_recv() {
            Ok(m) => m,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => {
                self.rx = None;
                if self.busy() {
                    self.state = State::Failed("The update check stopped unexpectedly.".into());
                    return self.manual;
                }
                return false;
            }
        };
        self.rx = None;
        match msg {
            Msg::Checked(Ok(release)) => {
                remember_check();
                if is_newer(&release.version, current_version()) {
                    self.state = State::Available(release);
                    true
                } else {
                    self.state = State::UpToDate;
                    self.manual
                }
            }
            Msg::Checked(Err(e)) => {
                // a failed background check stays quiet; a requested one explains itself
                self.state = if self.manual { State::Failed(e) } else { State::Idle };
                self.manual
            }
            Msg::Downloaded(Ok(path)) => {
                if let State::Downloading(r) = self.state.clone() {
                    self.state = State::Ready(r, path);
                }
                true
            }
            Msg::Downloaded(Err(e)) => {
                self.state = State::Failed(e);
                true
            }
        }
    }

    /// Starts the downloaded installer, which replaces this copy and opens it again.
    /// Returns true when the application should now close.
    pub fn install_and_restart(&mut self) -> bool {
        let State::Ready(_, path) = &self.state else { return false };
        // the reopened copy is an ordinary session, never a scripted test run
        match std::process::Command::new(path).args(["--quiet", "--relaunch"]).env_remove("INKDOWN_AUTOMATION").spawn() {
            Ok(_) => true,
            Err(e) => {
                self.state = State::Failed(format!("Could not start the installer ({e})."));
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("2.0"), Some((2, 0, 0)));
        assert_eq!(parse_version("1.4.0-beta.2"), Some((1, 4, 0)));
        assert_eq!(parse_version("latest"), None);
        assert!(is_newer("v1.0.1", "1.0.0"));
        assert!(is_newer("1.10.0", "1.9.9"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("0.9.9", "1.0.0"));
        assert!(!is_newer("nonsense", "1.0.0"));
    }

    #[test]
    fn reads_a_release_record() {
        let json = r#"{"tag_name":"v1.2.0","html_url":"https://example.org/r/v1.2.0","assets":[
            {"name":"Inkdown-Setup.exe","browser_download_url":"https://example.org/d/Inkdown-Setup.exe"},
            {"name":"SHA256SUMS.txt","browser_download_url":"https://example.org/d/SHA256SUMS.txt"},
            {"name":"inkdown-android.apk","browser_download_url":"https://example.org/d/a.apk"}]}"#;
        let r = parse_release(json).unwrap();
        assert_eq!(r.version, "1.2.0");
        assert_eq!(r.page, "https://example.org/r/v1.2.0");
        assert_eq!(r.installer.as_deref(), Some("https://example.org/d/Inkdown-Setup.exe"));
        assert_eq!(r.checksums.as_deref(), Some("https://example.org/d/SHA256SUMS.txt"));
        assert!(parse_release(r#"{"message":"Not Found"}"#).unwrap_err().contains("Not Found"));
        assert!(parse_release("<html>").is_err());
        assert!(parse_release(r#"{"tag_name":"nightly"}"#).is_err());
        let none = parse_release(r#"{"tag_name":"v2.0.0"}"#).unwrap();
        assert!(none.installer.is_none() && none.page.ends_with("/releases/latest"));
    }

    #[test]
    fn finds_checksums() {
        let h = "a".repeat(64);
        let list = format!("{}  inkdown-android.apk\n{h}  Inkdown-Setup.exe\n{} *other.bin\n", "b".repeat(64), "C".repeat(64));
        assert_eq!(checksum_for(&list, "Inkdown-Setup.exe"), Some(h));
        assert_eq!(checksum_for(&list, "other.bin"), Some("c".repeat(64)));
        assert_eq!(checksum_for(&list, "missing.exe"), None);
        assert_eq!(checksum_for("short  Inkdown-Setup.exe", "Inkdown-Setup.exe"), None);
    }
}
