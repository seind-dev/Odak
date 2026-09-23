//! Auto-update from GitHub Releases through Velopack (installed and portable builds; a `cargo run`
//! build has nothing to update).
//!
//! - Checks two seconds after launch, then every two hours, and downloads in the background.
//! - Installs at once (the app restarts itself) right after launch, or whenever only the tray is
//!   running; otherwise the sidebar and Settings offer "restart to update", and quitting installs.
//! - A download left over from an earlier run is applied on launch by `VelopackApp` (see main).
//! - One check or download at a time.

use crate::account::blocking;
use crate::model::{Notice, NoticeKind};
use crate::overlay;
use crate::state::{AppState, Page};
use crate::ui::shell::Shell;
use chrono::{DateTime, Utc};
use futures::StreamExt;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui::{App, AppContext, Entity, Global};
use std::thread;
use std::time::{Duration, Instant};
use velopack::sources::GithubSource;
use velopack::{UpdateCheck, UpdateInfo, UpdateManager};

const REPO_URL: &str = "https://github.com/seind-dev/Odak";
const FIRST_CHECK: Duration = Duration::from_secs(2);
const CHECK_EVERY: Duration = Duration::from_secs(2 * 60 * 60);
/// How often a waiting update looks for a moment when nobody sees the window (no network).
const TICK: Duration = Duration::from_secs(60);
/// An update found this soon after launch installs right away.
const FRESH_START: Duration = Duration::from_secs(90);
/// How long "Odak güncelleniyor" shows before the window goes away.
const NOTICE_BEFORE_RESTART: Duration = Duration::from_millis(1500);

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    /// Not checked yet.
    Idle,
    /// A development build: there is no installation to update.
    NotInstalled,
    Checking,
    UpToDate(DateTime<Utc>),
    Downloading { version: String, percent: u8 },
    /// Downloaded; installs on restart.
    Ready(String),
    Installing(String),
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    Startup,
    Periodic,
    Manual,
}

pub struct Updater {
    pub phase: Phase,
    pending: Option<UpdateInfo>,
    started: Instant,
    last_check: Option<Instant>,
}

struct GlobalUpdater(Entity<Updater>);

impl Global for GlobalUpdater {}

pub fn entity(cx: &App) -> Entity<Updater> {
    cx.global::<GlobalUpdater>().0.clone()
}

fn manager() -> Result<UpdateManager, velopack::Error> {
    UpdateManager::new(GithubSource::new(REPO_URL, None, false), None, None)
}

/// Creates the updater and runs its schedule. Call before the main window opens.
pub fn start(cx: &mut App) {
    let updater = cx.new(|_| Updater { phase: Phase::Idle, pending: None, started: Instant::now(), last_check: None });
    cx.set_global(GlobalUpdater(updater));
    cx.spawn(async move |cx| {
        cx.background_executor().timer(FIRST_CHECK).await;
        cx.update(|cx| check(Trigger::Startup, cx));
        loop {
            cx.background_executor().timer(TICK).await;
            cx.update(|cx| {
                let (ready, due) = {
                    let updater = entity(cx).read(cx);
                    (
                        matches!(updater.phase, Phase::Ready(_)),
                        updater.last_check.is_none_or(|at| at.elapsed() >= CHECK_EVERY),
                    )
                };
                if ready && !main_window_open(cx) {
                    install(cx);
                } else if due {
                    check(Trigger::Periodic, cx);
                }
            });
        }
    })
    .detach();
}

/// Whether a freshly downloaded update should install right away (restarting the app).
pub fn install_now(trigger: Trigger, window_open: bool, since_launch: Duration, editing_task: bool) -> bool {
    match trigger {
        // Asked for from Settings: the user chooses when with "Yeniden başlat ve güncelle".
        Trigger::Manual => false,
        // Only the tray is running: nobody notices a restart.
        _ if !window_open => true,
        // Just opened: update now, unless a task form holds unsaved typing.
        Trigger::Startup => since_launch < FRESH_START && !editing_task,
        Trigger::Periodic => false,
    }
}

fn main_window_open(cx: &App) -> bool {
    cx.windows().iter().any(|w| w.downcast::<Shell>().is_some())
}

fn set_phase(phase: Phase, cx: &mut App) {
    entity(cx).update(cx, |updater, cx| {
        updater.phase = phase;
        cx.notify();
    });
}

enum Step {
    Found(String),
    Percent(u8),
}

enum Outcome {
    NotInstalled,
    UpToDate,
    Downloaded(Box<UpdateInfo>),
    Failed(String),
}

/// Checks GitHub and downloads a newer version if there is one.
pub fn check(trigger: Trigger, cx: &mut App) {
    let updater = entity(cx);
    let busy = matches!(
        updater.read(cx).phase,
        Phase::Checking | Phase::Downloading { .. } | Phase::Ready(_) | Phase::Installing(_)
    );
    if busy {
        return;
    }
    updater.update(cx, |updater, cx| {
        updater.phase = Phase::Checking;
        updater.last_check = Some(Instant::now());
        cx.notify();
    });
    let (steps, mut progress) = unbounded();
    let work = blocking(move || find_and_download(steps));
    cx.spawn(async move |cx| {
        while let Some(step) = progress.next().await {
            // Progress may arrive after the check finished; it only moves a running download on.
            cx.update(|cx| {
                let (version, percent) = match (step, &entity(cx).read(cx).phase) {
                    (Step::Found(version), Phase::Checking) => (version, 0),
                    (Step::Percent(percent), Phase::Downloading { version, .. }) => (version.clone(), percent),
                    _ => return,
                };
                set_phase(Phase::Downloading { version, percent }, cx);
            });
        }
    })
    .detach();
    cx.spawn(async move |cx| {
        let Ok(outcome) = work.await else { return };
        cx.update(|cx| finish(trigger, outcome, cx));
    })
    .detach();
}

