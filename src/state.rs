//! App-wide state: the persisted `Data` plus navigation, in one entity reachable through a Global.

use crate::data::Data;
use crate::model::{Status, Task};
use crate::store;
use crate::supabase::Session;
use crate::sync;
use crate::views;
use chrono::Utc;
use gpui::{App, AppContext, Context, Entity, Global};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use uuid::Uuid;

/// How long "Geri al" stays on screen.
const UNDO_FOR: Duration = Duration::from_secs(6);

/// A deletion or change that can still be taken back ("Geri al" or Ctrl+Z).
pub struct Undo {
    pub label: String,
    /// Tells toasts apart, so a new one replaces the old one's animation and timer.
    pub generation: u64,
    task: Task,
    position: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    pub undo: Option<Undo>,
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
            undo: None,
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

    /// Deletes a task, offering "Geri al".
    pub fn delete_task(&mut self, id: Uuid, cx: &mut Context<Self>) {
        let Some(position) = self.data.tasks.iter().position(|t| t.id == id) else { return };
        let task = self.data.tasks[position].clone();
        if self.mutate(cx, |d| d.delete_task(id)).is_ok() {
            self.offer_undo(format!("\"{}\" silindi", task.title), task, position, cx);
        }
    }

    /// Changes a task's status. Completing a repeating task moves it to its next date, which can
    /// be taken back.
    pub fn set_status(&mut self, id: Uuid, status: Status, cx: &mut Context<Self>) {
        let Some(position) = self.data.tasks.iter().position(|t| t.id == id) else { return };
        let before = self.data.tasks[position].clone();
        let _ = self.mutate(cx, |d| d.set_status(id, status, Utc::now()));
        self.offer_undo_if_repeated(before, status, cx);
    }

    /// After a repeating task was completed (list, Kanban, form, pop-up): says where it moved.
    pub fn offer_undo_if_repeated(&mut self, before: Task, status: Status, cx: &mut Context<Self>) {
        if status != Status::Completed || before.status == Status::Completed || before.recurrence.is_none() {
            return;
        }
        let Some(position) = self.data.tasks.iter().position(|t| t.id == before.id) else { return };
        let next = self.data.tasks[position].due_date.map(views::format_date).unwrap_or_default();
        self.offer_undo(format!("\"{}\" tamamlandı · sonraki: {next}", before.title), before, position, cx);
    }

    fn offer_undo(&mut self, label: String, task: Task, position: usize, cx: &mut Context<Self>) {
        let generation = self.undo.as_ref().map_or(0, |u| u.generation) + 1;
        self.undo = Some(Undo { label, generation, task, position });
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(UNDO_FOR).await;
            let _ = this.update(cx, |s, cx| {
                if s.undo.as_ref().is_some_and(|u| u.generation == generation) {
                    s.undo = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Takes back the last deletion or repeating-task completion, while its toast is up.
    pub fn undo(&mut self, cx: &mut Context<Self>) {
        if let Some(undo) = self.undo.take() {
            self.mutate(cx, |d| d.restore_task(undo.task, undo.position));
        }
    }

    pub fn dismiss_undo(&mut self, cx: &mut Context<Self>) {
        self.undo = None;
        cx.notify();
    }

    pub fn dismiss_banner(&mut self, cx: &mut Context<Self>) {
        self.banner = None;
        cx.notify();
    }
}
