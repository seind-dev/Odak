//! Signing in, restoring the session at startup and signing out: wires `auth` and `supabase` into
//! the app state. Network and disk work runs on its own thread; results come back on the main thread.

use crate::auth;
use crate::model::Profile;
use crate::state::{AppState, Auth, SyncStatus};
use crate::store;
use crate::supabase::{self, Error, Session};
use crate::sync;
use crate::ui::shell::open_main_window;
use chrono::Utc;
use futures::channel::oneshot;
use gpui::App;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use uuid::Uuid;

/// Runs blocking work on its own thread; await the receiver on the main thread.
pub fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> oneshot::Receiver<T> {
    let (tx, rx) = oneshot::channel();
    thread::spawn(move || {
        let _ = tx.send(work());
    });
    rx
}

/// At startup: signed in again with the saved refresh token, offline too. The first sync turn
/// refreshes the session; if the server rejects it, Settings asks to sign in again.
pub fn restore(cx: &mut App) {
    if !supabase::enabled() {
        return;
    }
    let state = AppState::global(cx);
    let Some(account) = state.read(cx).data.account.clone() else {
        auth::forget_refresh_token(); // left over without an account: useless
        return;
    };
    let Some(token) = auth::load_refresh_token() else {
        return; // Settings shows the account with "sign in again"
    };
    state.update(cx, |s, cx| {
        s.auth = Auth::SignedIn(Session::stale(account.id, token));
        // Accounts signed in before tasks were synced are not bound yet.
        s.apply_remote(cx, |d| d.bind_account(account.id));
    });
}

/// Opens Discord sign-in in the browser and waits for it to come back.
pub fn sign_in(cx: &mut App) {
    let state = AppState::global(cx);
    let listener = match auth::Loopback::bind() {
        Ok(listener) => listener,
        Err(e) => {
            state.update(cx, |s, cx| {
                s.auth_error = Some(e.to_string());
                cx.notify();
            });
            return;
        }
    };
    let pkce = auth::Pkce::new();
    let cancel = Arc::new(AtomicBool::new(false));
    cx.open_url(&supabase::authorize_url(&pkce.challenge, auth::REDIRECT));
    state.update(cx, |s, cx| {
        s.auth = Auth::Waiting(cancel.clone());
        s.auth_error = None;
        cx.notify();
    });
    let flag = cancel.clone();
    let work = blocking(move || {
        let code = listener.wait_for_code(&flag)?;
        let session = supabase::exchange_code(&code, &pkce.verifier)?;
        keep_session(&session);
        let profile = load_profile(&session, None);
        Ok::<_, Error>((session, profile))
    });
    cx.spawn(async move |cx| {
        let Ok(result) = work.await else { return };
        cx.update(|cx| {
            let waiting = |auth: &Auth| matches!(auth, Auth::Waiting(f) if Arc::ptr_eq(f, &cancel));
            match result {
                Ok((session, profile)) => adopt(session, profile, waiting, cx),
                Err(Error::Cancelled) => {}
                Err(e) => AppState::global(cx).update(cx, |s, cx| {
                    if waiting(&s.auth) {
                        s.auth = Auth::SignedOut;
                        s.auth_error = Some(e.to_string());
                        cx.notify();
                    }
                }),
            }
        });
    })
    .detach();
}

pub fn cancel_sign_in(cx: &mut App) {
    AppState::global(cx).update(cx, |s, cx| {
        if let Auth::Waiting(cancel) = &s.auth {
            cancel.store(true, Ordering::Relaxed);
            s.auth = Auth::SignedOut;
            cx.notify();
        }
    });
}

/// Makes a new session current if sign-in is still expected (`expected` checks the state);
/// otherwise the user moved on (cancelled), so the session is ended again.
fn adopt(session: Session, profile: Option<Profile>, expected: impl Fn(&Auth) -> bool, cx: &mut App) {
    let state = AppState::global(cx);
    let adopted = state.update(cx, |s, cx| {
        if !expected(&s.auth) {
            return false;
        }
        let id = session.user_id;
        let known = s.data.account.clone().filter(|a| a.id == id);
        let account = profile.or(known).unwrap_or_else(|| Profile {
            id,
            username: String::new(),
            display_name: String::new(),
            avatar_url: None,
        });
        s.auth = Auth::SignedIn(session.clone());
        s.auth_error = None;
        s.mutate(cx, |d| {
            d.account = Some(account);
            d.bind_account(id);
        });
        true
    });
    if adopted {
        sync::request(cx, sync::NOW);
        open_main_window(cx);
    } else {
        auth::forget_refresh_token();
        thread::spawn(move || end_server_session(session));
    }
}

