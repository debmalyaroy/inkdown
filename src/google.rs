//! Google Drive integration: the user signs in with their own Google account
//! (OAuth 2.0 for installed apps: loopback redirect + PKCE) and Inkdown uploads a
//! .docx that Drive converts into a native Google Doc.
//!
//! Scope is `drive.file`: Inkdown can only see files it created itself.
//! The refresh token is kept in the settings folder, encrypted with the Windows
//! user's key (DPAPI) on Windows.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const SCOPE: &str = "https://www.googleapis.com/auth/drive.file";
const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const UPLOAD_URL: &str = "https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart&fields=id,name,webViewLink";

fn store_path() -> PathBuf {
    crate::install::config_dir().join("integrations.dat")
}

#[cfg(windows)]
fn seal(data: &[u8], protect: bool) -> Option<Vec<u8>> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB};
    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        let done = if protect {
            CryptProtectData(&input, None, None, None, None, 0, &mut out)
        } else {
            CryptUnprotectData(&input, None, None, None, None, 0, &mut out)
        };
        done.ok()?;
        let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData as _)));
        Some(bytes)
    }
}
#[cfg(not(windows))]
fn seal(data: &[u8], _protect: bool) -> Option<Vec<u8>> {
    Some(data.to_vec()) // other platforms: use the system keychain when those ports are packaged
}

fn load() -> Value {
    std::fs::read(store_path())
        .ok()
        .and_then(|b| seal(&b, false))
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| json!({}))
}
fn store(v: &Value) -> Result<(), String> {
    let path = store_path();
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let sealed = seal(&serde_json::to_vec(v).unwrap_or_default(), true).ok_or("Could not protect the sign-in data.")?;
    std::fs::write(path, sealed).map_err(|e| e.to_string())
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().http_status_as_error(false).timeout_global(Some(Duration::from_secs(90))).build().into()
}
fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}
fn random(n: usize) -> Result<Vec<u8>, String> {
    let mut v = vec![0u8; n];
    getrandom::fill(&mut v).map_err(|e| e.to_string())?;
    Ok(v)
}
fn enc(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}
fn api_error(v: &Value, fallback: &str) -> String {
    v["error_description"].as_str().or(v["error"]["message"].as_str()).or(v["error"].as_str()).unwrap_or(fallback).to_string()
}

pub fn status() -> Value {
    let s = load();
    json!({
        "connected": s["refresh_token"].is_string(),
        "clientId": s["client_id"].as_str().unwrap_or(""),
        "hasSecret": s["client_secret"].as_str().is_some_and(|x| !x.is_empty()),
    })
}

pub fn disconnect() -> Result<(), String> {
    let mut s = load();
    if let Some(token) = s["refresh_token"].as_str() {
        let _ = agent().post("https://oauth2.googleapis.com/revoke").send_form([("token", token)]);
    }
    if let Some(o) = s.as_object_mut() {
        o.remove("refresh_token");
    }
    store(&s)
}

pub fn auth_url(client_id: &str, redirect: &str, challenge: &str, state: &str) -> String {
    format!(
        "{AUTH_URL}?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={challenge}&code_challenge_method=S256&state={state}&access_type=offline&prompt=consent",
        enc(client_id),
        enc(redirect),
        enc(SCOPE)
    )
}

