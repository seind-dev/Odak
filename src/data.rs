//! The persisted document and every operation on it. Pure: no GPUI, no I/O.

use crate::model::{Notice, Priority, Profile, Reminder, Repeat, Settings, Status, SubTask, Task};
use crate::reminders;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

pub const DATA_VERSION: u32 = 1;

/// Notification history keeps at most this many notices (newest first).
pub const MAX_NOTICES: usize = 100;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Data {
    pub version: u32,
    /// Always sorted by `Task::order`: iteration order is display order.
    pub tasks: Vec<Task>,
    pub settings: Settings,
    /// Pop-ups shown so far, newest first (see `record_notice`).
    pub notices: Vec<Notice>,
    /// The signed-in account. Kept when the session expires, so the app can ask to sign in again.
    pub account: Option<Profile>,
}

impl Default for Data {
    fn default() -> Self {
        Data { version: DATA_VERSION, tasks: Vec::new(), settings: Settings::default(), notices: Vec::new(), account: None }
    }
}

/// What the task form edits. `reminder` is (next trigger, repeat).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TaskDraft {
    pub title: String,
    pub description: String,
    pub priority: Priority,
    pub status: Status,
    pub due_date: Option<DateTime<Utc>>,
    pub reminder: Option<(DateTime<Utc>, Repeat)>,
    pub tags: Vec<String>,
    pub subtasks: Vec<SubTask>,
}

impl TaskDraft {
    pub fn from_task(t: &Task) -> Self {
        TaskDraft {
            title: t.title.clone(),
            description: t.description.clone(),
            priority: t.priority,
            status: t.status,
            due_date: t.due_date,
            reminder: t.reminder.as_ref().filter(|r| r.enabled).map(|r| (r.next_trigger, r.repeat)),
            tags: t.tags.clone(),
            subtasks: t.subtasks.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataError {
    EmptyTitle,
    NotFound,
}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DataError::EmptyTitle => "Başlık boş olamaz",
            DataError::NotFound => "Görev bulunamadı",
        })
    }
}

impl Data {
    pub fn task(&self, id: Uuid) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    fn task_mut(&mut self, id: Uuid) -> Result<&mut Task, DataError> {
        self.tasks.iter_mut().find(|t| t.id == id).ok_or(DataError::NotFound)
    }

    /// Adds a task at the end of the list and returns its id.
    pub fn add_task(&mut self, draft: TaskDraft, now: DateTime<Utc>) -> Result<Uuid, DataError> {
        let title = clean_title(&draft.title)?;
        let order = self.tasks.iter().map(|t| t.order + 1).max().unwrap_or(0);
        let task = Task {
            id: Uuid::new_v4(),
            title,
            description: draft.description,
            priority: draft.priority,
            status: draft.status,
            reminder: draft.reminder.map(|(at, repeat)| new_reminder(at, repeat)),
            subtasks: draft.subtasks,
            tags: clean_tags(draft.tags),
            order,
            due_date: draft.due_date,
            created_at: now,
            updated_at: now,
        };
        let id = task.id;
        self.tasks.push(task);
        Ok(id)
    }

    pub fn update_task(&mut self, id: Uuid, draft: TaskDraft, now: DateTime<Utc>) -> Result<(), DataError> {
        let title = clean_title(&draft.title)?;
        let task = self.task_mut(id)?;
        let reminder = match draft.reminder {
            None => None,
            Some((at, repeat)) => match &task.reminder {
                // Unchanged in the form: keep the original first trigger.
                Some(r) if r.enabled && r.next_trigger == at && r.repeat == repeat => Some(r.clone()),
                _ => Some(new_reminder(at, repeat)),
            },
        };
        task.title = title;
        task.description = draft.description;
        task.priority = draft.priority;
        task.status = draft.status;
        task.due_date = draft.due_date;
        task.reminder = reminder;
        task.tags = clean_tags(draft.tags);
        task.subtasks = draft.subtasks;
        task.updated_at = now;
        Ok(())
    }

    pub fn set_status(&mut self, id: Uuid, status: Status, now: DateTime<Utc>) -> Result<(), DataError> {
        let task = self.task_mut(id)?;
        if task.status != status {
            task.status = status;
            task.updated_at = now;
        }
        Ok(())
    }

    pub fn delete_task(&mut self, id: Uuid) -> Result<(), DataError> {
        let before = self.tasks.len();
        self.tasks.retain(|t| t.id != id);
        if self.tasks.len() == before { Err(DataError::NotFound) } else { Ok(()) }
    }

    /// Drag & drop: `id` takes the position of `target` and `order` is renumbered.
    pub fn move_to(&mut self, id: Uuid, target: Uuid) -> Result<(), DataError> {
        let from = self.tasks.iter().position(|t| t.id == id).ok_or(DataError::NotFound)?;
        let to = self.tasks.iter().position(|t| t.id == target).ok_or(DataError::NotFound)?;
        let task = self.tasks.remove(from);
        self.tasks.insert(to, task);
        for (i, t) in self.tasks.iter_mut().enumerate() {
            t.order = i as i64;
        }
        Ok(())
    }

    /// Adds a notice at the top of the history, dropping the oldest beyond `MAX_NOTICES`.
    pub fn record_notice(&mut self, notice: Notice) {
        self.notices.insert(0, notice);
        self.notices.truncate(MAX_NOTICES);
    }

    pub fn mark_notice_read(&mut self, id: Uuid) {
        if let Some(notice) = self.notices.iter_mut().find(|n| n.id == id) {
            notice.read = true;
        }
    }

    pub fn mark_all_notices_read(&mut self) {
        for notice in &mut self.notices {
            notice.read = true;
        }
    }

    pub fn clear_notices(&mut self) {
        self.notices.clear();
    }

    pub fn unread_notices(&self) -> usize {
        self.notices.iter().filter(|n| !n.read).count()
    }

    /// Cheap check so the reminder loop only saves when something fires.
    pub fn has_due_reminders(&self, now: DateTime<Utc>) -> bool {
        !reminders::due_now(&self.tasks, now).is_empty()
    }

    /// Advances every due reminder and returns snapshots of the tasks to notify about.
    pub fn fire_due_reminders(&mut self, now: DateTime<Utc>) -> Vec<Task> {
        let due = reminders::due_now(&self.tasks, now);
        let mut fired = Vec::new();
        for task in self.tasks.iter_mut().filter(|t| due.contains(&t.id)) {
            fired.push(task.clone());
            if let Some(reminder) = task.reminder.as_mut() {
                reminders::advance(reminder, now);
            }
        }
        fired
    }
}

fn clean_title(title: &str) -> Result<String, DataError> {
    let title = title.trim();
    if title.is_empty() { Err(DataError::EmptyTitle) } else { Ok(title.to_string()) }
}

/// Trims tags and drops empty ones and duplicates (first occurrence wins).
fn clean_tags(tags: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        let tag = tag.trim().to_string();
        if !tag.is_empty() && !out.contains(&tag) {
            out.push(tag);
        }
    }
    out
}

fn new_reminder(at: DateTime<Utc>, repeat: Repeat) -> Reminder {
    Reminder { date_time: at, repeat, enabled: true, next_trigger: at }
}

#[cfg(test)]
#[path = "data_tests.rs"]
mod tests;
