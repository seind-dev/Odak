use super::*;
use crate::model::{Priority, Status};
use chrono::TimeZone;

fn at(h: u32, m: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 1, h, m, 0).unwrap()
}

fn reminder(next: DateTime<Utc>, repeat: Repeat, enabled: bool) -> Reminder {
    Reminder { date_time: next, repeat, enabled, next_trigger: next }
}

fn task_with(reminder: Option<Reminder>) -> Task {
    Task {
        id: Uuid::new_v4(),
        title: "t".into(),
        description: String::new(),
        priority: Priority::Low,
        status: Status::Pending,
        reminder,
        subtasks: vec![],
        tags: vec![],
        order: 0,
        due_date: None,
        created_at: at(0, 0),
        updated_at: at(0, 0),
    }
}

#[test]
fn due_now_includes_only_enabled_triggers_at_or_before_now() {
    let past = task_with(Some(reminder(at(9, 0), Repeat::Once, true)));
    let exact = task_with(Some(reminder(at(10, 0), Repeat::Once, true)));
    let future = task_with(Some(reminder(at(11, 0), Repeat::Once, true)));
    let disabled = task_with(Some(reminder(at(9, 0), Repeat::Once, false)));
    let none = task_with(None);
    let tasks = vec![past.clone(), exact.clone(), future, disabled, none];
    assert_eq!(due_now(&tasks, at(10, 0)), vec![past.id, exact.id]);
}

#[test]
fn advance_disables_one_time_reminder() {
    let mut r = reminder(at(9, 0), Repeat::Once, true);
    advance(&mut r, at(9, 0));
    assert!(!r.enabled);
    assert_eq!(r.next_trigger, at(9, 0));
}

#[test]
fn advance_moves_daily_by_one_day() {
    let mut r = reminder(at(9, 0), Repeat::Daily, true);
    advance(&mut r, at(9, 0));
    assert!(r.enabled);
    assert_eq!(r.next_trigger, at(9, 0) + Duration::days(1));
}

#[test]
fn advance_moves_weekly_by_seven_days() {
    let mut r = reminder(at(9, 0), Repeat::Weekly, true);
    advance(&mut r, at(9, 30));
    assert_eq!(r.next_trigger, at(9, 0) + Duration::weeks(1));
}

#[test]
fn advance_after_long_sleep_jumps_to_first_future_trigger() {
    let mut r = reminder(at(9, 0), Repeat::Daily, true);
    let now = at(9, 0) + Duration::days(3) + Duration::hours(2);
    advance(&mut r, now);
    assert_eq!(r.next_trigger, at(9, 0) + Duration::days(4));
}
