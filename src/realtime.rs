//! Realtime: a websocket to Supabase Realtime on its own thread (protocol 1.0.0, JSON messages).
//! A task change on the server triggers a sync; a new comment refreshes an open form and notifies
//! the task's owner and assignee. The access token follows the session in `AppState`.

use crate::model::{Notice, NoticeKind};
use crate::overlay;
use crate::state::{AppState, Auth};
use crate::supabase::{self, Comment};
use crate::sync;
use crate::ui::widgets::user_name;
use chrono::Utc;
use futures::StreamExt;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui::{App, AppContext, Entity, EventEmitter, Global};
use serde_json::{Value, json};
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

const TOPIC: &str = "realtime:odak";
const HEARTBEAT: Duration = Duration::from_secs(25);
/// Server changes come in bursts (a sync from another device); one pull covers them.
const PULL_AFTER: Duration = Duration::from_secs(1);
const RECONNECT: [Duration; 4] =
    [Duration::from_secs(1), Duration::from_secs(5), Duration::from_secs(15), Duration::from_secs(60)];

/// Emits `CommentAdded` for every new comment the account can see (open forms listen).
pub struct CommentFeed;

pub struct CommentAdded(pub Comment);

impl EventEmitter<CommentAdded> for CommentFeed {}

struct Realtime {
    feed: Entity<CommentFeed>,
    /// Access token last handed to the connection thread (`None`: disconnected).
    token: Option<String>,
    control: Option<mpsc::Sender<Option<String>>>,
}

impl Global for Realtime {}

pub fn feed(cx: &App) -> Entity<CommentFeed> {
    cx.global::<Realtime>().feed.clone()
}

pub fn start(cx: &mut App) {
    let feed = cx.new(|_| CommentFeed);
    if !supabase::enabled() {
        cx.set_global(Realtime { feed, token: None, control: None });
        return;
    }
    let (control_tx, control_rx) = mpsc::channel();
    let (events_tx, mut events) = unbounded();
    thread::spawn(move || run(control_rx, events_tx));
    cx.set_global(Realtime { feed, token: None, control: Some(control_tx) });

    // Hand the connection a new token whenever the session changes (sign-in, refresh, sign-out).
    let state = AppState::global(cx);
    cx.observe(&state, |state, cx| {
        let token = match &state.read(cx).auth {
            Auth::SignedIn(session) if !session.needs_refresh(Utc::now()) => Some(session.access_token.clone()),
            _ => None,
        };
        let realtime = cx.global_mut::<Realtime>();
        if realtime.token != token {
            realtime.token = token.clone();
            if let Some(control) = &realtime.control {
                let _ = control.send(token);
            }
        }
    })
    .detach();

    cx.spawn(async move |cx| {
        while let Some(event) = events.next().await {
            cx.update(|cx| match event {
                Incoming::TaskChanged => sync::request(cx, PULL_AFTER),
                Incoming::Comment(comment) => comment_added(comment, cx),
                Incoming::Rejoin(_) | Incoming::Ignore => {}
            });
        }
    })
    .detach();
}

fn comment_added(comment: Comment, cx: &mut App) {
    let notice = {
        let data = &AppState::global(cx).read(cx).data;
        let me = data.me();
        let task = data.task(comment.task_id);
        let involved = task.is_some_and(|t| me.is_some() && (t.owner_id == me || t.assignee_id == me));
        (involved && Some(comment.user_id) != me).then(|| {
            let task = task.expect("checked above");
            let body: String = comment.body.chars().take(80).collect();
            Notice {
                task_id: Some(task.id),
                ..Notice::new(
                    NoticeKind::Shared,
                    format!("{} yorum yaptı", user_name(data, comment.user_id)),
                    format!("{}: {body}", task.title),
                    Utc::now(),
                )
            }
        })
    };
    feed(cx).update(cx, |_, cx| cx.emit(CommentAdded(comment)));
    if let Some(notice) = notice {
        overlay::notify(cx, notice);
    }
}

#[derive(Debug, PartialEq)]
enum Incoming {
    TaskChanged,
    Comment(Comment),
    /// The channel failed or was closed by the server: reconnect and join again.
    Rejoin(String),
    Ignore,
}

/// Reads one server message.
fn parse(text: &str) -> Incoming {
    let Ok(message) = serde_json::from_str::<Value>(text) else { return Incoming::Ignore };
    let payload = &message["payload"];
    if message["topic"] != TOPIC {
        return Incoming::Ignore; // heartbeat replies
    }
    match message["event"].as_str().unwrap_or_default() {
        "postgres_changes" => {
            let data = &payload["data"];
            match (data["table"].as_str(), data["type"].as_str()) {
                (Some("tasks"), _) => Incoming::TaskChanged,
                (Some("task_comments"), Some("INSERT")) => {
                    serde_json::from_value(data["record"].clone()).map_or(Incoming::Ignore, Incoming::Comment)
                }
                _ => Incoming::Ignore,
            }
        }
        "phx_reply" if payload["status"] == "error" => Incoming::Rejoin(format!("join rejected: {}", payload["response"])),
        "system" if payload["status"] == "error" => Incoming::Rejoin(format!("channel error: {}", payload["message"])),
        "phx_error" | "phx_close" => Incoming::Rejoin(format!("channel {}", message["event"])),
        _ => Incoming::Ignore,
    }
}

