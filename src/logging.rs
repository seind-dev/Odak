//! File logging to `<data dir>\app.log` (previous run kept as app.log.old). Panics are logged too.

use simplelog::{Config, LevelFilter, WriteLogger};
use std::fs::{self, File};
use std::path::Path;

pub fn init(dir: &Path) {
    let _ = fs::create_dir_all(dir);
    let log = dir.join("app.log");
    let _ = fs::rename(&log, dir.join("app.log.old"));
    if let Ok(file) = File::create(&log) {
        let _ = WriteLogger::init(LevelFilter::Info, Config::default(), file);
    }
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("panic: {info}\n{}", std::backtrace::Backtrace::force_capture());
        default_hook(info);
    }));
}
