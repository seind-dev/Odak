use super::*;
use chrono::{Duration, TimeZone};

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap()
}

fn draft(title: &str) -> TaskDraft {
    TaskDraft { title: title.into(), ..Default::default() }
}

fn titles(d: &Data) -> String {
    d.tasks.iter().map(|t| t.title.as_str()).collect()
}

#[test]
fn add_trims_title_and_uses_defaults() {
    let mut d = Data::default();
    let id = d.add_task(draft("  Süt al  "), t0()).unwrap();
    let t = d.task(id).unwrap();
    assert_eq!(t.title, "Süt al");
    assert_eq!((t.priority, t.status), (Priority::Low, Status::Pending));
    assert_eq!((t.created_at, t.updated_at), (t0(), t0()));
}

#[test]
fn add_rejects_blank_title() {
    let mut d = Data::default();
    assert_eq!(d.add_task(draft("   "), t0()), Err(DataError::EmptyTitle));
    assert!(d.tasks.is_empty());
}

#[test]
fn add_appends_with_increasing_order() {
    let mut d = Data::default();
    d.add_task(draft("a"), t0()).unwrap();
    d.add_task(draft("b"), t0()).unwrap();
    assert_eq!(d.tasks.iter().map(|t| t.order).collect::<Vec<_>>(), vec![0, 1]);
    assert_eq!(titles(&d), "ab");
}

#[test]
fn add_cleans_tags_and_creates_enabled_reminder() {
    let mut d = Data::default();
    let at = t0() + Duration::hours(1);
    let id = d
        .add_task(
            TaskDraft {
                title: "x".into(),
                tags: vec![" iş ".into(), "".into(), "iş".into(), "ev".into()],
                reminder: Some((at, Repeat::Daily)),
                ..Default::default()
            },
            t0(),
        )
        .unwrap();
    let t = d.task(id).unwrap();
    assert_eq!(t.tags, vec!["iş", "ev"]);
    let r = t.reminder.as_ref().unwrap();
    assert!(r.enabled);
    assert_eq!((r.date_time, r.next_trigger, r.repeat), (at, at, Repeat::Daily));
}

#[test]
fn update_changes_fields_and_keeps_identity() {
    let mut d = Data::default();
    let id = d.add_task(draft("a"), t0()).unwrap();
    let later = t0() + Duration::minutes(5);
    let edit = TaskDraft {
        title: "b".into(),
        priority: Priority::High,
        status: Status::Completed,
        ..Default::default()
    };
    d.update_task(id, edit, later).unwrap();
    let t = d.task(id).unwrap();
    assert_eq!((t.title.as_str(), t.priority, t.status), ("b", Priority::High, Status::Completed));
    assert_eq!((t.created_at, t.updated_at), (t0(), later));
}

#[test]
fn update_with_unchanged_reminder_keeps_its_state() {
    let mut d = Data::default();
    let at = t0() + Duration::hours(1);
    let id = d
        .add_task(TaskDraft { title: "a".into(), reminder: Some((at, Repeat::Weekly)), ..Default::default() }, t0())
        .unwrap();
    // A weekly reminder that already fired once: next_trigger moved, date_time did not.
    let next = at + Duration::weeks(1);
    d.tasks[0].reminder.as_mut().unwrap().next_trigger = next;
    let edit = TaskDraft::from_task(&d.tasks[0]);
    assert_eq!(edit.reminder, Some((next, Repeat::Weekly)));
    d.update_task(id, edit, t0()).unwrap();
    assert_eq!(d.tasks[0].reminder.as_ref().unwrap().date_time, at);
}

#[test]
fn unknown_ids_fail_with_not_found() {
    let mut d = Data::default();
    let missing = Uuid::new_v4();
    assert_eq!(d.update_task(missing, draft("a"), t0()), Err(DataError::NotFound));
    assert_eq!(d.delete_task(missing), Err(DataError::NotFound));
    assert_eq!(d.set_status(missing, Status::Completed, t0()), Err(DataError::NotFound));
}

#[test]
fn delete_removes_task() {
    let mut d = Data::default();
    let id = d.add_task(draft("a"), t0()).unwrap();
    d.delete_task(id).unwrap();
    assert!(d.task(id).is_none());
}

