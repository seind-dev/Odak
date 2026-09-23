//! Thin Supabase client (Auth and REST) on blocking `ureq` calls: run them off the main thread.

use crate::model::{Group, Priority, Profile, Reminder, Status, SubTask, Task};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
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

/// Realtime websocket address (protocol 1.0.0: JSON messages).
pub fn realtime_url() -> String {
    let host = URL.unwrap_or_default().trim_start_matches("https://");
    format!("wss://{host}/realtime/v1/websocket?apikey={}&vsn=1.0.0", KEY.unwrap_or_default())
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

    /// Worth trying again later: no connection, an expired token, rate limiting or a server fault.
    /// Anything else (403, 404, 409, 400) will fail the same way again.
    pub fn is_temporary(&self) -> bool {
        matches!(self, Error::Network(_) | Error::Status(401 | 408 | 429 | 500.., _))
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

/// A row of the `tasks` table. Subtasks and the reminder are stored as the app's own JSON.
#[derive(Serialize, Deserialize)]
struct TaskRow {
    id: Uuid,
    owner_id: Option<Uuid>,
    group_id: Option<Uuid>,
    assignee_id: Option<Uuid>,
    title: String,
    description: String,
    priority: Priority,
    status: Status,
    tags: Vec<String>,
    subtasks: Vec<SubTask>,
    due_date: Option<DateTime<Utc>>,
    reminder: Option<Reminder>,
    order: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<&Task> for TaskRow {
    fn from(t: &Task) -> Self {
        let t = t.clone();
        TaskRow {
            id: t.id,
            owner_id: t.owner_id,
            group_id: t.group_id,
            assignee_id: t.assignee_id,
            title: t.title,
            description: t.description,
            priority: t.priority,
            status: t.status,
            tags: t.tags,
            subtasks: t.subtasks,
            due_date: t.due_date,
            reminder: t.reminder,
            order: t.order,
            created_at: t.created_at,
            updated_at: t.updated_at,
        }
    }
}

impl From<TaskRow> for Task {
    fn from(r: TaskRow) -> Self {
        Task {
            id: r.id,
            title: r.title,
            description: r.description,
            priority: r.priority,
            status: r.status,
            reminder: r.reminder,
            subtasks: r.subtasks,
            tags: r.tags,
            order: r.order,
            due_date: r.due_date,
            created_at: r.created_at,
            updated_at: r.updated_at,
            owner_id: r.owner_id,
            group_id: r.group_id,
            assignee_id: r.assignee_id,
        }
    }
}

/// Last-write-wins upload through the `upsert_task` function: `Ok(false)` means the server
/// already had a newer version and kept it.
pub fn upsert_task(session: &Session, task: &Task) -> Result<bool, Error> {
    let request = authorized(AGENT.post(endpoint("/rest/v1/rpc/upsert_task")), session);
    read_json(request.send_json(json!({ "task": TaskRow::from(task) })))
}

pub fn delete_task(session: &Session, id: Uuid) -> Result<(), Error> {
    let request = authorized(AGENT.delete(endpoint("/rest/v1/tasks")), session).query("id", format!("eq.{id}"));
    check(request.call()).map(drop)
}

/// Every task the user can see (own and group tasks).
/// Note: the API returns at most 1000 rows per request; page with a Range header if that is ever too few.
pub fn fetch_tasks(session: &Session) -> Result<Vec<Task>, Error> {
    let request = authorized(AGENT.get(endpoint("/rest/v1/tasks")), session).query("select", "*");
    let rows: Vec<TaskRow> = read_json(request.call())?;
    Ok(rows.into_iter().map(Task::from).collect())
}

#[derive(Deserialize)]
struct GroupRow {
    id: Uuid,
    name: String,
    owner_id: Uuid,
    #[serde(default)]
    group_members: Vec<MemberRow>,
}

#[derive(Deserialize)]
struct MemberRow {
    user_id: Uuid,
}

impl From<GroupRow> for Group {
    fn from(r: GroupRow) -> Self {
        let members = r.group_members.into_iter().map(|m| m.user_id).collect();
        Group { id: r.id, name: r.name, owner_id: r.owner_id, members }
    }
}

/// The account's groups with their members.
pub fn fetch_groups(session: &Session) -> Result<Vec<Group>, Error> {
    let request = authorized(AGENT.get(endpoint("/rest/v1/groups")), session)
        .query("select", "id,name,owner_id,group_members(user_id)")
        .query("order", "created_at");
    let rows: Vec<GroupRow> = read_json(request.call())?;
    Ok(rows.into_iter().map(Group::from).collect())
}

/// The account and everyone who shares a group with it.
pub fn fetch_profiles(session: &Session) -> Result<Vec<Profile>, Error> {
    let request = authorized(AGENT.get(endpoint("/rest/v1/profiles")), session).query("select", "id,username,display_name,avatar_url");
    read_json(request.call())
}

pub fn create_group(session: &Session, name: &str) -> Result<Group, Error> {
    let request = authorized(AGENT.post(endpoint("/rest/v1/groups")), session)
        .header("Prefer", "return=representation")
        .query("select", "id,name,owner_id");
    let rows: Vec<GroupRow> = read_json(request.send_json(json!({ "name": name })))?;
    let row = rows.into_iter().next().ok_or_else(|| Error::Local("Grup oluşturulamadı".into()))?;
    // The server adds the owner as the first member.
    Ok(Group { members: vec![row.owner_id], ..Group::from(row) })
}

/// Looks a user up by exact Discord username (case-insensitive). `None`: nobody by that name has
/// signed in to Odak yet.
pub fn find_profile(session: &Session, username: &str) -> Result<Option<Profile>, Error> {
    let request = authorized(AGENT.post(endpoint("/rest/v1/rpc/find_profile_by_discord_name")), session);
    let rows: Vec<Profile> = read_json(request.send_json(json!({ "name": username })))?;
    Ok(rows.into_iter().next())
}

pub fn add_member(session: &Session, group: Uuid, user: Uuid) -> Result<(), Error> {
    let request = authorized(AGENT.post(endpoint("/rest/v1/group_members")), session);
    check(request.send_json(json!({ "group_id": group, "user_id": user }))).map(drop)
}

/// Removes a member, or the account itself when leaving.
pub fn remove_member(session: &Session, group: Uuid, user: Uuid) -> Result<(), Error> {
    let request = authorized(AGENT.delete(endpoint("/rest/v1/group_members")), session)
        .query("group_id", format!("eq.{group}"))
        .query("user_id", format!("eq.{user}"));
    check(request.call()).map(drop)
}

/// Deletes a group with its tasks (owner only).
pub fn delete_group(session: &Session, group: Uuid) -> Result<(), Error> {
    let request = authorized(AGENT.delete(endpoint("/rest/v1/groups")), session).query("id", format!("eq.{group}"));
    check(request.call()).map(drop)
}

/// A comment on a task. Online only: not kept in data.json.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Comment {
    pub id: Uuid,
    pub task_id: Uuid,
    pub user_id: Uuid,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

/// An entry of a task's history, written by database triggers.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Activity {
    pub id: i64,
    pub user_id: Option<Uuid>,
    pub action: String,
    #[serde(default)]
    pub details: String,
    pub created_at: DateTime<Utc>,
}

/// A task's comments, oldest first.
pub fn fetch_comments(session: &Session, task: Uuid) -> Result<Vec<Comment>, Error> {
    let request = authorized(AGENT.get(endpoint("/rest/v1/task_comments")), session)
        .query("select", "id,task_id,user_id,body,created_at")
        .query("task_id", format!("eq.{task}"))
        .query("order", "created_at");
    read_json(request.call())
}

pub fn add_comment(session: &Session, task: Uuid, body: &str) -> Result<(), Error> {
    let request = authorized(AGENT.post(endpoint("/rest/v1/task_comments")), session);
    check(request.send_json(json!({ "task_id": task, "body": body }))).map(drop)
}

pub fn delete_comment(session: &Session, id: Uuid) -> Result<(), Error> {
    let request = authorized(AGENT.delete(endpoint("/rest/v1/task_comments")), session).query("id", format!("eq.{id}"));
    check(request.call()).map(drop)
}

/// A task's latest history entries, newest first.
pub fn fetch_activity(session: &Session, task: Uuid) -> Result<Vec<Activity>, Error> {
    let request = authorized(AGENT.get(endpoint("/rest/v1/task_activity")), session)
        .query("select", "id,user_id,action,details,created_at")
        .query("task_id", format!("eq.{task}"))
        .query("order", "created_at.desc")
        .query("limit", "30");
    read_json(request.call())
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
