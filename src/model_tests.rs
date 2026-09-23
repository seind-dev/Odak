use super::*;

fn sample_task() -> Task {
    Task {
        id: Uuid::nil(),
        title: "A".into(),
        description: String::new(),
        priority: Priority::High,
        status: Status::InProgress,
        reminder: None,
        subtasks: vec![],
        tags: vec![],
        order: 0,
        due_date: None,
        created_at: DateTime::<Utc>::UNIX_EPOCH,
        updated_at: DateTime::<Utc>::UNIX_EPOCH,
    }
}

#[test]
fn task_json_is_camel_case_with_snake_case_enums() {
    let json = serde_json::to_value(sample_task()).unwrap();
    assert_eq!(json["priority"], "high");
    assert_eq!(json["status"], "in_progress");
    assert!(json.get("createdAt").is_some());
    assert!(json.get("dueDate").is_some());
}

#[test]
fn minimal_task_json_loads_with_defaults_and_ignores_unknown_fields() {
    // Later phases add fields (e.g. groupId); files must keep loading both ways.
    let json = r#"{"id":"00000000-0000-0000-0000-000000000000","title":"A",
        "createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z","groupId":"x"}"#;
    let t: Task = serde_json::from_str(json).unwrap();
    assert_eq!(t.priority, Priority::Low);
    assert_eq!(t.status, Status::Pending);
    assert!(t.tags.is_empty() && t.subtasks.is_empty() && t.reminder.is_none());
}

#[test]
fn status_next_cycles() {
    assert_eq!(Status::Pending.next(), Status::InProgress);
    assert_eq!(Status::InProgress.next(), Status::Completed);
    assert_eq!(Status::Completed.next(), Status::Pending);
}

#[test]
fn settings_default_to_dark_theme() {
    let s: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(s, Settings::default());
    assert_eq!(s.theme, Theme::Dark);
}
