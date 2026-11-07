//! "Launch at startup": an HKCU Run entry on Windows, an XDG autostart entry on Linux. Whether the
//! window shows at login is the "Küçültülmüş başlat" setting's call, not the entry's (`--minimized`
//! is only for the updater's restart of an app that was running in the tray).

use auto_launch::{AutoLaunchBuilder, WindowsEnableMode};

pub fn apply(enabled: bool) {
    if let Err(e) = try_apply(enabled) {
        log::error!("launch at startup ({enabled}): {e}");
    }
}

fn try_apply(enabled: bool) -> Result<(), Box<dyn std::error::Error>> {
    // auto-launch writes "<path> <args>" unquoted, so quote the path for user names with spaces.
    let exe = format!("\"{}\"", launcher_path()?.display());
    let launcher = AutoLaunchBuilder::new()
        .set_app_name(crate::APP_ID)
        .set_app_path(&exe)
        .set_windows_enable_mode(WindowsEnableMode::CurrentUser)
        .build()?;
    if enabled {
        // Rewritten on every start, which also drops the --minimized of entries from older versions.
        launcher.enable()?;
    } else if launcher.is_enabled()? {
        launcher.disable()?;
    }
    Ok(())
}

/// The file to start. Inside an AppImage `current_exe` is a temporary mount that is gone after a
/// reboot, so the AppImage itself (which its runtime puts in `APPIMAGE`) is started instead.
fn launcher_path() -> std::io::Result<std::path::PathBuf> {
    match std::env::var_os("APPIMAGE").filter(|path| !path.is_empty()) {
        Some(path) if cfg!(target_os = "linux") => Ok(path.into()),
        _ => std::env::current_exe(),
    }
}
