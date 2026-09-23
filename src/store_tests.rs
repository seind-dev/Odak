use super::*;
use crate::data::TaskDraft;

/// A fresh empty directory under the system temp dir.
fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("seindtask-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn draft(title: &str) -> TaskDraft {
    TaskDraft { title: title.into(), ..Default::default() }
}

#[test]
fn missing_file_loads_empty_without_message() {
    let (data, msg) = load(&temp_dir().join("data.json"));
    assert_eq!(data, Data::default());
    assert!(msg.is_none());
}

#[test]
fn save_then_load_round_trips_and_leaves_no_temp_file() {
    let dir = temp_dir();
    let path = dir.join("data.json");
    let mut data = Data::default();
    data.add_task(draft("Süt al"), chrono::Utc::now()).unwrap();
    data.settings.start_minimized = true;
    save(&path, &data).unwrap();
    save(&path, &data).unwrap(); // replacing an existing file works too
    let (loaded, msg) = load(&path);
    assert_eq!(loaded, data);
    assert!(msg.is_none());
    assert!(!dir.join("data.json.tmp").exists());
}

#[test]
fn corrupt_file_is_moved_aside_and_reported() {
    let dir = temp_dir();
    let path = dir.join("data.json");
    fs::write(&path, "{ not json").unwrap();
    let (data, msg) = load(&path);
    assert_eq!(data, Data::default());
    assert!(msg.unwrap().contains("yedeklendi"));
    assert!(!path.exists());
    let backups: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("data.json.corrupt-"))
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(fs::read_to_string(&backups[0]).unwrap(), "{ not json");
}

#[test]
fn load_sorts_tasks_by_order() {
    let path = temp_dir().join("data.json");
    let mut data = Data::default();
    data.add_task(draft("a"), chrono::Utc::now()).unwrap();
    data.add_task(draft("b"), chrono::Utc::now()).unwrap();
    data.tasks[0].order = 5;
    save(&path, &data).unwrap();
    let (loaded, _) = load(&path);
    assert_eq!(loaded.tasks.iter().map(|t| t.title.as_str()).collect::<String>(), "ba");
}

#[test]
fn file_without_settings_loads_defaults() {
    let path = temp_dir().join("data.json");
    fs::write(&path, r#"{"version":1,"tasks":[]}"#).unwrap();
    let (loaded, msg) = load(&path);
    assert_eq!(loaded, Data::default());
    assert!(msg.is_none());
}