#[test]
fn set_status_touches_updated_at_only_on_change() {
    let mut d = Data::default();
    let id = d.add_task(draft("a"), t0()).unwrap();
    let later = t0() + Duration::minutes(1);
    d.set_status(id, Status::Pending, later).unwrap();
    assert_eq!(d.tasks[0].updated_at, t0());
    d.set_status(id, Status::InProgress, later).unwrap();
    assert_eq!((d.tasks[0].status, d.tasks[0].updated_at), (Status::InProgress, later));
}

#[test]
fn move_to_takes_the_target_position_in_both_directions() {
    let mut d = Data::default();
    let ids: Vec<Uuid> = ["a", "b", "c", "d"].iter().map(|t| d.add_task(draft(t), t0()).unwrap()).collect();
    d.move_to(ids[0], ids[3]).unwrap();
    assert_eq!(titles(&d), "bcda");
    d.move_to(ids[0], ids[1]).unwrap();
    assert_eq!(titles(&d), "abcd");
    assert_eq!(d.tasks.iter().map(|t| t.order).collect::<Vec<_>>(), vec![0, 1, 2, 3]);
}

#[test]
fn fire_due_reminders_returns_due_tasks_and_advances_them() {
    let mut d = Data::default();
    let once = d
        .add_task(TaskDraft { title: "once".into(), reminder: Some((t0(), Repeat::Once)), ..Default::default() }, t0())
        .unwrap();
    let later_at = t0() + Duration::hours(2);
    let later = d
        .add_task(TaskDraft { title: "later".into(), reminder: Some((later_at, Repeat::Once)), ..Default::default() }, t0())
        .unwrap();
    let now = t0() + Duration::seconds(10);
    assert!(d.has_due_reminders(now));
    let fired = d.fire_due_reminders(now);
    assert_eq!(fired.iter().map(|t| t.id).collect::<Vec<_>>(), vec![once]);
    assert!(!d.task(once).unwrap().reminder.as_ref().unwrap().enabled);
    assert!(d.task(later).unwrap().reminder.as_ref().unwrap().enabled);
    assert!(!d.has_due_reminders(now));
}

use crate::model::NoticeKind;

fn notice(title: &str) -> Notice {
    Notice::new(NoticeKind::Reminder, title, "", t0())
}

#[test]
fn notices_are_newest_first_and_capped() {
    let mut d = Data::default();
    for i in 0..(MAX_NOTICES + 5) {
        d.record_notice(notice(&i.to_string()));
    }
    assert_eq!(d.notices.len(), MAX_NOTICES);
    assert_eq!(d.notices[0].title, (MAX_NOTICES + 4).to_string());
    assert_eq!(d.notices.last().unwrap().title, "5");
}

#[test]
fn notices_read_state_and_clear() {
    let mut d = Data::default();
    d.record_notice(notice("a"));
    d.record_notice(notice("b"));
    assert_eq!(d.unread_notices(), 2);
    let id = d.notices[1].id;
    d.mark_notice_read(id);
    assert_eq!(d.unread_notices(), 1);
    assert!(d.notices[1].read);
    d.mark_all_notices_read();
    assert_eq!(d.unread_notices(), 0);
    d.clear_notices();
    assert!(d.notices.is_empty());
}

// ----- sync: upload queue, account binding, merge -----

fn account(n: u128) -> Profile {
    Profile { id: Uuid::from_u128(n), username: format!("u{n}"), display_name: String::new(), avatar_url: None }
}

/// Signed in as account 1 on a device that belongs to it.
fn bound() -> Data {
    Data { account: Some(account(1)), last_account: Some(Uuid::from_u128(1)), ..Default::default() }
}

/// Runs `f` the way `AppState::mutate` does.
fn tracked(d: &mut Data, f: impl FnOnce(&mut Data)) -> bool {
    let before = d.tasks.clone();
    f(d);
    d.track_changes(&before, t0() + Duration::hours(1))
}

#[test]
fn nothing_is_tracked_before_the_first_sign_in() {
    let mut d = Data::default();
    assert!(!tracked(&mut d, |d| {
        d.add_task(draft("a"), t0()).unwrap();
    }));
    assert!(d.pending.is_empty());
}