fn join(token: &str) -> Value {
    json!({
        "topic": TOPIC,
        "event": "phx_join",
        "ref": "1",
        "join_ref": "1",
        "payload": {
            "config": {
                "broadcast": { "ack": false, "self": false },
                "presence": { "enabled": false },
                "private": false,
                "postgres_changes": [
                    { "event": "*", "schema": "public", "table": "tasks" },
                    { "event": "INSERT", "schema": "public", "table": "task_comments" },
                ],
            },
            "access_token": token,
        },
    })
}

/// Why a connection ended.
enum End {
    /// Signed out: wait for the next token.
    SignedOut,
    /// Dropped or rejected: reconnect after a pause.
    Failed(String),
    /// The app is quitting.
    Quit,
}

/// The connection thread: waits for a token, stays connected while signed in, reconnects with a
/// growing pause after failures.
fn run(control: Receiver<Option<String>>, events: UnboundedSender<Incoming>) {
    let mut token: Option<String> = None;
    let mut failures = 0;
    loop {
        while token.is_none() {
            match control.recv() {
                Ok(next) => token = next,
                Err(_) => return,
            }
        }
        match connection(&mut token, &control, &events) {
            End::Quit => return,
            End::SignedOut => {
                token = None;
                failures = 0;
            }
            End::Failed(reason) => {
                log::info!("realtime disconnected: {reason}");
                let pause = RECONNECT[failures.min(RECONNECT.len() - 1)];
                failures += 1;
                // Wait, but take a new token (or a sign-out) as soon as it comes.
                match control.recv_timeout(pause) {
                    Ok(next) => token = next,
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        }
    }
}

fn connection(token: &mut Option<String>, control: &Receiver<Option<String>>, events: &UnboundedSender<Incoming>) -> End {
    let Some(current) = token.clone() else { return End::SignedOut };
    let mut socket = match tungstenite::connect(supabase::realtime_url()) {
        Ok((socket, _)) => socket,
        Err(e) => return End::Failed(e.to_string()),
    };
    // Reads wake up every second to send heartbeats and pick up new tokens.
    let tcp: &TcpStream = match socket.get_ref() {
        MaybeTlsStream::Rustls(tls) => tls.get_ref(),
        MaybeTlsStream::Plain(tcp) => tcp,
        _ => return End::Failed("unexpected stream".into()),
    };
    if let Err(e) = tcp.set_read_timeout(Some(Duration::from_secs(1))) {
        return End::Failed(e.to_string());
    }
    if let Err(e) = send(&mut socket, join(&current)) {
        return End::Failed(e.to_string());
    }
    log::info!("realtime connected");
    let mut next_ref: u64 = 2;
    let mut last_beat = Instant::now();
    loop {
        loop {
            match control.try_recv() {
                Ok(Some(fresh)) => {
                    let message = json!({ "topic": TOPIC, "event": "access_token", "ref": next_ref.to_string(), "payload": { "access_token": fresh } });
                    next_ref += 1;
                    *token = Some(fresh);
                    if let Err(e) = send(&mut socket, message) {
                        return End::Failed(e.to_string());
                    }
                }
                Ok(None) => {
                    let _ = socket.close(None);
                    return End::SignedOut;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return End::Quit,
            }
        }
        if last_beat.elapsed() >= HEARTBEAT {
            let beat = json!({ "topic": "phoenix", "event": "heartbeat", "ref": next_ref.to_string(), "payload": {} });
            next_ref += 1;
            last_beat = Instant::now();
            if let Err(e) = send(&mut socket, beat) {
                return End::Failed(e.to_string());
            }
        }
        match socket.read() {
            Ok(Message::Text(text)) => match parse(&text) {
                Incoming::Rejoin(reason) => return End::Failed(reason),
                Incoming::Ignore => {}
                event => {
                    if events.unbounded_send(event).is_err() {
                        return End::Quit;
                    }
                }
            },
            Ok(Message::Close(frame)) => return End::Failed(format!("closed by server: {frame:?}")),
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => return End::Failed(e.to_string()),
        }
    }
}

fn send(socket: &mut WebSocket<MaybeTlsStream<TcpStream>>, message: Value) -> tungstenite::Result<()> {
    socket.send(Message::text(message.to_string()))
}

#[cfg(test)]
#[path = "realtime_tests.rs"]
mod tests;
