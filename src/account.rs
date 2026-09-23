//! Signing in, restoring the session at startup and signing out: wires `auth` and `supabase` into
//! the app state. Network and disk work runs on its own thread; results come back on the main thread.

use crate::auth;
use crate::model::Profile;
use crate::state::{AppState, Auth};
use crate::store;
use crate::supabase::{self, Error, Session};
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
fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> oneshot::Receiver<T> {
    let (tx, rx) = oneshot::channel();
    thread::spawn(move || {
        let _ = tx.send(work());
    });
    rx
}

/// At startup: signs back in with the saved refresh token. Offline, the account stays signed in
/// with a stale session that is refreshed later; a rejected token means "sign in again".
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
        s.auth = Auth::SignedIn(Session::stale(account.id, token.clone()));
        cx.notify();
    });
    let work = blocking(move || supabase::refresh(&token).map(establish));
    cx.spawn(async move |cx| {
        let Ok(result) = work.await else { return };
        cx.update(|cx| match result {
            Ok((session, profile)) => {
                adopt(session, profile, cx, |auth| matches!(auth, Auth::SignedIn(current) if current.user_id == account.id));
            }
            Err(e) if e.is_auth_rejected() => {
                log::warn!("saved session rejected: {e}");
                auth::forget_refresh_token();
                AppState::global(cx).update(cx, |s, cx| {
                    s.auth = Auth::SignedOut;
                    s.auth_error = Some("Oturumun süresi doldu, tekrar giriş yap.".into());
                    cx.notify();
                });
            }
            Err(e) => log::info!("session not refreshed yet: {e}"),
        });
    })
    .detach();
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
        supabase::exchange_code(&code, &pkce.verifier).map(establish)
    });
    cx.spawn(async move |cx| {
        let Ok(result) = work.await else { return };
        cx.update(|cx| {
            let waiting = |auth: &Auth| matches!(auth, Auth::Waiting(f) if Arc::ptr_eq(f, &cancel));
            match result {
                Ok((session, profile)) => {
                    let signed_in = adopt(session, profile, cx, waiting);
                    if signed_in {
                        open_main_window(cx);
                    }
                }
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

/// Forgets the account on this device and ends its server session in the background.
pub fn sign_out(cx: &mut App) {
    let session = AppState::global(cx).update(cx, |s, cx| {
        s.auth_error = None;
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

/// Background part of every successful sign-in: keep the (rotated) refresh token at once, then
/// load the profile and its avatar. A missing profile is not fatal.
fn establish(session: Session) -> (Session, Option<Profile>) {
    if let Err(e) = auth::save_refresh_token(&session.refresh_token) {
        log::error!("could not save the session: {e}");
    }
    let profile = supabase::own_profile(&session).map_err(|e| log::warn!("profile unavailable: {e}")).ok();
    if let Some(profile) = &profile {
        cache_avatar(profile);
    }
    (session, profile)
}

/// Makes `session` current if the app still expects it (`expected` checks the current state);
/// otherwise the user moved on (cancelled, signed out), so the session is dropped. Returns whether
/// it was adopted.
fn adopt(session: Session, profile: Option<Profile>, cx: &mut App, expected: impl Fn(&Auth) -> bool) -> bool {
    let state = AppState::global(cx);
    let adopted = state.update(cx, |s, cx| {
        if !expected(&s.auth) {
            return false;
        }
        let known = s.data.account.clone().filter(|a| a.id == session.user_id);
        let account = profile.or(known).unwrap_or_else(|| Profile {
            id: session.user_id,
            username: String::new(),
            display_name: String::new(),
            avatar_url: None,
        });
        s.auth = Auth::SignedIn(session.clone());
        s.auth_error = None;
        s.mutate(cx, |d| d.account = Some(account));
        true
    });
    if !adopted {
        auth::forget_refresh_token();
        thread::spawn(move || end_server_session(session));
    }
    adopted
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
