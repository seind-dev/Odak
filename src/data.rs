//! The persisted document and every operation on it. Pure: no GPUI, no I/O.

use crate::model::{Group, Notice, PendingOp, Priority, Profile, Reminder, Repeat, Settings, Status, SubTask, Task};
use crate::reminders;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use uuid::Uuid;

pub const DATA_VERSION: u32 = 1;

/// Notification history keeps at most this many notices (newest first).
pub const MAX_NOTICES: usize = 100;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Data {
    pub version: u32,
    /// Always sorted by `Task::order`: iteration order is display order.
    pub tasks: Vec<Task>,
    pub settings: Settings,
    /// Pop-ups shown so far, newest first (see `record_notice`).
    pub notices: Vec<Notice>,
    /// The signed-in account. Kept when the session expires, so the app can ask to sign in again.
    pub account: Option<Profile>,
    /// The account this device's tasks belong to. Stays after signing out, so signing back in with
    /// the same account carries on where it stopped. `None`: never signed in, nothing is tracked.
    pub last_account: Option<Uuid>,
    /// Changes waiting to be uploaded, oldest first.
    pub pending: Vec<PendingOp>,
    /// Tasks known to exist on the server (as of the last pull or upload).
    pub remote_ids: Vec<Uuid>,
    pub last_sync: Option<DateTime<Utc>>,
    /// Groups the account belongs to, as of the last sync.
    pub groups: Vec<Group>,
    /// People who share a group with the account (names and avatars), as of the last sync.
    pub profiles: Vec<Profile>,
}

impl Default for Data {
    fn default() -> Self {
        Data {
            version: DATA_VERSION,
            tasks: Vec::new(),
            settings: Settings::default(),
            notices: Vec::new(),
            account: None,
            last_account: None,
            pending: Vec::new(),
            remote_ids: Vec::new(),
            last_sync: None,
            groups: Vec::new(),
            profiles: Vec::new(),
        }
    }
}

/// An upload queue entry that a sync turn finished with (sent, found stale or rejected).
#[derive(Clone, Debug, PartialEq)]
pub struct Sent {
    pub op: PendingOp,
    /// The task as it was sent (`Upsert` only).
    pub snapshot: Option<Task>,
    /// The server has the task now (an accepted or stale `Upsert`).
    pub uploaded: bool,
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
    pub group_id: Option<Uuid>,
    pub assignee_id: Option<Uuid>,
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
            group_id: t.group_id,
            assignee_id: t.assignee_id,
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

    /// The signed-in account's id.
    pub fn me(&self) -> Option<Uuid> {
        self.account.as_ref().map(|a| a.id)
    }

    pub fn group(&self, id: Uuid) -> Option<&Group> {
        self.groups.iter().find(|g| g.id == id)
    }

    /// Name and avatar of a user: the account itself or someone sharing a group with it.
    pub fn profile(&self, id: Uuid) -> Option<&Profile> {
        self.account.iter().chain(&self.profiles).find(|p| p.id == id)
    }

    /// Reminders ring for the task's owner and assignee only; without an account, for every task.
    fn reminds_me(&self, t: &Task) -> bool {
        match self.me() {
            None => true,
            Some(me) => t.owner_id.is_none_or(|owner| owner == me) || t.assignee_id == Some(me),
        }
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
            owner_id: None,
            group_id: draft.group_id,
            assignee_id: draft.group_id.and(draft.assignee_id),
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
        task.group_id = draft.group_id;
        task.assignee_id = draft.group_id.and(draft.assignee_id);
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
        !self.due_reminders(now).is_empty()
    }

    fn due_reminders(&self, now: DateTime<Utc>) -> Vec<Uuid> {
        let mut due = reminders::due_now(&self.tasks, now);
        due.retain(|id| self.task(*id).is_some_and(|t| self.reminds_me(t)));
        due
    }

    /// Advances every due reminder and returns snapshots of the tasks to notify about.
    pub fn fire_due_reminders(&mut self, now: DateTime<Utc>) -> Vec<Task> {
        let due = self.due_reminders(now);
        let mut fired = Vec::new();
        for task in self.tasks.iter_mut().filter(|t| due.contains(&t.id)) {
            fired.push(task.clone());
            if let Some(reminder) = task.reminder.as_mut() {
                reminders::advance(reminder, now);
            }
        }
        fired
    }

    /// Queues how the tasks changed since `before` for upload, once the device is bound to an account.
    /// A changed task whose `updated_at` did not move gets a newer one, so the server's
    /// last-write-wins accepts it. Returns whether anything was queued.
    pub fn track_changes(&mut self, before: &[Task], now: DateTime<Utc>) -> bool {
        if self.last_account.is_none() {
            return false;
        }
        let old: HashMap<Uuid, &Task> = before.iter().map(|t| (t.id, t)).collect();
        let mut queued = false;
        for task in &mut self.tasks {
            let prev = old.get(&task.id);
            if prev.is_some_and(|p| **p == *task) {
                continue;
            }
            if let Some(prev) = prev
                && task.updated_at <= prev.updated_at
            {
                task.updated_at = now.max(prev.updated_at + chrono::Duration::milliseconds(1));
            }
            queue_upsert(&mut self.pending, task.id);
            queued = true;
        }
        let current: HashSet<Uuid> = self.tasks.iter().map(|t| t.id).collect();
        for gone in before.iter().filter(|t| !current.contains(&t.id)) {
            self.pending.retain(|op| *op != PendingOp::Upsert(gone.id));
            // Never reached the server: nothing to delete there.
            if self.remote_ids.contains(&gone.id) && !self.pending.contains(&PendingOp::Delete(gone.id)) {
                self.pending.push(PendingOp::Delete(gone.id));
            }
            queued = true;
        }
        queued
    }

