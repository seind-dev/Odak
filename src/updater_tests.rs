use super::*;

const SOON: Duration = Duration::from_secs(10);
const LATER: Duration = Duration::from_secs(10 * 60);

#[test]
fn updates_found_at_launch_install_right_away() {
    assert!(install_now(Trigger::Startup, true, SOON, false));
    assert!(!install_now(Trigger::Startup, true, LATER, false), "the user has been working for a while");
}

#[test]
fn an_open_task_form_postpones_the_restart() {
    assert!(!install_now(Trigger::Startup, true, SOON, true));
}

#[test]
fn a_tray_only_app_installs_silently() {
    assert!(install_now(Trigger::Periodic, false, LATER, false));
    assert!(install_now(Trigger::Startup, false, LATER, true));
}

#[test]
fn an_open_window_waits_for_the_user() {
    assert!(!install_now(Trigger::Periodic, true, LATER, false));
    assert!(!install_now(Trigger::Manual, true, SOON, false));
}
