//! App-wide state: the persisted `Data` plus navigation, in one entity reachable through a Global.

use crate::data::Data;
use crate::store;
use gpui::{App, AppContext, Context, Entity, Global};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Dashboard,
    List,
    Kanban,
    Calendar,
    Form,
    Notifications,
    Settings,
}

pub struct AppState {
    pub data: Data,
    pub page: Page,
    /// Task open in the form; `None` on the form page means "new task".
    pub editing: Option<Uuid>,
    /// Error shown above the page until dismissed (or, for save errors, until a save works).
    pub banner: Option<String>,
    path: PathBuf,
}

struct GlobalState(Entity<AppState>);

impl Global for GlobalState {}

const SAVE_ERROR: &str = "Kaydedilemedi: ";

impl AppState {
    /// Loads `path` and registers the state as a global.
    pub fn init(path: PathBuf, cx: &mut App) -> Entity<AppState> {
        let (data, banner) = store::load(&path);
        let state = cx.new(|_| AppState { data, page: Page::List, editing: None, banner, path });
        cx.set_global(GlobalState(state.clone()));
        state
    }

    pub fn global(cx: &App) -> Entity<AppState> {
        cx.global::<GlobalState>().0.clone()
    }

    /// Applies a change, saves data.json and re-renders. A failed save keeps the change in
    /// memory and shows a banner; the next change saves everything again.
    pub fn mutate<R>(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Data) -> R) -> R {
        let result = f(&mut self.data);
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
        result
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
