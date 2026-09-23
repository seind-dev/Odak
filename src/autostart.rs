//! "Launch at Windows startup": an HKCU Run entry that starts the app with --minimized.

use auto_launch::{AutoLaunchBuilder, WindowsEnableMode};

pub fn apply(enabled: bool) {
    if let Err(e) = try_apply(enabled) {
        log::error!("launch at startup ({enabled}): {e}");
    }
}

fn try_apply(enabled: bool) -> Result<(), Box<dyn std::error::Error>> {
    // auto-launch writes "<path> <args>" unquoted, so quote the path for user names with spaces.
    let exe = format!("\"{}\"", std::env::current_exe()?.display());
    let launcher = AutoLaunchBuilder::new()
        .set_app_name(crate::APP_ID)
        .set_app_path(&exe)
        .set_windows_enable_mode(WindowsEnableMode::CurrentUser)
        .set_args(&["--minimized"])
        .build()?;
    if enabled {
        launcher.enable()?;
    } else if launcher.is_enabled()? {
        launcher.disable()?;
    }
    Ok(())
}
