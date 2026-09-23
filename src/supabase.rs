//! Thin Supabase client (Auth and REST) on blocking `ureq` calls: run them off the main thread.

use crate::model::Profile;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::fmt;
use std::sync::LazyLock;
use std::time::Duration;
use ureq::http::Response;
use ureq::{Agent, Body, RequestBuilder};
use uuid::Uuid;

/// Baked in at build time from `.env` (see build.rs).
const URL: Option<&str> = option_env!("SUPABASE_URL");
const KEY: Option<&str> = option_env!("SUPABASE_PUBLISHABLE_KEY");

/// False in builds made without the Supabase settings: account features are then off.
pub fn enabled() -> bool {
    URL.is_some() && KEY.is_some()
}

fn endpoint(path: &str) -> String {
    format!("{}{path}", URL.unwrap_or_default())
}

static AGENT: LazyLock<Agent> = LazyLock::new(|| {
    Agent::new_with_config(
        Agent::config_builder().http_status_as_error(false).timeout_global(Some(Duration::from_secs(20))).build(),
    )
});

#[derive(Debug)]
pub enum Error {
    /// No answer: offline, DNS, TLS or timeout.
    Network(String),
    /// The server answered with an error status.
    Status(u16, String),
    /// A local problem (busy port, unexpected reply, file access).
    Local(String),
    /// The user cancelled.
    Cancelled,
}

impl Error {
    /// The server no longer accepts the session (bad or revoked refresh token).
    pub fn is_auth_rejected(&self) -> bool {
        matches!(self, Error::Status(400 | 401 | 403, _))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Network(e) => write!(f, "Sunucuya ulaşılamadı ({e})"),
            Error::Status(code, message) => write!(f, "Sunucu hatası {code}: {message}"),
            Error::Local(message) => f.write_str(message),
            Error::Cancelled => f.write_str("İptal edildi"),
        }
    }
}

/// A signed-in session. The access token is short-lived; the refresh token rotates on every use.
#[derive(Clone, Debug)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: DateTime<Utc>,
    pub user_id: Uuid,
}

impl Session {
    /// A session restored from disk: only the refresh token is known, so it must be refreshed first.
    pub fn stale(user_id: Uuid, refresh_token: String) -> Self {
        Session { access_token: String::new(), refresh_token, expires_at: DateTime::UNIX_EPOCH, user_id }
    }

    /// The access token is missing or has less than a minute left.
    pub fn needs_refresh(&self, now: DateTime<Utc>) -> bool {
        self.expires_at - now < chrono::Duration::seconds(60)
    }
}

#[derive(Deserialize)]
struct TokenReply {
    access_token: String,
    refresh_token: String,
    expires_in: i64,
    user: UserRef,
}

#[derive(Deserialize)]
struct UserRef {
    id: Uuid,
}

/// Browser address that starts Discord sign-in and comes back to `redirect` with `?code=`.
pub fn authorize_url(challenge: &str, redirect: &str) -> String {
    let params = [
        ("provider", "discord"),
        ("redirect_to", redirect),
        ("code_challenge", challenge),
        ("code_challenge_method", "s256"),
    ];
    url::Url::parse_with_params(&endpoint("/auth/v1/authorize"), params).map(String::from).unwrap_or_default()
}

/// Trades the code from the sign-in redirect for a session (PKCE).
pub fn exchange_code(code: &str, verifier: &str) -> Result<Session, Error> {
    token("pkce", json!({ "auth_code": code, "code_verifier": verifier }))
}

pub fn refresh(refresh_token: &str) -> Result<Session, Error> {
    token("refresh_token", json!({ "refresh_token": refresh_token }))
}

fn token(grant: &str, body: Value) -> Result<Session, Error> {
    let request = AGENT.post(endpoint("/auth/v1/token")).query("grant_type", grant).header("apikey", KEY.unwrap_or_default());
    let reply: TokenReply = read_json(request.send_json(body))?;
    Ok(Session {
        access_token: reply.access_token,
        refresh_token: reply.refresh_token,
        expires_at: Utc::now() + chrono::Duration::seconds(reply.expires_in),
        user_id: reply.user.id,
    })
}

/// Ends this device's session on the server (other devices stay signed in).
pub fn sign_out(session: &Session) -> Result<(), Error> {
    let request = authorized(AGENT.post(endpoint("/auth/v1/logout")), session).query("scope", "local");
    check(request.send_empty()).map(drop)
}

/// The signed-in user's row in `profiles`.
pub fn own_profile(session: &Session) -> Result<Profile, Error> {
    let request = authorized(AGENT.get(endpoint("/rest/v1/profiles")), session)
        .query("select", "id,username,display_name,avatar_url")
        .query("id", format!("eq.{}", session.user_id));
    let rows: Vec<Profile> = read_json(request.call())?;
    rows.into_iter().next().ok_or_else(|| Error::Local("Profil bulunamadı".into()))
}

/// Downloads a file from anywhere (avatars).
pub fn download(url: &str) -> Result<Vec<u8>, Error> {
    check(AGENT.get(url).call())?.body_mut().read_to_vec().map_err(|e| Error::Network(e.to_string()))
}

fn authorized<B>(request: RequestBuilder<B>, session: &Session) -> RequestBuilder<B> {
    request.header("apikey", KEY.unwrap_or_default()).header("Authorization", format!("Bearer {}", session.access_token))
}

fn check(result: Result<Response<Body>, ureq::Error>) -> Result<Response<Body>, Error> {
    let mut response = result.map_err(|e| Error::Network(e.to_string()))?;
    let status = response.status().as_u16();
    if status >= 400 {
        let body = response.body_mut().read_to_string().unwrap_or_default();
        return Err(Error::Status(status, error_message(&body)));
    }
    Ok(response)
}

fn read_json<T: DeserializeOwned>(result: Result<Response<Body>, ureq::Error>) -> Result<T, Error> {
    check(result)?.body_mut().read_json().map_err(|e| Error::Local(format!("Beklenmeyen sunucu yanıtı: {e}")))
}

/// The human-readable part of an Auth or PostgREST error body.
fn error_message(body: &str) -> String {
    let json: Value = serde_json::from_str(body).unwrap_or_default();
    ["msg", "message", "error_description", "error"]
        .iter()
        .find_map(|key| json.get(key).and_then(Value::as_str))
        .map(str::to_owned)
        .unwrap_or_else(|| body.chars().take(200).collect())
}
