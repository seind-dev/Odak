//! Auto-update from GitHub Releases through Velopack. Only works in an installed build.

use std::sync::Mutex;
use velopack::sources::GithubSource;
use velopack::{UpdateCheck, UpdateInfo, UpdateManager};

const REPO_URL: &str = "https://github.com/seind-dev/Odak";

/// A downloaded update waiting for the app to quit.
static PENDING: Mutex<Option<UpdateInfo>> = Mutex::new(None);

pub enum Outcome {
    NotInstalled,
    UpToDate,
    Ready(String),
    Failed(String),
}

impl Outcome {
    pub fn message(&self) -> String {
        match self {
            Outcome::NotInstalled => "Güncelleme yalnızca kurulu sürümde çalışır.".into(),
            Outcome::UpToDate => "Uygulama güncel.".into(),
            Outcome::Ready(v) => format!("v{v} indirildi; çıkışta veya sonraki açılışta kurulacak."),
            Outcome::Failed(e) => format!("Güncelleme denetlenemedi: {e}"),
        }
    }
}

fn manager() -> Result<UpdateManager, velopack::Error> {
    UpdateManager::new(GithubSource::new(REPO_URL, None, false), None, None)
}

/// Checks GitHub and downloads a newer version if there is one. Blocking: run it off the main thread.
pub fn check_and_download() -> Outcome {
    let manager = match manager() {
        Ok(m) => m,
        Err(e) => {
            log::info!("updater unavailable: {e}");
            return Outcome::NotInstalled;
        }
    };
    let update = match manager.check_for_updates() {
        Ok(UpdateCheck::UpdateAvailable(update)) => *update,
        Ok(_) => return Outcome::UpToDate,
        Err(e) => {
            log::error!("update check failed: {e}");
            return Outcome::Failed(e.to_string());
        }
    };
    if let Err(e) = manager.download_updates(&update, None) {
        log::error!("update download failed: {e}");
        return Outcome::Failed(e.to_string());
    }
    let version = update.TargetFullRelease.Version.clone();
    log::info!("update {version} downloaded");
    *PENDING.lock().unwrap() = Some(update);
    Outcome::Ready(version)
}

/// Call right before quitting: starts Velopack's installer, which waits for this process to exit.
/// If the app is killed instead, Velopack applies the downloaded update on the next start.
pub fn apply_pending_on_exit() {
    let Some(update) = PENDING.lock().unwrap().take() else { return };
    let result = manager().and_then(|m| m.wait_exit_then_apply_updates(&update, true, false, Vec::<String>::new()));
    if let Err(e) = result {
        log::error!("could not start the update installer: {e}");
    }
}