#[test]
fn edits_queue_one_upsert_per_task() {
    let mut d = bound();
    let mut id = Uuid::nil();
    tracked(&mut d, |d| id = d.add_task(draft("a"), t0()).unwrap());
    tracked(&mut d, |d| d.set_status(id, Status::Completed, t0()).unwrap());
    assert_eq!(d.pending, vec![PendingOp::Upsert(id)]);
    assert!(!tracked(&mut d, |_| {}), "no change, nothing queued");
}

#[test]
fn deleting_an_unsent_task_just_drops_its_upload() {
    let mut d = bound();
    let mut id = Uuid::nil();
    tracked(&mut d, |d| id = d.add_task(draft("a"), t0()).unwrap());
    tracked(&mut d, |d| d.delete_task(id).unwrap());
    assert!(d.pending.is_empty());
}

#[test]
fn deleting_an_uploaded_task_queues_a_delete() {
    let mut d = bound();
    let id = d.add_task(draft("a"), t0()).unwrap();
    d.remote_ids.push(id);
    tracked(&mut d, |d| d.delete_task(id).unwrap());
    assert_eq!(d.pending, vec![PendingOp::Delete(id)]);
}

#[test]
fn changes_that_keep_updated_at_get_a_newer_one() {
    let mut d = bound();
    let a = d.add_task(draft("a"), t0()).unwrap();
    let b = d.add_task(draft("b"), t0()).unwrap();
    tracked(&mut d, |d| d.move_to(b, a).unwrap());
    assert_eq!(d.pending.len(), 2);
    assert!(d.tasks.iter().all(|t| t.updated_at > t0()));
}

#[test]
fn first_account_uploads_every_local_task() {
    let mut d = Data::default();
    let a = d.add_task(draft("a"), t0()).unwrap();
    d.account = Some(account(1));
    d.bind_account(Uuid::from_u128(1));
    assert_eq!((d.last_account, d.pending.clone()), (Some(Uuid::from_u128(1)), vec![PendingOp::Upsert(a)]));
    assert!(!d.needs_account_choice());
}

#[test]
fn same_account_carries_on() {
    let mut d = bound();
    d.pending.push(PendingOp::Delete(Uuid::from_u128(9)));
    d.bind_account(Uuid::from_u128(1));
    assert_eq!(d.pending, vec![PendingOp::Delete(Uuid::from_u128(9))]);
}

#[test]
fn another_account_asks_about_local_tasks() {
    let mut d = bound();
    let a = d.add_task(draft("a"), t0()).unwrap();
    d.remote_ids.push(a);
    d.account = Some(account(2));
    d.bind_account(Uuid::from_u128(2));
    assert!(d.needs_account_choice());

    let mut copy = d.clone();
    copy.adopt_local_tasks(true);
    let task = &copy.tasks[0];
    assert_ne!(task.id, a, "copies get new ids");
    assert_eq!((copy.last_account, copy.pending.clone()), (Some(Uuid::from_u128(2)), vec![PendingOp::Upsert(task.id)]));
    assert!(copy.remote_ids.is_empty() && !copy.needs_account_choice());

    d.adopt_local_tasks(false);
    assert!(d.tasks.is_empty() && d.pending.is_empty() && !d.needs_account_choice());
}

#[test]
fn another_account_without_local_tasks_just_switches() {
    let mut d = bound();
    d.remote_ids.push(Uuid::from_u128(9));
    d.account = Some(account(2));
    d.bind_account(Uuid::from_u128(2));
    assert!(!d.needs_account_choice());
    assert!(d.remote_ids.is_empty());
}

fn remote(d: &Data, id: Uuid, title: &str) -> Task {
    Task { title: title.into(), ..d.task(id).unwrap().clone() }
}