/// Runs the browser sign-in. Blocks (up to three minutes) until Google redirects back.
pub fn connect(client_id: &str, client_secret: &str) -> Result<Value, String> {
    let client_id = client_id.trim();
    if !client_id.ends_with(".apps.googleusercontent.com") {
        return Err("That does not look like a Google OAuth client ID (it ends with .apps.googleusercontent.com).".into());
    }
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let redirect = format!("http://127.0.0.1:{}", listener.local_addr().map_err(|e| e.to_string())?.port());
    let verifier = b64url(&random(48)?);
    let challenge = b64url(&Sha256::digest(verifier.as_bytes()));
    let state = b64url(&random(16)?);
    crate::shell_open(&auth_url(client_id, &redirect, &challenge, &state));

    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(180);
    let code = loop {
        if Instant::now() > deadline {
            return Err("Sign-in timed out. Try again when the browser window is open.".into());
        }
        let Ok((mut stream, _)) = listener.accept() else {
            std::thread::sleep(Duration::from_millis(120));
            continue;
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buf = [0u8; 4096];
        let n = stream.read(&mut buf).unwrap_or(0);
        let request = String::from_utf8_lossy(&buf[..n]);
        let query = request.split_whitespace().nth(1).and_then(|p| p.split_once('?')).map(|(_, q)| q.to_string()).unwrap_or_default();
        let get = |k: &str| {
            query.split('&').find_map(|kv| kv.split_once('=').filter(|(key, _)| *key == k).map(|(_, v)| percent_encoding::percent_decode_str(v).decode_utf8_lossy().to_string()))
        };
        let (code, err, st) = (get("code"), get("error"), get("state"));
        if code.is_none() && err.is_none() {
            let _ = stream.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
            continue; // favicon and similar
        }
        let ok = code.is_some() && st.as_deref() == Some(state.as_str());
        let page = format!(
            "<!doctype html><meta charset=utf-8><title>Inkdown</title><body style=\"font:16px Segoe UI,sans-serif;padding:48px;text-align:center\"><h2>{}</h2><p>You can close this tab and return to Inkdown.</p>",
            if ok { "Google account connected" } else { "Sign-in was not completed" }
        );
        let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}", page.len());
        if let Some(e) = err {
            return Err(format!("Google sign-in was cancelled ({e})."));
        }
        if !ok {
            return Err("Sign-in response did not match this request.".into());
        }
        break code.unwrap_or_default();
    };

    let mut resp = agent()
        .post(TOKEN_URL)
        .send_form([
            ("client_id", client_id),
            ("client_secret", client_secret.trim()),
            ("code", code.as_str()),
            ("code_verifier", verifier.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect.as_str()),
        ])
        .map_err(|e| format!("Could not reach Google: {e}"))?;
    let body: Value = resp.body_mut().read_json().map_err(|e| e.to_string())?;
    let Some(refresh) = body["refresh_token"].as_str() else {
        return Err(format!("Google did not grant access: {}", api_error(&body, "no refresh token returned")));
    };
    store(&json!({ "client_id": client_id, "client_secret": client_secret.trim(), "refresh_token": refresh }))?;
    Ok(status())
}

fn access_token() -> Result<String, String> {
    let s = load();
    let (Some(id), Some(refresh)) = (s["client_id"].as_str(), s["refresh_token"].as_str()) else {
        return Err("Connect a Google account first (Menu > Integrations).".into());
    };
    let mut resp = agent()
        .post(TOKEN_URL)
        .send_form([("client_id", id), ("client_secret", s["client_secret"].as_str().unwrap_or("")), ("refresh_token", refresh), ("grant_type", "refresh_token")])
        .map_err(|e| format!("Could not reach Google: {e}"))?;
    let body: Value = resp.body_mut().read_json().map_err(|e| e.to_string())?;
    body["access_token"].as_str().map(str::to_string).ok_or_else(|| format!("Google sign-in has expired: {}. Reconnect under Integrations.", api_error(&body, "token refresh failed")))
}

/// multipart/related body: JSON metadata + the .docx bytes.
pub fn multipart(boundary: &str, title: &str, docx: &[u8]) -> Vec<u8> {
    let meta = json!({ "name": title, "mimeType": "application/vnd.google-apps.document" });
    let mut body = Vec::with_capacity(docx.len() + 512);
    let _ = write!(body, "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{meta}\r\n--{boundary}\r\nContent-Type: application/vnd.openxmlformats-officedocument.wordprocessingml.document\r\n\r\n");
    body.extend_from_slice(docx);
    let _ = write!(body, "\r\n--{boundary}--\r\n");
    body
}

/// Upload a .docx and let Drive convert it. Returns `{ id, name, webViewLink }`.
pub fn upload_docx(docx: &[u8], title: &str) -> Result<Value, String> {
    let token = access_token()?;
    let boundary = format!("inkdown{}", b64url(&random(12)?));
    let mut resp = agent()
        .post(UPLOAD_URL)
        .header("Authorization", format!("Bearer {token}"))
        .content_type(format!("multipart/related; boundary={boundary}"))
        .send(multipart(&boundary, title, docx))
        .map_err(|e| format!("Upload failed: {e}"))?;
    let body: Value = resp.body_mut().read_json().map_err(|e| e.to_string())?;
    if body["webViewLink"].is_string() {
        Ok(body)
    } else {
        Err(format!("Google Drive refused the upload: {}", api_error(&body, "unknown error")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_requests() {
        let url = auth_url("123-abc.apps.googleusercontent.com", "http://127.0.0.1:5000", "CHAL", "ST");
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?client_id=123%2Dabc%2Eapps%2Egoogleusercontent%2Ecom"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127%2E0%2E0%2E1%3A5000") && url.contains("code_challenge_method=S256") && url.contains("drive%2Efile"));
        let body = multipart("B", "My \"doc\"", b"PK\x03\x04");
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with("--B\r\nContent-Type: application/json"));
        assert!(text.contains(r#""name":"My \"doc\"""#) && text.contains("application/vnd.google-apps.document") && text.ends_with("\r\n--B--\r\n"));
        assert!(connect("not-a-client-id", "").is_err());
        let sealed = seal(b"secret", true).unwrap();
        assert_eq!(seal(&sealed, false).unwrap(), b"secret");
    }
}
