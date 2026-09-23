//! Pure reminder scheduling. The app polls `due_now` every 15 seconds (see main.rs).

use crate::model::{Reminder, Repeat, Task};
use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

/// Ids of tasks whose enabled reminder is due at `now`.
pub fn due_now(tasks: &[Task], now: DateTime<Utc>) -> Vec<Uuid> {
    tasks
        .iter()
        .filter(|t| t.reminder.as_ref().is_some_and(|r| r.enabled && r.next_trigger <= now))
        .map(|t| t.id)
        .collect()
}

/// Call after a reminder fired. One-time reminders are disabled; repeating ones move to their
/// first trigger after `now`, so a reminder missed for several periods fires only once.
pub fn advance(reminder: &mut Reminder, now: DateTime<Utc>) {
    let step = match reminder.repeat {
        Repeat::Once => {
            reminder.enabled = false;
            return;
        }
        Repeat::Daily => Duration::days(1),
        Repeat::Weekly => Duration::weeks(1),
    };
    if reminder.next_trigger <= now {
        let behind = (now - reminder.next_trigger).num_seconds();
        let steps = behind / step.num_seconds() + 1;
        reminder.next_trigger += step * steps as i32;
    }
}

#[cfg(test)]
#[path = "reminders_tests.rs"]
mod tests;