#[test]
fn finished_uploads_leave_the_queue_unless_changed_meanwhile() {
    let mut d = bound();
    let a = d.add_task(draft("a"), t0()).unwrap();
    let b = d.add_task(draft("b"), t0()).unwrap();
    d.pending = vec![PendingOp::Upsert(a), PendingOp::Upsert(b)];
    let sent = [a, b].map(|id| Sent { op: PendingOp::Upsert(id), snapshot: d.task(id).cloned(), uploaded: true });
    d.set_status(b, Status::Completed, t0() + Duration::minutes(1)).unwrap(); // edited during the turn
    let server = vec![remote(&d, a, "a"), Task { status: Status::Pending, ..remote(&d, b, "b") }];
    d.merge_sync(&sent, Some(server), t0());
    assert_eq!(d.pending, vec![PendingOp::Upsert(b)]);
    assert_eq!(d.task(b).unwrap().status, Status::Completed, "local edit kept while it waits");
    assert_eq!(d.remote_ids.len(), 2);
    assert_eq!(d.last_sync, Some(t0()));
}

#[test]
fn pull_applies_server_changes_and_deletions() {
    let mut d = bound();
    let edited = d.add_task(draft("old"), t0()).unwrap();
    let deleted = d.add_task(draft("deleted there"), t0()).unwrap();
    let local = d.add_task(draft("never sent"), t0()).unwrap();
    d.remote_ids = vec![edited, deleted];
    let mut new = remote(&d, edited, "new from another device");
    new.id = Uuid::from_u128(77);
    let server = vec![remote(&d, edited, "new"), new];
    d.merge_sync(&[], Some(server), t0());
    assert_eq!(d.task(edited).unwrap().title, "new");
    assert!(d.task(deleted).is_none());
    assert!(d.task(local).is_some(), "local-only tasks stay");
    assert!(d.task(Uuid::from_u128(77)).is_some());
}

#[test]
fn a_task_deleted_during_its_upload_is_deleted_on_the_server_next() {
    let mut d = bound();
    let a = d.add_task(draft("a"), t0()).unwrap();
    let sent = [Sent { op: PendingOp::Upsert(a), snapshot: d.task(a).cloned(), uploaded: true }];
    let server = vec![remote(&d, a, "a")];
    d.delete_task(a).unwrap();
    d.pending.clear(); // what track_changes did: the unsent upload was dropped
    d.merge_sync(&sent, Some(server), t0());
    assert_eq!(d.pending, vec![PendingOp::Delete(a)]);
    assert!(d.task(a).is_none(), "not brought back by the pull");
}

#[test]
fn a_failed_pull_keeps_local_tasks() {
    let mut d = bound();
    let a = d.add_task(draft("a"), t0()).unwrap();
    d.remote_ids.push(a);
    d.pending.push(PendingOp::Delete(Uuid::from_u128(5)));
    let sent = [Sent { op: PendingOp::Delete(Uuid::from_u128(5)), snapshot: None, uploaded: false }];
    d.merge_sync(&sent, None, t0());
    assert!(d.pending.is_empty());
    assert!(d.task(a).is_some());
    assert_eq!(d.last_sync, None);
}

