//! Keeps one copy running. A second launch asks the first to show its window, then exits.

use crate::tray::Command;
use futures::channel::mpsc::UnboundedSender;
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, EVENT_MODIFY_STATE, INFINITE, OpenEventW, SetEvent, WaitForSingleObject,
};
use windows::core::PCWSTR;

/// Proof that this is the only running copy.
pub struct Instance {
    /// Auto-reset event that later launches set; a raw handle so it can move to the listener thread.
    show_event: Option<isize>,
}

/// Null-terminated UTF-16 kernel object name, e.g. `Local\seindtask-show`.
fn object_name(suffix: &str) -> Vec<u16> {
    format!("Local\\{}-{suffix}", crate::APP_ID).encode_utf16().chain([0]).collect()
}

/// `None` if another copy is already running (it has been asked to show its window).
pub fn acquire() -> Option<Instance> {
    let (mutex_name, event_name) = (object_name("single-instance"), object_name("show"));
    // The handles are intentionally never closed: they must live as long as the process.
    unsafe {
        let _mutex = CreateMutexW(None, true, PCWSTR(mutex_name.as_ptr()));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if let Ok(event) = OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(event_name.as_ptr())) {
                let _ = SetEvent(event);
            }
            return None;
        }
        let show_event = CreateEventW(None, false, false, PCWSTR(event_name.as_ptr())).ok().map(|h| h.0 as isize);
        Some(Instance { show_event })
    }
}

impl Instance {
    /// Forwards "show" requests from later launches as `Command::Show`.
    pub fn listen(self, commands: UnboundedSender<Command>) {
        let Some(raw) = self.show_event else { return };
        std::thread::spawn(move || {
            loop {
                unsafe { WaitForSingleObject(HANDLE(raw as *mut std::ffi::c_void), INFINITE) };
                if commands.unbounded_send(Command::Show).is_err() {
                    break;
                }
            }
        });
    }
}
