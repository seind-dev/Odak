//! Background sync. A turn refreshes the session if needed, sends the upload queue, pulls every
//! task and merges (`Data::merge_sync`). Turns run one at a time: at startup and sign-in, shortly
//! after a change, every five minutes and on request; after a network error they back off.

use crate::account::{self, blocking};
use crate::auth;
use crate::data::Sent;
use crate::model::{PendingOp, Profile, Task};
use crate::state::{AppState, Auth, SyncStatus};
use crate::supabase::{self, Error, Session};
use chrono::{DateTime, Utc};
use futures::channel::mpsc::{UnboundedSender, unbounded};
use futures::{FutureExt, StreamExt};
use gpui::{App, AsyncApp, Global};
use std::time::{Duration, Instant};
use uuid::Uuid;

pub const NOW: Duration = Duration::ZERO;
/// Edits come in bursts (typing, dragging); wait for them to settle.
pub const AFTER_CHANGE: Duration = Duration::from_secs(2);
const PERIOD: Duration = Duration::from_secs(5 * 60);
const BACKOFF: [Duration; 4] =
    [Duration::from_secs(5), Duration::from_secs(30), Duration::from_secs(2 * 60), Duration::from_secs(5 * 60)];

struct Engine(UnboundedSender<Duration>);

impl Global for Engine {}

/// Asks for a turn within `delay`; an earlier request wins. Does nothing before `start`.
pub fn request(cx: &App, delay: Duration) {
    if let Some(engine) = cx.try_global::<Engine>() {
        let _ = engine.0.unbounded_send(delay);
    }
}

pub fn start(cx: &mut App) {
    if !supabase::enabled() {
        return;
    }
    let (tx, mut requests) = unbounded::<Duration>();
    cx.set_global(Engine(tx));
    cx.spawn(async move |cx| {
        let mut due = Instant::now();
        let mut failures = 0;
        loop {
            // Sleep until `due`; requests can only bring it forward.
            loop {
                let wait = due.saturating_duration_since(Instant::now());
                if wait.is_zero() {
                    break;
                }
                let timer = cx.background_executor().timer(wait).fuse();
                futures::pin_mut!(timer);
                futures::select! {
                    () = timer => break,
                    request = requests.next() => match request {
                        Some(delay) => due = due.min(Instant::now() + delay),
                        None => return,
                    },
                }
            }
            // This turn covers everything asked so far; requests made while it runs schedule the next.
            while requests.try_recv().is_ok() {}
            let next = match turn(cx).await {
                Turn::Offline => {
                    failures += 1;
                    BACKOFF[(failures - 1).min(BACKOFF.len() - 1)]
                }
                Turn::Done | Turn::Skipped => {
                    failures = 0;
                    PERIOD
                }
            };
            due = Instant::now() + next;
        }
    })
    .detach();
}

enum Turn {
    Done,
    Offline,
    Skipped,
}

async fn turn(cx: &mut AsyncApp) -> Turn {
    let Some(job) = cx.update(prepare) else { return Turn::Skipped };
    let Ok(outcome) = blocking(move || run(job)).await else { return Turn::Skipped };
    cx.update(|cx| apply(outcome, cx))
}

struct Job {
    session: Session,
    /// The queue with a snapshot of each task to upload.
    ops: Vec<(PendingOp, Option<Task>)>,
    known_avatar: Option<String>,
}

fn prepare(cx: &mut App) -> Option<Job> {
    let state = AppState::global(cx);
    let job = {
        let s = state.read(cx);
        let Auth::SignedIn(session) = &s.auth else { return None };
        if s.data.needs_account_choice() {
            return None; // Settings asks what to do with the other account's tasks first
        }
        let snapshot = |op: &PendingOp| match op {
            PendingOp::Upsert(id) => s.data.task(*id).cloned(),
            PendingOp::Delete(_) => None,
        };
        Job {
            session: session.clone(),
            ops: s.data.pending.iter().map(|op| (*op, snapshot(op))).collect(),
            known_avatar: s.data.account.as_ref().and_then(|a| a.avatar_url.clone()),
        }
    };
    state.update(cx, |s, cx| {
        s.sync = SyncStatus::Syncing;
        cx.notify();
    });
    Some(job)
}

