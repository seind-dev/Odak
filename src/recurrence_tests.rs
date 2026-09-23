use super::*;
use chrono::Timelike;

fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
    Local.with_ymd_and_hms(y, m, d, h, min, 0).unwrap().with_timezone(&Utc)
}

/// (local date, hour, minute) of a UTC instant.
fn local(t: DateTime<Utc>) -> (NaiveDate, u32, u32) {
    let l = t.with_timezone(&Local);
    (l.date_naive(), l.hour(), l.minute())
}

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

// 2026-09-21 is a Monday.
const MON: (i32, u32, u32) = (2026, 9, 21);

#[test]
fn daily_moves_to_the_next_day_at_the_same_time() {
    let due = at(MON.0, MON.1, MON.2, 9, 30);
    let now = at(MON.0, MON.1, MON.2, 10, 0);
    assert_eq!(local(next_due(&Recurrence::Day, Some(due), now)), (date(2026, 9, 22), 9, 30));
}

#[test]
fn missed_occurrences_are_skipped() {
    let due = at(2026, 9, 18, 9, 0); // three days overdue
    let now = at(MON.0, MON.1, MON.2, 12, 0);
    assert_eq!(local(next_due(&Recurrence::Day, Some(due), now)).0, date(2026, 9, 22));
}

#[test]
fn weekly_goes_to_the_next_chosen_weekday() {
    let rule = Recurrence::Week { days: vec![0, 3] }; // Monday and Thursday
    let now = at(MON.0, MON.1, MON.2, 8, 0);
    assert_eq!(local(next_due(&rule, Some(at(MON.0, MON.1, MON.2, 18, 0)), now)).0, date(2026, 9, 24));
    assert_eq!(local(next_due(&rule, Some(at(2026, 9, 24, 18, 0)), now)).0, date(2026, 9, 28));
}

#[test]
fn weekly_without_days_keeps_the_due_weekday() {
    let rule = Recurrence::Week { days: vec![] };
    let now = at(MON.0, MON.1, MON.2, 8, 0);
    assert_eq!(local(next_due(&rule, Some(at(MON.0, MON.1, MON.2, 18, 0)), now)).0, date(2026, 9, 28));
}

#[test]
fn monthly_keeps_the_day_or_the_months_last_day() {
    let now = at(2026, 9, 1, 8, 0);
    assert_eq!(local(next_due(&Recurrence::Month, Some(at(2026, 9, 15, 9, 0)), now)).0, date(2026, 10, 15));
    let now = at(2027, 1, 31, 8, 0);
    assert_eq!(local(next_due(&Recurrence::Month, Some(at(2027, 1, 31, 9, 0)), now)).0, date(2027, 2, 28));
}

#[test]
fn a_task_without_a_due_date_starts_from_today() {
    let now = at(MON.0, MON.1, MON.2, 8, 0);
    assert_eq!(local(next_due(&Recurrence::Day, None, now)), (date(2026, 9, 22), 23, 59));
}

#[test]
fn moving_to_a_day_keeps_the_time() {
    assert_eq!(local(move_to_day(Some(at(MON.0, MON.1, MON.2, 14, 45)), date(2026, 9, 30))), (date(2026, 9, 30), 14, 45));
    assert_eq!(local(move_to_day(None, date(2026, 9, 30))), (date(2026, 9, 30), 23, 59));
}

#[test]
fn labels_read_naturally() {
    assert_eq!(label(&Recurrence::Day), "Her gün");
    assert_eq!(label(&Recurrence::Month), "Her ay");
    assert_eq!(label(&Recurrence::Week { days: vec![4, 0, 1, 2, 3] }), "Hafta içi her gün");
    assert_eq!(label(&Recurrence::Week { days: vec![3, 0] }), "Her Pzt, Per");
}
