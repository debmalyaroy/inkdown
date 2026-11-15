//! One running Inkdown per user.
//!
//! Opening a file from the file manager starts a new process. If Inkdown is already
//! running, that process hands the file to the running window and exits, instead of
//! becoming a second window with its own idea of the session (two windows saving the same
//! session file was how unsaved drafts used to get lost).
//!
//! The running copy listens on a loopback port (127.0.0.1 only) and notes the port in
//! `instance.json` beside the settings. "New Window" still opens a separate window.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

const HELLO: &str = "inkdown-open 1";

fn lock_path() -> PathBuf {
    crate::settings::config_dir().join("instance.json")
}

/// False in test runs and in windows opened with "New Window".
fn enabled() -> bool {
    if std::env::var_os("INKDOWN_SINGLE_INSTANCE").is_some() {
        return true;
    }
    std::env::var_os("INKDOWN_AUTOMATION").is_none() && std::env::var_os("INKDOWN_NO_SESSION").is_none()
}

/// Hands `files` to a running Inkdown. Returns true when it took them (this process
/// should then exit).
pub fn forward(files: &[PathBuf]) -> bool {
    if !enabled() {
        return false;
    }
    let Some(port) = std::fs::read_to_string(lock_path()).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).and_then(|v| v.get("port").and_then(|p| p.as_u64())) else { return false };
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port as u16));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(400)) else { return false };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(1500)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(1500)));
    let mut msg = format!("{HELLO}\n");
    for f in files {
        // the other process has a different working folder
        if crate::live::is_url(&f.to_string_lossy()) {
            msg.push_str(&f.to_string_lossy());
            msg.push('\n');
            continue;
        }
        let full = std::fs::canonicalize(f).unwrap_or_else(|_| std::env::current_dir().map(|d| d.join(f)).unwrap_or_else(|_| f.clone()));
        msg.push_str(&full.to_string_lossy());
        msg.push('\n');
    }
    msg.push('\n');
    if stream.write_all(msg.as_bytes()).is_err() {
        return false;
    }
    let mut reply = String::new();
    // only a real Inkdown answers "ok"; anything else (a stale port reused by another
    // program) means: start normally
    BufReader::new(stream).read_line(&mut reply).is_ok() && reply.trim() == "ok"
}

/// Starts listening for files from later launches. `wake` is called when some arrive.
pub fn listen(wake: impl Fn() + Send + 'static) -> Option<Receiver<Vec<PathBuf>>> {
    if !enabled() {
        return None;
    }
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).ok()?;
    let port = listener.local_addr().ok()?.port();
    let _ = std::fs::create_dir_all(crate::settings::config_dir());
    std::fs::write(lock_path(), format!("{{\"pid\": {}, \"port\": {port}}}\n", std::process::id())).ok()?;
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("ink-instance".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let _ = stream.set_read_timeout(Some(Duration::from_millis(1500)));
                let mut reader = BufReader::new(&stream);
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() || line.trim() != HELLO {
                    continue;
                }
                let mut files = Vec::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(n) if n > 0 && !line.trim().is_empty() && files.len() < 200 => files.push(PathBuf::from(line.trim_end_matches(['\r', '\n']))),
                        _ => break,
                    }
                }
                let mut out = &stream;
                let _ = out.write_all(b"ok\n");
                if tx.send(files).is_err() {
                    break;
                }
                wake();
            }
        })
        .ok()?;
    Some(rx)
}

/// Forgets the port on a normal exit. A stale note is harmless: nothing answers there.
pub fn release() {
    if enabled() {
        let _ = std::fs::remove_file(lock_path());
    }
}