struct Outcome {
    user_id: Uuid,
    /// Set when the session was refreshed.
    session: Option<Session>,
    profile: Option<Profile>,
    sent: Vec<Sent>,
    remote: Option<Vec<Task>>,
    error: Option<Error>,
    /// The refresh token was rejected: the user has to sign in again.
    relogin: bool,
}

/// The network part of a turn. Blocking.
fn run(job: Job) -> Outcome {
    let mut session = job.session;
    let mut out = Outcome {
        user_id: session.user_id,
        session: None,
        profile: None,
        sent: Vec::new(),
        remote: None,
        error: None,
        relogin: false,
    };
    if session.needs_refresh(Utc::now()) {
        match supabase::refresh(&session.refresh_token) {
            Ok(fresh) => {
                account::keep_session(&fresh);
                out.profile = account::load_profile(&fresh, job.known_avatar.as_deref());
                out.session = Some(fresh.clone());
                session = fresh;
            }
            Err(e) => {
                out.relogin = e.is_auth_rejected();
                out.error = Some(e);
                return out;
            }
        }
    }
    for (op, snapshot) in job.ops {
        let result = match (op, &snapshot) {
            (PendingOp::Upsert(_), Some(task)) => supabase::upsert_task(&session, task).map(|_| true),
            (PendingOp::Upsert(_), None) => Ok(false),
            (PendingOp::Delete(id), _) => supabase::delete_task(&session, id).map(|()| false),
        };
        match result {
            Ok(uploaded) => out.sent.push(Sent { op, snapshot, uploaded }),
            Err(e) if e.is_temporary() => {
                out.error = Some(e); // the rest of the queue waits for the next try
                return out;
            }
            Err(e) => {
                log::warn!("sync dropped {op:?}: {e}");
                out.sent.push(Sent { op, snapshot, uploaded: false });
            }
        }
    }
    match supabase::fetch_tasks(&session) {
        Ok(tasks) => out.remote = Some(tasks),
        Err(e) => out.error = Some(e),
    }
    out
}

fn apply(out: Outcome, cx: &mut App) -> Turn {
    AppState::global(cx).update(cx, |s, cx| {
        match &mut s.auth {
            Auth::SignedIn(current) if current.user_id == out.user_id => {
                if let Some(session) = out.session {
                    *current = session;
                }
                if matches!(out.error, Some(Error::Status(401, _))) {
                    current.expires_at = DateTime::UNIX_EPOCH; // refresh on the next try
                }
            }
            _ => return Turn::Skipped, // signed out or switched accounts meanwhile
        }
        if out.relogin {
            log::warn!("session rejected: {:?}", out.error);
            auth::forget_refresh_token();
            s.auth = Auth::SignedOut;
            s.auth_error = Some("Oturumun süresi doldu, tekrar giriş yap.".into());
            s.sync = SyncStatus::Idle;
            cx.notify();
            return Turn::Skipped;
        }
        s.apply_remote(cx, |d| {
            if let Some(profile) = out.profile {
                d.account = Some(profile);
            }
            d.merge_sync(&out.sent, out.remote, Utc::now());
        });
        let (status, turn) = match out.error {
            None => (SyncStatus::Idle, Turn::Done),
            Some(e) if e.is_temporary() => {
                log::info!("sync postponed: {e}");
                (SyncStatus::Offline, Turn::Offline)
            }
            Some(e) => {
                log::warn!("sync failed: {e}");
                (SyncStatus::Failed(e.to_string()), Turn::Done)
            }
        };
        s.sync = status;
        cx.notify();
        turn
    })
}
