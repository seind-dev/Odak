#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod account;
mod auth;
mod autostart;
mod data;
mod fonts;
mod logging;
mod model;
mod overlay;
mod realtime;
mod reminders;
mod single_instance;
mod state;
mod store;
mod supabase;
mod sync;
mod theme;
mod tray;
mod ui;
mod updater;
mod views;

use chrono::Utc;
use gpui::{App, QuitMode};
use model::{Notice, NoticeKind};
use state::AppState;
use std::time::Duration;

/// Name shown in the window, tray and pop-ups. The internal id (`APP_ID`) stays fixed so updates and
/// saved data keep working across renames.
pub const APP_NAME: &str = "Odak";

/// Internal id: data folder, startup entry and single-instance names (the Velopack package id in
/// release.ps1 matches it). Debug builds get their own id so `cargo run` can run next to an
/// installed copy without sharing its data or being taken for a second launch of it.
pub const APP_ID: &str = if cfg!(debug_assertions) { "seindtask-dev" } else { "seindtask" };

/// How often due reminders are checked.
const REMINDER_POLL: Duration = Duration::from_secs(15);

fn main() {
    // Must run first: handles Velopack install/update hooks and applies downloaded updates.
    velopack::VelopackApp::build().run();
    let dir = store::data_dir();
    logging::init(&dir);
    let Some(instance) = single_instance::acquire() else {
        return; // another copy is running and was asked to show its window
    };
    let minimized = std::env::args().any(|arg| arg == "--minimized");

    // Explicit: closing the window keeps the app (tray, reminders) running; only "Çıkış" quits.
    gpui_platform::application().with_quit_mode(QuitMode::Explicit).run(move |cx: &mut App| {
        fonts::load(cx);
        ui::motion::follow_system_setting(cx);
        ui::text_input::bind_keys(cx);
        ui::shell::bind_keys(cx);
        let state = AppState::init(dir.join("data.json"), cx);
        let settings = state.read(cx).data.settings.clone();
        autostart::apply(settings.auto_launch);
        account::restore(cx);
        sync::start(cx);
        realtime::start(cx);
        instance.listen(tray::install(cx));
        if !(minimized || settings.start_minimized) {
            ui::shell::open_main_window(cx);
        }
        overlay::startup_alert(cx);
        start_reminder_loop(cx);
        start_update_check(cx);
    });
}

/// Fires due reminders now (catching ones missed while the app was closed), then every `REMINDER_POLL`.
fn start_reminder_loop(cx: &mut App) {
    cx.spawn(async move |cx| {
        loop {
            cx.update(|cx| {
                ui::motion::follow_system_setting(cx);
                fire_due_reminders(cx);
            });
            cx.background_executor().timer(REMINDER_POLL).await;
        }
    })
    .detach();
}

fn fire_due_reminders(cx: &mut App) {
    let state = AppState::global(cx);
    let now = Utc::now();
    if !state.read(cx).data.has_due_reminders(now) {
        return;
    }
    let fired = state.update(cx, |s, cx| s.mutate(cx, |d| d.fire_due_reminders(now)));
    for task in &fired {
        overlay::reminder(cx, task);
    }
}

/// About 3 s after startup: check GitHub Releases and, if a newer version was downloaded, say so.
fn start_update_check(cx: &mut App) {
    cx.spawn(async move |cx| {
        cx.background_executor().timer(Duration::from_secs(3)).await;
        let outcome = cx.background_executor().spawn(async { updater::check_and_download() }).await;
        if let updater::Outcome::Ready(version) = outcome {
            let body = format!("v{version} çıkışta veya sonraki açılışta kurulacak");
            cx.update(|cx| overlay::notify(cx, Notice::new(NoticeKind::Update, "Güncelleme hazır", body, Utc::now())));
        }
    })
    .detach();
}