/// Answers the question Settings asks when another account signs in on this device.
pub fn adopt_local_tasks(copy: bool, cx: &mut App) {
    AppState::global(cx).update(cx, |s, cx| s.apply_remote(cx, |d| d.adopt_local_tasks(copy)));
    sync::request(cx, sync::NOW);
}

/// Signs out, first asking for a confirmation while changes still wait to be uploaded (and trying
/// once more to send them).
pub fn request_sign_out(cx: &mut App) {
    let state = AppState::global(cx);
    if state.read(cx).data.pending.is_empty() {
        return sign_out(cx);
    }
    state.update(cx, |s, cx| {
        s.confirm_sign_out = true;
        cx.notify();
    });
    sync::request(cx, sync::NOW);
}

pub fn keep_signed_in(cx: &mut App) {
    AppState::global(cx).update(cx, |s, cx| {
        s.confirm_sign_out = false;
        cx.notify();
    });
}

/// Forgets the account on this device and ends its server session in the background. The tasks
/// and their waiting uploads stay: signing back in with the same account sends them.
pub fn sign_out(cx: &mut App) {
    let session = AppState::global(cx).update(cx, |s, cx| {
        s.auth_error = None;
        s.confirm_sign_out = false;
        s.sync = SyncStatus::Idle;
        s.mutate(cx, |d| d.account = None);
        match std::mem::replace(&mut s.auth, Auth::SignedOut) {
            Auth::SignedIn(session) => Some(session),
            _ => None,
        }
    });
    auth::forget_refresh_token();
    if let Some(session) = session {
        thread::spawn(move || end_server_session(session));
    }
}

fn end_server_session(session: Session) {
    let session = if session.needs_refresh(Utc::now()) {
        match supabase::refresh(&session.refresh_token) {
            Ok(fresh) => fresh,
            Err(e) => return log::info!("server session left to expire: {e}"),
        }
    } else {
        session
    };
    if let Err(e) = supabase::sign_out(&session) {
        log::warn!("server sign-out failed: {e}");
    }
}

/// Saves the refresh token at once: the server rotates it, so the previous one stops working.
pub fn keep_session(session: &Session) {
    if let Err(e) = auth::save_refresh_token(&session.refresh_token) {
        log::error!("could not save the session: {e}");
    }
}

/// Loads the signed-in user's profile and caches the avatar when it is new or changed. Blocking.
pub fn load_profile(session: &Session, known_avatar: Option<&str>) -> Option<Profile> {
    let profile = supabase::own_profile(session).map_err(|e| log::warn!("profile unavailable: {e}")).ok()?;
    if profile.avatar_url.as_deref() != known_avatar || !avatar_path(profile.id).exists() {
        cache_avatar(&profile);
    }
    Some(profile)
}

/// Where a user's avatar is cached (see `cache_avatar`).
pub fn avatar_path(id: Uuid) -> PathBuf {
    store::data_dir().join("avatars").join(format!("{id}.png"))
}

/// Downloads the avatar as a small static PNG. Best effort: the UI falls back to initials.
fn cache_avatar(profile: &Profile) {
    let Some(url) = profile.avatar_url.as_deref().filter(|u| !u.is_empty()) else { return };
    // Discord serves animated avatars as .gif; the .png variant is the static first frame.
    let url = format!("{}?size=128", url.strip_suffix(".gif").map(|u| format!("{u}.png")).as_deref().unwrap_or(url));
    let path = avatar_path(profile.id);
    let saved = supabase::download(&url).map_err(|e| e.to_string()).and_then(|bytes| {
        fs::create_dir_all(path.parent().unwrap_or(&path)).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, bytes).and_then(|()| fs::rename(&tmp, &path)).map_err(|e| e.to_string())
    });
    if let Err(e) = saved {
        log::warn!("avatar not cached: {e}");
    }
}
