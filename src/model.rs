//! Persisted data types, serialized as camelCase JSON in data.json.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    High,
    Medium,
    #[default]
    Low,
}

impl Priority {
    pub const ALL: [Priority; 3] = [Priority::High, Priority::Medium, Priority::Low];

    pub fn label(self) -> &'static str {
        match self {
            Priority::High => "Yüksek",
            Priority::Medium => "Orta",
            Priority::Low => "Düşük",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Pending,
    InProgress,
    Completed,
}

impl Status {
    pub const ALL: [Status; 3] = [Status::Pending, Status::InProgress, Status::Completed];

    pub fn label(self) -> &'static str {
        match self {
            Status::Pending => "Beklemede",
            Status::InProgress => "Devam Ediyor",
            Status::Completed => "Tamamlandı",
        }
    }

    /// Status after clicking the status dot on a task card.
    pub fn next(self) -> Status {
        match self {
            Status::Pending => Status::InProgress,
            Status::InProgress => Status::Completed,
            Status::Completed => Status::Pending,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Repeat {
    #[default]
    Once,
    Daily,
    Weekly,
}

impl Repeat {
    pub const ALL: [Repeat; 3] = [Repeat::Once, Repeat::Daily, Repeat::Weekly];

    pub fn label(self) -> &'static str {
        match self {
            Repeat::Once => "Bir kez",
            Repeat::Daily => "Günlük",
            Repeat::Weekly => "Haftalık",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
    /// First trigger as chosen by the user.
    pub date_time: DateTime<Utc>,
    #[serde(default)]
    pub repeat: Repeat,
    #[serde(default)]
    pub enabled: bool,
    pub next_trigger: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubTask {
    pub id: Uuid,
    pub title: String,
    #[serde(default)]
    pub completed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    Reminder,
    Alert,
    Update,
    /// Something another member did: an assignment or a comment.
    Shared,
}

/// A notification that was shown as a pop-up, kept for the Bildirimler page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    pub id: Uuid,
    pub kind: NoticeKind,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub priority: Option<Priority>,
    /// Task the notice is about; clicking the notice opens it.
    #[serde(default)]
    pub task_id: Option<Uuid>,
    pub at: DateTime<Utc>,
    #[serde(default)]
    pub read: bool,
}

impl Notice {
    pub fn new(kind: NoticeKind, title: impl Into<String>, body: impl Into<String>, at: DateTime<Utc>) -> Self {
        Notice { id: Uuid::new_v4(), kind, title: title.into(), body: body.into(), priority: None, task_id: None, at, read: false }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: Uuid,
    pub title: String,
    /// Markdown source, shown as plain text in phase 1. Empty means none.
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub priority: Priority,
    #[serde(default)]
    pub status: Status,
    #[serde(default)]
    pub reminder: Option<Reminder>,
    #[serde(default)]
    pub subtasks: Vec<SubTask>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Position in the list; `Data::tasks` is kept sorted by it.
    #[serde(default)]
    pub order: i64,
    #[serde(default)]
    pub due_date: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Account that owns the task on the server; `None` until it is first synced.
    #[serde(default)]
    pub owner_id: Option<Uuid>,
    #[serde(default)]
    pub group_id: Option<Uuid>,
    #[serde(default)]
    pub assignee_id: Option<Uuid>,
}

/// An entry in the upload queue (`Data::pending`). A task has at most one `Upsert` waiting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", content = "id", rename_all = "snake_case")]
pub enum PendingOp {
    Upsert(Uuid),
    Delete(Uuid),
}

impl PendingOp {
    pub fn id(self) -> Uuid {
        match self {
            PendingOp::Upsert(id) | PendingOp::Delete(id) => id,
        }
    }
}

/// A shared group and the ids of its members (the owner included).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: Uuid,
    pub name: String,
    pub owner_id: Uuid,
    #[serde(default)]
    pub members: Vec<Uuid>,
}

/// A user as shown in the app, from the `profiles` table (display only, never for access checks).
/// The aliases accept the snake_case the REST API sends.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: Uuid,
    #[serde(default)]
    pub username: String,
    #[serde(default, alias = "display_name")]
    pub display_name: String,
    #[serde(default, alias = "avatar_url")]
    pub avatar_url: Option<String>,
}

impl Profile {
    pub fn name(&self) -> &str {
        if self.display_name.is_empty() { &self.username } else { &self.display_name }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub auto_launch: bool,
    pub theme: Theme,
    pub start_minimized: bool,
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
