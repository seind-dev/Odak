//! Repeating tasks and due-date moves, in local time. Pure.

use crate::model::Recurrence;
use crate::views::{WEEKDAYS_TR, add_months, first_of_month};
use chrono::{DateTime, Datelike, Days, Local, NaiveDate, NaiveTime, TimeZone, Utc};

/// Time given to a due date that has none yet (date-only due dates end the day).
fn end_of_day() -> NaiveTime {
    NaiveTime::from_hms_opt(23, 59, 0).expect("valid time")
}

/// `day` at local `time`, as UTC (the later instant when a clock change repeats it, the naive
/// time when one skips it).
fn at_local(day: NaiveDate, time: NaiveTime) -> DateTime<Utc> {
    let naive = day.and_time(time);
    Local.from_local_datetime(&naive).latest().map_or_else(|| naive.and_utc(), |t| t.with_timezone(&Utc))
}

/// The date a repeating task moves to when it is completed: the first occurrence after both its
/// current due date and today (missed occurrences are skipped), at the same time of day.
pub fn next_due(rule: &Recurrence, current: Option<DateTime<Utc>>, now: DateTime<Utc>) -> DateTime<Utc> {
    let today = now.with_timezone(&Local).date_naive();
    let (due_day, time) = match current {
        Some(due) => {
            let local = due.with_timezone(&Local);
            (local.date_naive(), local.time())
        }
        None => (today, end_of_day()),
    };
    let base = due_day.max(today);
    let next = match rule {
        Recurrence::Day => base + Days::new(1),
        Recurrence::Week { days } => {
            let own = [due_day.weekday().num_days_from_monday() as u8];
            let days: &[u8] = if days.is_empty() { &own } else { days };
            (1..=7)
                .map(|n| base + Days::new(n))
                .find(|d| days.contains(&(d.weekday().num_days_from_monday() as u8)))
                .expect("every weekday occurs within a week")
        }
        Recurrence::Month => {
            // The due date's day of the month, or the month's last day when it is shorter.
            let in_month = |first: NaiveDate| {
                let last = (add_months(first, 1) - Days::new(1)).day();
                first.with_day(due_day.day().min(last)).expect("day within the month")
            };
            let this_month = in_month(first_of_month(base));
            if this_month > base { this_month } else { in_month(add_months(first_of_month(base), 1)) }
        }
    };
    at_local(next, time)
}

/// Moves a due date to `day`, keeping its time of day (or ending the day if there was none).
pub fn move_to_day(current: Option<DateTime<Utc>>, day: NaiveDate) -> DateTime<Utc> {
    let time = current.map_or_else(end_of_day, |due| due.with_timezone(&Local).time());
    at_local(day, time)
}

/// "Her gün", "Hafta içi her gün", "Her Pzt, Per", "Her ay".
pub fn label(rule: &Recurrence) -> String {
    match rule {
        Recurrence::Day => "Her gün".into(),
        Recurrence::Month => "Her ay".into(),
        Recurrence::Week { days } => {
            let mut days = days.clone();
            days.sort();
            days.dedup();
            match days.as_slice() {
                [0, 1, 2, 3, 4] => "Hafta içi her gün".into(),
                [0, 1, 2, 3, 4, 5, 6] => "Her gün".into(),
                [] => "Her hafta".into(),
                days => {
                    let names: Vec<&str> = days.iter().filter_map(|d| WEEKDAYS_TR.get(*d as usize).copied()).collect();
                    format!("Her {}", names.join(", "))
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "recurrence_tests.rs"]
mod tests;
