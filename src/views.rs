//! Pure helpers that turn the task list into what each page shows.

use crate::model::{Priority, Status, Task};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Timelike, Utc};
use uuid::Uuid;

pub const MONTHS_TR: [&str; 12] = [
    "Ocak", "Şubat", "Mart", "Nisan", "Mayıs", "Haziran", "Temmuz", "Ağustos", "Eylül", "Ekim", "Kasım", "Aralık",
];
pub const WEEKDAYS_TR: [&str; 7] = ["Pzt", "Sal", "Çar", "Per", "Cum", "Cmt", "Paz"];

/// Which tasks the list shows by group.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    #[default]
    All,
    Personal,
    Group(Uuid),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ListFilter {
    pub priority: Option<Priority>,
    pub tag: Option<String>,
    pub query: String,
    pub scope: Scope,
}

pub fn filter_tasks<'a>(tasks: &'a [Task], f: &ListFilter) -> Vec<&'a Task> {
    let query = f.query.trim().to_lowercase();
    tasks
        .iter()
        .filter(|t| f.priority.is_none_or(|p| t.priority == p))
        .filter(|t| f.tag.as_ref().is_none_or(|tag| t.tags.contains(tag)))
        .filter(|t| match f.scope {
            Scope::All => true,
            Scope::Personal => t.group_id.is_none(),
            Scope::Group(id) => t.group_id == Some(id),
        })
        .filter(|t| {
            query.is_empty()
                || t.title.to_lowercase().contains(&query)
                || t.description.to_lowercase().contains(&query)
        })
        .collect()
}

/// Every tag in use, sorted and unique.
pub fn all_tags(tasks: &[Task]) -> Vec<String> {
    let mut tags: Vec<String> = tasks.iter().flat_map(|t| t.tags.iter().cloned()).collect();
    tags.sort();
    tags.dedup();
    tags
}

/// One line of a task's history after the actor's name, e.g. "durumu değiştirdi: Beklemede → Tamamlandı".
/// `name_of` names the assignee of an `assigned` entry.
pub fn activity_text(action: &str, details: &str, name_of: impl Fn(Uuid) -> String) -> String {
    // The server logs codes ("pending → completed"); show their labels.
    let labels = |label: fn(&str) -> Option<&'static str>| {
        details.split(" → ").map(|code| label(code).unwrap_or(code)).collect::<Vec<_>>().join(" → ")
    };
    match action {
        "created" => "görevi oluşturdu".into(),
        "status_changed" => format!("durumu değiştirdi: {}", labels(|code| code_label::<Status>(code).map(Status::label))),
        "priority_changed" => format!("önceliği değiştirdi: {}", labels(|code| code_label::<Priority>(code).map(Priority::label))),
        "title_changed" => format!("başlığı değiştirdi: {details}"),
        "assigned" => match details.parse::<Uuid>() {
            Ok(id) => format!("görevi atadı: {}", name_of(id)),
            Err(_) => "atamayı kaldırdı".into(),
        },
        "commented" => "yorum yaptı".into(),
        other => other.into(),
    }
}

/// A status or priority from its stored code (`"in_progress"`).
fn code_label<T: serde::de::DeserializeOwned>(code: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(code.into())).ok()
}

/// Open tasks assigned to `me` (Dashboard).
pub fn assigned_to(tasks: &[Task], me: Uuid) -> Vec<&Task> {
    tasks.iter().filter(|t| t.assignee_id == Some(me) && t.status != Status::Completed).collect()
}

pub fn with_status(tasks: &[Task], status: Status) -> Vec<&Task> {
    tasks.iter().filter(|t| t.status == status).collect()
}

/// Tasks due on `day` in local time (calendar cells).
pub fn due_on(tasks: &[Task], day: NaiveDate) -> Vec<&Task> {
    tasks
        .iter()
        .filter(|t| t.due_date.is_some_and(|d| d.with_timezone(&Local).date_naive() == day))
        .collect()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub total: usize,
    pub pending: usize,
    pub in_progress: usize,
    pub completed: usize,
    pub overdue: usize,
}