/// The network part of a check. Blocking.
fn find_and_download(steps: UnboundedSender<Step>) -> Outcome {
    let manager = match manager() {
        Ok(manager) => manager,
        Err(e) => {
            log::info!("updater unavailable: {e}");
            return Outcome::NotInstalled;
        }
    };
    let update = match manager.check_for_updates() {
        Ok(UpdateCheck::UpdateAvailable(update)) => *update,
        Ok(_) => return Outcome::UpToDate,
        Err(e) => return Outcome::Failed(e.to_string()),
    };
    let _ = steps.unbounded_send(Step::Found(update.TargetFullRelease.Version.clone()));
    let (percent_tx, percent_rx) = std::sync::mpsc::channel::<i16>();
    let forward = thread::spawn(move || {
        for percent in percent_rx {
            let _ = steps.unbounded_send(Step::Percent(percent.clamp(0, 100) as u8));
        }
    });
    let downloaded = manager.download_updates(&update, Some(percent_tx));
    let _ = forward.join();
    match downloaded {
        Ok(()) => Outcome::Downloaded(Box::new(update)),
        Err(e) => Outcome::Failed(e.to_string()),
    }
}

fn finish(trigger: Trigger, outcome: Outcome, cx: &mut App) {
    match outcome {
        Outcome::NotInstalled => set_phase(Phase::NotInstalled, cx),
        Outcome::UpToDate => set_phase(Phase::UpToDate(Utc::now()), cx),
        Outcome::Failed(e) => {
            log::warn!("update check failed: {e}");
            // Automatic checks fail quietly (offline); the next one tries again.
            let phase = if trigger == Trigger::Manual { Phase::Failed(format!("Güncelleme denetlenemedi: {e}")) } else { Phase::Idle };
            set_phase(phase, cx);
        }
        Outcome::Downloaded(update) => {
            let version = update.TargetFullRelease.Version.clone();
            log::info!("update {version} downloaded");
            let since_launch = entity(cx).read(cx).started.elapsed();
            entity(cx).update(cx, |updater, cx| {
                updater.pending = Some(*update);
                updater.phase = Phase::Ready(version.clone());
                cx.notify();
            });
            let editing_task = AppState::global(cx).read(cx).page == Page::Form;
            if install_now(trigger, main_window_open(cx), since_launch, editing_task) {
                install(cx);
            } else if trigger != Trigger::Manual {
                let body = format!("v{version} indirildi. Kenar çubuğundaki \"Güncelle\"ye bas ya da uygulamayı kapatınca kurulsun.");
                overlay::notify(cx, Notice::new(NoticeKind::Update, "Güncelleme hazır", body, Utc::now()));
            }
        }
    }
}

/// Installs the downloaded update and restarts the app (into the tray if the window is closed).
pub fn install(cx: &mut App) {
    let updater = entity(cx);
    let Some(update) = updater.update(cx, |updater, _| updater.pending.take()) else { return };
    let version = update.TargetFullRelease.Version.clone();
    let window_open = main_window_open(cx);
    set_phase(Phase::Installing(version.clone()), cx);
    if window_open {
        let body = format!("v{version} kuruluyor; Odak birkaç saniye içinde yeniden açılacak.");
        overlay::notify(cx, Notice::new(NoticeKind::Update, "Odak güncelleniyor", body, Utc::now()));
    }
    cx.spawn(async move |cx| {
        if window_open {
            cx.background_executor().timer(NOTICE_BEFORE_RESTART).await;
        }
        cx.update(|cx| {
            let args: Vec<&str> = if window_open { Vec::new() } else { vec!["--minimized"] };
            // Velopack's updater waits for this process to exit, installs, then starts the new version.
            match manager().and_then(|m| m.wait_exit_then_apply_updates(&update, true, true, args)) {
                Ok(()) => cx.quit(),
                Err(e) => {
                    log::error!("could not start the update installer: {e}");
                    entity(cx).update(cx, |updater, cx| {
                        updater.pending = Some(update);
                        updater.phase = Phase::Failed(format!("Güncelleme kurulamadı: {e}"));
                        cx.notify();
                    });
                }
            }
        });
    })
    .detach();
}

/// Call right before quitting: installs a downloaded update after the app exits (no restart).
pub fn install_on_quit(cx: &mut App) {
    let Some(update) = entity(cx).update(cx, |updater, _| updater.pending.take()) else { return };
    if let Err(e) = manager().and_then(|m| m.wait_exit_then_apply_updates(&update, true, false, Vec::<String>::new())) {
        log::error!("could not start the update installer: {e}");
    }
}

/// After an update: says once which version is now running.
pub fn announce_new_version(cx: &mut App) {
    let current = env!("CARGO_PKG_VERSION");
    let state = AppState::global(cx);
    let last = state.read(cx).data.settings.last_version.clone();
    if last == current {
        return;
    }
    state.update(cx, |s, cx| s.mutate(cx, |d| d.settings.last_version = current.to_string()));
    // Empty: first run of a version that records it, nothing to compare with.
    if !last.is_empty() {
        let body = format!("v{last} → v{current}");
        overlay::notify(cx, Notice::new(NoticeKind::Update, "Odak güncellendi", body, Utc::now()));
    }
}

#[cfg(test)]
#[path = "updater_tests.rs"]
mod tests;