    /// `id` signed in. The first account on this device gets every local task uploaded; the same
    /// account again just carries on; another account waits for `adopt_local_tasks`, unless there
    /// is nothing local to decide about.
    pub fn bind_account(&mut self, id: Uuid) {
        match self.last_account {
            Some(last) if last == id => {}
            None => {
                self.last_account = Some(id);
                for task in &self.tasks {
                    queue_upsert(&mut self.pending, task.id);
                }
            }
            Some(_) if self.tasks.is_empty() => self.start_over(id),
            Some(_) => {}
        }
    }

    /// The signed-in account differs from the one this device's tasks belong to.
    pub fn needs_account_choice(&self) -> bool {
        matches!((&self.account, self.last_account), (Some(account), Some(last)) if account.id != last)
    }

    /// After signing out: keeps the account's own tasks (and their waiting uploads) for the next
    /// sign-in; other people's group tasks and the group directory go.
    pub fn forget_account(&mut self) {
        let mine = self.me().or(self.last_account);
        self.account = None;
        self.keep_only_tasks_of(mine);
        self.groups.clear();
        self.profiles.clear();
    }

    fn keep_only_tasks_of(&mut self, owner: Option<Uuid>) {
        let others: HashSet<Uuid> =
            self.tasks.iter().filter(|t| t.owner_id.is_some() && t.owner_id != owner).map(|t| t.id).collect();
        self.tasks.retain(|t| !others.contains(&t.id));
        self.pending.retain(|op| !others.contains(&op.id()));
        self.remote_ids.retain(|id| !others.contains(id));
    }

    /// Answers `needs_account_choice`: copy the local tasks into the signed-in account (with new
    /// ids, so they do not collide with the other account's copies) or remove them from this device.
    pub fn adopt_local_tasks(&mut self, copy: bool) {
        let Some(id) = self.me() else { return };
        self.keep_only_tasks_of(self.last_account);
        self.groups.clear();
        self.profiles.clear();
        self.start_over(id);
        if !copy {
            self.tasks.clear();
            return;
        }
        for task in &mut self.tasks {
            task.id = Uuid::new_v4();
            task.owner_id = None;
            task.group_id = None;
            task.assignee_id = None;
            self.pending.push(PendingOp::Upsert(task.id));
        }
    }

    fn start_over(&mut self, account: Uuid) {
        self.last_account = Some(account);
        self.pending.clear();
        self.remote_ids.clear();
        self.last_sync = None;
    }

    /// Applies a sync turn: finished uploads leave the queue (unless the task changed again during
    /// the turn), then the server's tasks replace local ones that have nothing waiting, and tasks
    /// deleted on the server go away here too. `remote` is `None` when the pull did not get through.
    /// Returns the tasks someone else newly assigned to the account (none on the first pull).
    pub fn merge_sync(&mut self, sent: &[Sent], remote: Option<Vec<Task>>, now: DateTime<Utc>) -> Vec<Uuid> {
        for done in sent {
            match done.op {
                PendingOp::Upsert(id) => {
                    let current = self.tasks.iter().find(|t| t.id == id);
                    let gone = current.is_none();
                    if gone || current == done.snapshot.as_ref() {
                        self.pending.retain(|op| *op != done.op);
                    }
                    if done.uploaded {
                        if !self.remote_ids.contains(&id) {
                            self.remote_ids.push(id);
                        }
                        // Deleted here while its upload was on the way: delete it there as well.
                        if gone && !self.pending.contains(&PendingOp::Delete(id)) {
                            self.pending.push(PendingOp::Delete(id));
                        }
                    }
                }
                PendingOp::Delete(id) => {
                    self.pending.retain(|op| *op != done.op);
                    self.remote_ids.retain(|r| *r != id);
                }
            }
        }
        let Some(remote) = remote else { return Vec::new() };
        let me = self.me();
        let first_pull = self.last_sync.is_none();
        let mut assigned = Vec::new();
        let waiting: HashSet<Uuid> = self.pending.iter().map(|op| op.id()).collect();
        let on_server: HashSet<Uuid> = remote.iter().map(|t| t.id).collect();
        let known = &self.remote_ids;
        self.tasks.retain(|t| on_server.contains(&t.id) || waiting.contains(&t.id) || !known.contains(&t.id));
        for task in remote {
            if waiting.contains(&task.id) {
                continue;
            }
            let local = self.tasks.iter().find(|t| t.id == task.id);
            let newly_mine = me.is_some() && task.assignee_id == me && local.is_none_or(|l| l.assignee_id != me);
            if newly_mine && !first_pull && task.owner_id != me {
                assigned.push(task.id);
            }
            match self.tasks.iter_mut().find(|t| t.id == task.id) {
                Some(local) => *local = task,
                None => self.tasks.push(task),
            }
        }
        self.tasks.sort_by_key(|t| t.order);
        self.remote_ids = on_server.into_iter().collect();
        self.remote_ids.sort();
        self.last_sync = Some(now);
        assigned
    }
}

fn queue_upsert(pending: &mut Vec<PendingOp>, id: Uuid) {
    if !pending.contains(&PendingOp::Upsert(id)) {
        pending.push(PendingOp::Upsert(id));
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