pub fn stats(tasks: &[Task], now: DateTime<Utc>) -> Stats {
    let mut s = Stats { total: tasks.len(), ..Default::default() };
    for t in tasks {
        match t.status {
            Status::Pending => s.pending += 1,
            Status::InProgress => s.in_progress += 1,
            Status::Completed => s.completed += 1,
        }
        if is_overdue(t, now) {
            s.overdue += 1;
        }
    }
    s
}

pub fn is_overdue(t: &Task, now: DateTime<Utc>) -> bool {
    t.status != Status::Completed && t.due_date.is_some_and(|d| d < now)
}

pub fn open_high_priority(tasks: &[Task]) -> Vec<&Task> {
    tasks
        .iter()
        .filter(|t| t.priority == Priority::High && t.status != Status::Completed)
        .collect()
}

/// Unfinished tasks due between `now` and `now + days`, soonest first.
pub fn due_soon(tasks: &[Task], now: DateTime<Utc>, days: i64) -> Vec<&Task> {
    let end = now + Duration::days(days);
    let mut out: Vec<&Task> = tasks
        .iter()
        .filter(|t| t.status != Status::Completed && t.due_date.is_some_and(|d| d >= now && d <= end))
        .collect();
    out.sort_by_key(|t| t.due_date);
    out
}

/// "Az önce", "5 dk önce", "3 saat önce", or "23 Eylül 04:12" (local) once older than a day.
pub fn time_ago(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let secs = (now - at).num_seconds();
    if secs < 60 {
        "Az önce".into()
    } else if secs < 3_600 {
        format!("{} dk önce", secs / 60)
    } else if secs < 86_400 {
        format!("{} saat önce", secs / 3_600)
    } else {
        let l = at.with_timezone(&Local);
        format!("{} {} {:02}:{:02}", l.day(), MONTHS_TR[l.month0() as usize], l.hour(), l.minute())
    }
}

/// Palette search: tasks whose title, description or a tag contains `query` (case-insensitive).
pub fn search_tasks<'a>(tasks: &'a [Task], query: &str, limit: usize) -> Vec<&'a Task> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    tasks
        .iter()
        .filter(|t| {
            t.title.to_lowercase().contains(&q)
                || t.description.to_lowercase().contains(&q)
                || t.tags.iter().any(|tag| tag.to_lowercase().contains(&q))
        })
        .take(limit)
        .collect()
}

/// The 42 days (six Monday-first weeks) shown for the month that starts at `first`.
pub fn month_days(first: NaiveDate) -> Vec<NaiveDate> {
    let start = first - Duration::days(first.weekday().num_days_from_monday() as i64);
    (0..42).map(|i| start + Duration::days(i)).collect()
}

/// First day of the month `n` months after the month starting at `first`.
pub fn add_months(first: NaiveDate, n: i32) -> NaiveDate {
    let m0 = first.month0() as i32 + n;
    NaiveDate::from_ymd_opt(first.year() + m0.div_euclid(12), m0.rem_euclid(12) as u32 + 1, 1)
        .expect("first of month is always valid")
}

pub fn first_of_month(day: NaiveDate) -> NaiveDate {
    day.with_day(1).expect("day 1 always exists")
}

/// "Eylül 2026".
pub fn month_title(first: NaiveDate) -> String {
    format!("{} {}", MONTHS_TR[first.month0() as usize], first.year())
}

/// "23 Eylül 2026" in local time.
pub fn format_date(d: DateTime<Utc>) -> String {
    let l = d.with_timezone(&Local);
    format!("{} {} {}", l.day(), MONTHS_TR[l.month0() as usize], l.year())
}

/// "23 Eylül 2026 09:00" in local time.
pub fn format_date_time(d: DateTime<Utc>) -> String {
    let l = d.with_timezone(&Local);
    format!("{} {:02}:{:02}", format_date(d), l.hour(), l.minute())
}

#[cfg(test)]
#[path = "views_tests.rs"]
mod tests;
