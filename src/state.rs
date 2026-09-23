//! App-wide state: the persisted `Data` plus navigation, in one entity reachable through a Global.

use crate::data::Data;
use crate::store;
use crate::supabase::Session;
use crate::sync;
use chrono::Utc;
use gpui::{App, AppContext, Context, Entity, Global};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Dashboard,
    List,
    Kanban,
    Calendar,
    Form,
    Groups,
    Notifications,
    Settings,
}

/// Account session. Not saved in data.json: the refresh token lives in session.bin (see `auth`).
pub enum Auth {
    SignedOut,
    /// Browser sign-in in progress; setting the flag cancels it.
    Waiting(Arc<AtomicBool>),
    SignedIn(Session),
}

/// What the last sync turn ended with.
#[derive(Clone, Debug, PartialEq)]
pub enum SyncStatus {
    Idle,
    Syncing,
    /// The server could not be reached; the queue waits for the next try.
    Offline,
    Failed(String),
}

pub struct AppState {
    pub data: Data,
    pub page: Page,
    /// Task open in the form; `None` on the form page means "new task".
    pub editing: Option<Uuid>,
    /// Error shown above the page until dismissed (or, for save errors, until a save works).
    pub banner: Option<String>,
    pub auth: Auth,
    /// Why the last sign-in or session restore failed, shown in Settings.
    pub auth_error: Option<String>,
    pub sync: SyncStatus,
    /// Settings asks before signing out with changes still waiting to be uploaded.
    pub confirm_sign_out: bool,
    path: PathBuf,
}

struct GlobalState(Entity<AppState>);

impl Global for GlobalState {}

const SAVE_ERROR: &str = "Kaydedilemedi: ";

impl AppState {
    /// Loads `path` and registers the state as a global.
    pub fn init(path: PathBuf, cx: &mut App) -> Entity<AppState> {
        let (data, banner) = store::load(&path);
        let state = cx.new(|_| AppState {
            data,
            page: Page::List,
            editing: None,
            banner,
            auth: Auth::SignedOut,
            auth_error: None,
            sync: SyncStatus::Idle,
            confirm_sign_out: false,
            path,
        });
        cx.set_global(GlobalState(state.clone()));
        state
    }

    pub fn global(cx: &App) -> Entity<AppState> {
        cx.global::<GlobalState>().0.clone()
    }

    /// Applies a change, queues task changes for upload (see `Data::track_changes`), saves data.json
    /// and re-renders.
    pub fn mutate<R>(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Data) -> R) -> R {
        let before = self.data.last_account.is_some().then(|| self.data.tasks.clone());
        let result = f(&mut self.data);
        if let Some(before) = before
            && self.data.track_changes(&before, Utc::now())
        {
            sync::request(cx, sync::AFTER_CHANGE);
        }
        self.save(cx);
        result
    }

    /// Applies what came from the server: saved, but not queued for upload again.
    pub fn apply_remote<R>(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Data) -> R) -> R {
        let result = f(&mut self.data);
        self.save(cx);
        result
    }

    /// Saves data.json and re-renders. A failed save keeps the change in memory and shows a
    /// banner; the next change saves everything again.
    fn save(&mut self, cx: &mut Context<Self>) {
        // Note: synchronous save on the UI thread (the file is a few KB); move to the background executor if it grows.
        match store::save(&self.path, &self.data) {
            Ok(()) => {
                if self.banner.as_deref().is_some_and(|b| b.starts_with(SAVE_ERROR)) {
                    self.banner = None;
                }
            }
            Err(e) => {
                log::error!("save failed: {e}");
                self.banner = Some(format!("{SAVE_ERROR}{e}"));
            }
        }
        cx.notify();
    }

    pub fn navigate(&mut self, page: Page, cx: &mut Context<Self>) {
        self.page = page;
        self.editing = None;
        cx.notify();
    }

    pub fn edit(&mut self, id: Uuid, cx: &mut Context<Self>) {
        self.page = Page::Form;
        self.editing = Some(id);
        cx.notify();
    }

    pub fn dismiss_banner(&mut self, cx: &mut Context<Self>) {
        self.banner = None;
        cx.notify();
    }
}