#[test]
fn old_data_files_load_without_sync_fields() {
    let d: Data = serde_json::from_str(r#"{"version":1,"tasks":[],"settings":{},"notices":[]}"#).unwrap();
    assert!(d.last_account.is_none() && d.pending.is_empty() && d.account.is_none());
    let json = serde_json::to_string(&bound()).unwrap();
    assert!(json.contains("\"lastAccount\"") && json.contains("\"remoteIds\""));
}


// ----- groups: drafts, assignment notices, reminders, signing out -----

fn owned_by(d: &mut Data, title: &str, owner: u128, group: Option<u128>, assignee: Option<u128>) -> Uuid {
    let id = d.add_task(draft(title), t0()).unwrap();
    let t = d.tasks.iter_mut().find(|t| t.id == id).unwrap();
    t.owner_id = Some(Uuid::from_u128(owner));
    t.group_id = group.map(Uuid::from_u128);
    t.assignee_id = assignee.map(Uuid::from_u128);
    id
}

#[test]
fn drafts_keep_group_and_drop_an_assignee_without_one() {
    let mut d = Data::default();
    let g = Uuid::from_u128(50);
    let a = Uuid::from_u128(2);
    let id = d.add_task(TaskDraft { group_id: Some(g), assignee_id: Some(a), ..draft("x") }, t0()).unwrap();
    assert_eq!((d.task(id).unwrap().group_id, d.task(id).unwrap().assignee_id), (Some(g), Some(a)));
    d.update_task(id, TaskDraft { group_id: None, assignee_id: Some(a), ..draft("x") }, t0()).unwrap();
    assert_eq!((d.task(id).unwrap().group_id, d.task(id).unwrap().assignee_id), (None, None));
}

#[test]
fn tasks_newly_assigned_by_others_are_reported_after_the_first_pull() {
    let mut d = bound();
    let theirs = owned_by(&mut d, "theirs", 2, Some(50), None);
    let own = owned_by(&mut d, "own", 1, Some(50), None);
    let already = owned_by(&mut d, "already", 2, Some(50), Some(1));
    let assign = |t: &Task| Task { assignee_id: Some(Uuid::from_u128(1)), ..t.clone() };
    let server: Vec<Task> = d.tasks.iter().map(assign).collect();

    let mut first = d.clone();
    assert!(first.merge_sync(&[], Some(server.clone()), t0()).is_empty(), "first pull: no flood of notices");

    d.last_sync = Some(t0());
    let assigned = d.merge_sync(&[], Some(server), t0());
    assert_eq!(assigned, vec![theirs], "own tasks and old assignments are not news");
    assert!(!assigned.contains(&own) && !assigned.contains(&already));
}

#[test]
fn reminders_ring_only_for_the_owner_and_the_assignee() {
    let mut d = bound();
    let due = |d: &mut Data, id: Uuid| {
        let t = d.tasks.iter_mut().find(|t| t.id == id).unwrap();
        t.reminder = Some(Reminder { date_time: t0(), repeat: Repeat::Once, enabled: true, next_trigger: t0(), snoozed_until: None });
    };
    let mine = owned_by(&mut d, "mine", 1, None, None);
    let assigned = owned_by(&mut d, "assigned", 2, Some(50), Some(1));
    let others = owned_by(&mut d, "others", 2, Some(50), Some(3));
    for id in [mine, assigned, others] {
        due(&mut d, id);
    }
    let fired: Vec<Uuid> = d.fire_due_reminders(t0()).iter().map(|t| t.id).collect();
    assert_eq!(fired, vec![mine, assigned]);

    let mut signed_out = Data::default();
    let id = signed_out.add_task(draft("local"), t0()).unwrap();
    due(&mut signed_out, id);
    assert!(signed_out.has_due_reminders(t0()), "without an account every reminder rings");
}

#[test]
fn signing_out_keeps_own_tasks_and_forgets_the_rest() {
    let mut d = bound();
    let mine = owned_by(&mut d, "mine", 1, Some(50), None);
    let unsent = d.add_task(draft("unsent"), t0()).unwrap();
    let theirs = owned_by(&mut d, "theirs", 2, Some(50), None);
    d.remote_ids = vec![mine, theirs];
    d.pending = vec![PendingOp::Upsert(theirs), PendingOp::Upsert(unsent)];
    d.groups.push(Group { id: Uuid::from_u128(50), name: "Ekip".into(), owner_id: Uuid::from_u128(2), members: vec![] });
    d.profiles.push(account(2));
    d.forget_account();
    assert!(d.task(mine).is_some() && d.task(unsent).is_some() && d.task(theirs).is_none());
    assert_eq!(d.pending, vec![PendingOp::Upsert(unsent)]);
    assert_eq!(d.remote_ids, vec![mine]);
    assert!(d.groups.is_empty() && d.profiles.is_empty() && d.account.is_none());
    assert_eq!(d.last_account, Some(Uuid::from_u128(1)), "same account can carry on later");
}

#[test]
fn copying_to_another_account_leaves_other_peoples_tasks_behind() {
    let mut d = bound();
    owned_by(&mut d, "mine", 1, None, None);
    owned_by(&mut d, "theirs", 2, Some(50), None);
    d.account = Some(account(3));
    d.adopt_local_tasks(true);
    assert_eq!(titles(&d), "mine");
}

#[test]
fn profile_lookup_includes_the_account() {
    let mut d = bound();
    d.profiles.push(account(2));
    assert_eq!(d.profile(Uuid::from_u128(1)).unwrap().username, "u1");
    assert_eq!(d.profile(Uuid::from_u128(2)).unwrap().username, "u2");
    assert!(d.profile(Uuid::from_u128(3)).is_none());
}

// ----- repeating tasks, snooze, undo, reschedule -----

#[test]
fn completing_a_repeating_task_moves_it_to_its_next_date() {
    let mut d = Data::default();
    let id = d.add_task(TaskDraft { recurrence: Some(Recurrence::Day), due_date: Some(t0()), ..draft("Spor") }, t0()).unwrap();
    {
        let t = d.tasks.iter_mut().find(|t| t.id == id).unwrap();
        t.subtasks.push(SubTask { id: Uuid::from_u128(1), title: "ısın".into(), completed: true });
        t.reminder = Some(Reminder { date_time: t0(), repeat: Repeat::Once, enabled: false, next_trigger: t0(), snoozed_until: None });
    }
    d.set_status(id, Status::Completed, t0()).unwrap();
    let t = d.task(id).unwrap();
    assert_eq!(t.status, Status::Pending);
    assert_eq!(t.due_date, Some(t0() + Duration::days(1)));
    assert!(!t.subtasks[0].completed);
    let r = t.reminder.as_ref().unwrap();
    assert!(r.enabled && r.next_trigger == t0() + Duration::days(1), "one-time reminder moves with the date");
}

#[test]
fn saving_a_repeating_task_as_completed_also_moves_it() {
    let mut d = Data::default();
    let id = d.add_task(TaskDraft { recurrence: Some(Recurrence::Day), due_date: Some(t0()), ..draft("Spor") }, t0()).unwrap();
    let edit = TaskDraft { status: Status::Completed, ..TaskDraft::from_task(d.task(id).unwrap()) };
    d.update_task(id, edit, t0()).unwrap();
    assert_eq!(d.task(id).unwrap().status, Status::Pending);
    assert_eq!(d.task(id).unwrap().due_date, Some(t0() + Duration::days(1)));
}

#[test]
fn snoozed_reminders_ring_again_once() {
    let mut d = Data::default();
    let id = d.add_task(draft("a"), t0()).unwrap();
    d.snooze(id, t0() + Duration::minutes(10), t0()).unwrap();
    assert!(!d.has_due_reminders(t0() + Duration::minutes(9)));
    let fired = d.fire_due_reminders(t0() + Duration::minutes(10));
    assert_eq!(fired.len(), 1);
    assert!(d.task(id).unwrap().reminder.as_ref().unwrap().snoozed_until.is_none());
    assert!(!d.has_due_reminders(t0() + Duration::hours(1)));
}

#[test]
fn undo_puts_a_deleted_task_back_without_deleting_it_on_the_server() {
    let mut d = bound();
    let a = d.add_task(draft("a"), t0()).unwrap();
    let b = d.add_task(draft("b"), t0()).unwrap();
    d.remote_ids = vec![a, b];
    let snapshot = d.task(a).unwrap().clone();
    tracked(&mut d, |d| d.delete_task(a).unwrap());
    assert_eq!(d.pending, vec![PendingOp::Delete(a)]);
    tracked(&mut d, |d| d.restore_task(snapshot, 0));
    assert_eq!(titles(&d), "ab");
    assert!(!d.pending.contains(&PendingOp::Delete(a)));
}

#[test]
fn undo_restores_a_changed_task() {
    let mut d = Data::default();
    let id = d.add_task(TaskDraft { recurrence: Some(Recurrence::Day), due_date: Some(t0()), ..draft("Spor") }, t0()).unwrap();
    let before = d.task(id).unwrap().clone();
    d.set_status(id, Status::Completed, t0()).unwrap();
    d.restore_task(before.clone(), 0);
    assert_eq!(d.task(id), Some(&before));
    assert_eq!(d.tasks.len(), 1);
}

#[test]
fn rescheduling_keeps_the_time_of_day() {
    let mut d = Data::default();
    let id = d.add_task(TaskDraft { due_date: Some(t0()), ..draft("a") }, t0()).unwrap();
    let day = t0().with_timezone(&chrono::Local).date_naive() + chrono::Days::new(3);
    d.reschedule(id, day, t0()).unwrap();
    assert_eq!(d.task(id).unwrap().due_date, Some(t0() + Duration::days(3)));
}
