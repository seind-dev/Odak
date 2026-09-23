//! Loads and saves data.json. Saves are atomic (temp file + rename) and synced to disk.

use crate::data::Data;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// `%APPDATA%\seindtask` (the working directory if APPDATA is unset).
pub fn data_dir() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default().join("seindtask")
}

/// Loads the data file. A missing file gives empty data. An unreadable or corrupt file is
/// moved aside (never overwritten) and the returned message says where it went.
pub fn load(path: &Path) -> (Data, Option<String>) {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return (Data::default(), None),
        Err(e) => return (Data::default(), Some(move_aside(path, &e.to_string()))),
    };
    match serde_json::from_str::<Data>(&text) {
        Ok(mut data) => {
            data.tasks.sort_by_key(|t| t.order);
            (data, None)
        }
        Err(e) => (Data::default(), Some(move_aside(path, &e.to_string()))),
    }
}

fn move_aside(path: &Path, reason: &str) -> String {
    let backup = path.with_extension(format!("json.corrupt-{}", chrono::Utc::now().timestamp()));
    log::error!("data file unusable ({reason}); moving it to {}", backup.display());
    match fs::rename(path, &backup) {
        Ok(()) => format!("Veri dosyası okunamadı, yedeklendi: {}", backup.display()),
        Err(e) => format!("Veri dosyası okunamadı ve yedeklenemedi ({e}): {}", path.display()),
    }
}

pub fn save(path: &Path, data: &Data) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(data).map_err(io::Error::other)?;
    let mut file = File::create(&tmp)?;
    file.write_all(&json)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path)
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
