//! Discord sign-in through the default browser: PKCE, a one-shot loopback listener that catches the
//! redirect, and the refresh token kept on disk encrypted with DPAPI (readable by this Windows user only).

use crate::supabase::Error;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};
use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use std::{fs, thread};
use uuid::Uuid;
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};
use windows::core::PCWSTR;

const PORT: u16 = 53117;
/// Must be listed under Supabase → Authentication → URL Configuration → Redirect URLs.
pub const REDIRECT: &str = "http://127.0.0.1:53117/callback";
const TIMEOUT: Duration = Duration::from_secs(5 * 60);

pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn new() -> Self {
        // Two random v4 UUIDs: 64 hex characters with 244 random bits (RFC 7636 wants 43 to 128).
        let verifier = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        Pkce { challenge: challenge(&verifier), verifier }
    }
}

/// S256 code challenge: base64url(sha256(verifier)) without padding.
pub fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Listens on the redirect port. Bound before the browser opens, so a busy port is reported at once.
pub struct Loopback(TcpListener);

impl Loopback {
    pub fn bind() -> Result<Self, Error> {
        let listener = TcpListener::bind(("127.0.0.1", PORT))
            .and_then(|l| l.set_nonblocking(true).map(|()| l))
            .map_err(|e| Error::Local(format!("Giriş için {PORT} numaralı port açılamadı ({e}). Başka bir uygulama kullanıyor olabilir.")))?;
        Ok(Loopback(listener))
    }

    /// Waits for the browser to come back with the sign-in code. Blocking; gives up after five
    /// minutes or as soon as `cancel` is set.
    pub fn wait_for_code(self, cancel: &AtomicBool) -> Result<String, Error> {
        let deadline = Instant::now() + TIMEOUT;
        while !cancel.load(Ordering::Relaxed) {
            if Instant::now() >= deadline {
                return Err(Error::Local("Giriş beş dakika içinde tamamlanmadı.".into()));
            }
            match self.0.accept() {
                Ok((stream, _)) => {
                    if let Some(result) = answer(stream) {
                        return result;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(100)),
                Err(e) => return Err(Error::Local(format!("Giriş dönüşü alınamadı: {e}"))),
            }
        }
        Err(Error::Cancelled)
    }
}

/// Handles one browser request. `None` if it was not the callback (favicon, an empty pre-connect),
/// so the caller keeps waiting.
fn answer(stream: TcpStream) -> Option<Result<String, Error>> {
    stream.set_nonblocking(false).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line).ok()?;
    let result = parse_callback(&line);
    let (status, message) = match &result {
        Some(Ok(_)) => ("200 OK", "Giriş tamamlandı. Bu sekmeyi kapatıp Odak'a dönebilirsin."),
        Some(Err(_)) => ("200 OK", "Giriş tamamlanamadı. Ayrıntı Odak'ta."),
        None => ("404 Not Found", "Bulunamadı"),
    };
    let body = format!(
        "<!doctype html><meta charset=utf-8><title>Odak</title><body style=\"margin:0;height:100vh;display:grid;\
         place-items:center;background:#15161a;color:#e8e8ec;font:16px system-ui,sans-serif\"><p>{message}</p>"
    );
    let reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = (&stream).write_all(reply.as_bytes());
    result
}

/// Reads the request line of the redirect (`GET /callback?code=… HTTP/1.1`). `None` for other paths.
pub fn parse_callback(request_line: &str) -> Option<Result<String, Error>> {
    let target = request_line.strip_prefix("GET ")?.split(' ').next()?;
    let url = url::Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    if url.path() != "/callback" {
        return None;
    }
    let value = |key: &str| url.query_pairs().find(|(k, _)| k == key).map(|(_, v)| v.into_owned());
    if let Some(code) = value("code").filter(|c| !c.is_empty()) {
        return Some(Ok(code));
    }
    let reason = value("error_description").or_else(|| value("error")).unwrap_or_else(|| "yanıtta kod yok".into());
    Some(Err(Error::Local(format!("Discord girişi tamamlanamadı: {reason}"))))
}

fn session_path() -> PathBuf {
    crate::store::data_dir().join("session.bin")
}

pub fn save_refresh_token(token: &str) -> io::Result<()> {
    let path = session_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, protect(token.as_bytes()).map_err(io::Error::other)?)
}

/// `None` if there is no saved session or it cannot be decrypted (another user, another machine).
pub fn load_refresh_token() -> Option<String> {
    let bytes = fs::read(session_path()).ok()?;
    unprotect(&bytes).map_err(|e| log::warn!("session file unreadable: {e}")).ok().and_then(|b| String::from_utf8(b).ok())
}

pub fn forget_refresh_token() {
    if let Err(e) = fs::remove_file(session_path())
        && e.kind() != io::ErrorKind::NotFound
    {
        log::warn!("could not delete the session file: {e}");
    }
}

fn protect(plain: &[u8]) -> windows::core::Result<Vec<u8>> {
    let input = CRYPT_INTEGER_BLOB { cbData: plain.len() as u32, pbData: plain.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    unsafe {
        CryptProtectData(&input, PCWSTR::null(), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output)?;
        Ok(take(output))
    }
}

fn unprotect(sealed: &[u8]) -> windows::core::Result<Vec<u8>> {
    let input = CRYPT_INTEGER_BLOB { cbData: sealed.len() as u32, pbData: sealed.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    unsafe {
        CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output)?;
        Ok(take(output))
    }
}

/// Copies out a buffer that DPAPI allocated and frees it.
unsafe fn take(blob: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    unsafe {
        let bytes = std::slice::from_raw_parts(blob.pbData, blob.cbData as usize).to_vec();
        LocalFree(Some(HLOCAL(blob.pbData.cast())));
        bytes
    }
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
