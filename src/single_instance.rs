//! Keeps one copy running. A second launch asks the first to show its window, then exits.
//! Windows uses a named mutex and event; other systems a Unix socket in the runtime directory.

#[cfg(not(windows))]
pub use unix::*;
#[cfg(windows)]
pub use win::*;

#[cfg(windows)]
mod win {
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
}

#[cfg(not(windows))]
mod unix {
    use crate::tray::Command;
    use futures::channel::mpsc::UnboundedSender;
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;

    /// Proof that this is the only running copy.
    pub struct Instance {
        listener: Option<UnixListener>,
    }

    /// `$XDG_RUNTIME_DIR/<APP_ID>.sock` (private to this user), else the data folder.
    fn socket_path() -> PathBuf {
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(crate::store::data_dir);
        dir.join(format!("{}.sock", crate::APP_ID))
    }

    /// `None` if another copy is already running (it has been asked to show its window).
    pub fn acquire() -> Option<Instance> {
        let path = socket_path();
        if let Ok(mut stream) = UnixStream::connect(&path) {
            let _ = stream.write_all(b"show");
            return None;
        }
        // Nobody answers: whatever is left at the path belongs to a copy that has exited.
        let _ = std::fs::remove_file(&path);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let listener = UnixListener::bind(&path)
            .map_err(|e| log::warn!("single-instance socket unavailable ({}): {e}", path.display()))
            .ok();
        Some(Instance { listener })
    }

    impl Instance {
        /// Forwards "show" requests from later launches as `Command::Show`.
        pub fn listen(self, commands: UnboundedSender<Command>) {
            let Some(listener) = self.listener else { return };
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { continue };
                    let mut request = Vec::new();
                    let _ = (&mut stream).take(16).read_to_end(&mut request);
                    if request != b"show" {
                        continue;
                    }
                    if commands.unbounded_send(Command::Show).is_err() {
                        break;
                    }
                }
            });
        }
    }
}
