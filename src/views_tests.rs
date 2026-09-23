use super::*;
use chrono::TimeZone;
use uuid::Uuid;

fn noon(y: i32, m: u32, d: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, d, 12, 0, 0).unwrap()
}

fn task(title: &str, priority: Priority, status: Status, tags: &[&str], due: Option<DateTime<Utc>>) -> Task {
    Task {
        id: Uuid::new_v4(),
        title: title.into(),
        description: String::new(),
        priority,
        status,
        reminder: None,
        subtasks: vec![],
        tags: tags.iter().map(|t| t.to_string()).collect(),
        order: 0,
        due_date: due,
        created_at: noon(2026, 1, 1),
        updated_at: noon(2026, 1, 1),
        owner_id: None,
        group_id: None,
        assignee_id: None,
        recurrence: None,
    }
}

fn titles(tasks: Vec<&Task>) -> Vec<String> {
    tasks.iter().map(|t| t.title.clone()).collect()
}

#[test]
fn filter_by_priority_tag_and_query() {
    use Priority::*;
    let tasks = vec![
        task("Rapor yaz", High, Status::Pending, &["iş"], None),
        task("Süt al", Low, Status::Pending, &["ev"], None),
        task("Sunum", High, Status::Completed, &["iş"], None),
    ];
    let f = |priority, tag: Option<&str>, query: &str| ListFilter {
        priority,
        tag: tag.map(str::to_string),
        query: query.into(),
        ..Default::default()
    };
    assert_eq!(titles(filter_tasks(&tasks, &f(None, None, ""))), ["Rapor yaz", "Süt al", "Sunum"]);
    assert_eq!(titles(filter_tasks(&tasks, &f(Some(High), None, ""))), ["Rapor yaz", "Sunum"]);
    assert_eq!(titles(filter_tasks(&tasks, &f(None, Some("ev"), ""))), ["Süt al"]);
    assert_eq!(titles(filter_tasks(&tasks, &f(None, None, " RAPOR "))), ["Rapor yaz"]);
    assert_eq!(titles(filter_tasks(&tasks, &f(Some(High), None, "sun"))), ["Sunum"]);
}

#[test]
fn all_tags_is_sorted_and_unique() {
    let tasks = vec![
        task("a", Priority::Low, Status::Pending, &["iş", "acil"], None),
        task("b", Priority::Low, Status::Pending, &["iş"], None),
    ];
    assert_eq!(all_tags(&tasks), ["acil", "iş"]);
}

#[test]
fn stats_count_statuses_and_overdue() {
    let now = noon(2026, 9, 10);
    let tasks = vec![
        task("late", Priority::Low, Status::Pending, &[], Some(noon(2026, 9, 9))),
        task("soon", Priority::Low, Status::InProgress, &[], Some(noon(2026, 9, 11))),
        task("done", Priority::Low, Status::Completed, &[], Some(noon(2026, 9, 1))),
        task("none", Priority::Low, Status::Pending, &[], None),
    ];
    assert_eq!(stats(&tasks, now), Stats { total: 4, pending: 2, in_progress: 1, completed: 1, overdue: 1 });
}

#[test]
fn due_soon_is_bounded_unfinished_and_sorted() {
    let now = noon(2026, 9, 10);
    let tasks = vec![
        task("12", Priority::Low, Status::Pending, &[], Some(noon(2026, 9, 12))),
        task("11", Priority::Low, Status::Pending, &[], Some(noon(2026, 9, 11))),
        task("20", Priority::Low, Status::Pending, &[], Some(noon(2026, 9, 20))),
        task("9", Priority::Low, Status::Pending, &[], Some(noon(2026, 9, 9))),
        task("done", Priority::Low, Status::Completed, &[], Some(noon(2026, 9, 11))),
    ];
    assert_eq!(titles(due_soon(&tasks, now, 7)), ["11", "12"]);
}

#[test]
fn open_high_priority_skips_completed() {
    let tasks = vec![
        task("a", Priority::High, Status::Pending, &[], None),
        task("b", Priority::High, Status::Completed, &[], None),
        task("c", Priority::Low, Status::Pending, &[], None),
    ];
    assert_eq!(titles(open_high_priority(&tasks)), ["a"]);
}

#[test]
fn due_on_matches_the_local_day() {
    let tasks = vec![task("a", Priority::Low, Status::Pending, &[], Some(noon(2026, 9, 10)))];
    let day = |d| NaiveDate::from_ymd_opt(2026, 9, d).unwrap();
    assert_eq!(titles(due_on(&tasks, day(10))), ["a"]);
    assert!(due_on(&tasks, day(11)).is_empty());
}

#[test]
fn month_days_start_on_monday_and_cover_the_month() {
    let days = month_days(NaiveDate::from_ymd_opt(2026, 9, 1).unwrap());
    assert_eq!(days.len(), 42);
    assert_eq!(days[0], NaiveDate::from_ymd_opt(2026, 8, 31).unwrap());
    assert_eq!(days[0].weekday(), chrono::Weekday::Mon);
    assert!(days.contains(&NaiveDate::from_ymd_opt(2026, 9, 30).unwrap()));
}

#[test]
fn add_months_wraps_years() {
    let d = |y, m| NaiveDate::from_ymd_opt(y, m, 1).unwrap();
    assert_eq!(add_months(d(2026, 12), 1), d(2027, 1));
    assert_eq!(add_months(d(2026, 1), -1), d(2025, 12));
    assert_eq!(add_months(d(2026, 5), 0), d(2026, 5));
}

#[test]
fn month_title_is_turkish() {
    assert_eq!(month_title(NaiveDate::from_ymd_opt(2026, 9, 1).unwrap()), "Eylül 2026");
}

#[test]
fn time_ago_buckets() {
    let now = noon(2026, 9, 10);
    assert_eq!(time_ago(now - Duration::seconds(30), now), "Az önce");
    assert_eq!(time_ago(now - Duration::minutes(5), now), "5 dk önce");
    assert_eq!(time_ago(now - Duration::hours(3), now), "3 saat önce");
    assert!(time_ago(now - Duration::days(2), now).contains("Eylül"));
}

#[test]
fn search_tasks_matches_title_description_and_tags() {
    let mut a = task("Rapor", Priority::Low, Status::Pending, &["iş"], None);
    a.description = "Çeyrek sonu".into();
    let b = task("Market", Priority::Low, Status::Pending, &["ev"], None);
    let tasks = vec![a, b];
    assert_eq!(titles(search_tasks(&tasks, "rapor", 10)), ["Rapor"]);
    assert_eq!(titles(search_tasks(&tasks, "sonu", 10)), ["Rapor"]);
    assert_eq!(titles(search_tasks(&tasks, "EV", 10)), ["Market"]);
    assert!(search_tasks(&tasks, "  ", 10).is_empty());
    assert_eq!(search_tasks(&tasks, "r", 1).len(), 1);
}


#[test]
fn scope_filters_personal_and_group_tasks() {
    let g = Uuid::from_u128(50);
    let mut shared = task("shared", Priority::Low, Status::Pending, &[], None);
    shared.group_id = Some(g);
    let personal = task("personal", Priority::Low, Status::Pending, &[], None);
    let tasks = vec![shared, personal];
    let titles = |scope| {
        filter_tasks(&tasks, &ListFilter { scope, ..Default::default() }).iter().map(|t| t.title.as_str()).collect::<Vec<_>>()
    };
    assert_eq!(titles(Scope::All), vec!["shared", "personal"]);
    assert_eq!(titles(Scope::Personal), vec!["personal"]);
    assert_eq!(titles(Scope::Group(g)), vec!["shared"]);
}

#[test]
fn assigned_to_lists_open_tasks_of_that_user() {
    let me = Uuid::from_u128(1);
    let mut open = task("open", Priority::Low, Status::Pending, &[], None);
    open.assignee_id = Some(me);
    let mut done = task("done", Priority::Low, Status::Completed, &[], None);
    done.assignee_id = Some(me);
    let other = task("other", Priority::Low, Status::Pending, &[], None);
    let tasks = vec![open, done, other];
    assert_eq!(assigned_to(&tasks, me).iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), vec!["open"]);
}

#[test]
fn activity_lines_use_labels_and_names() {
    let name = |_| "Ayşe".to_string();
    assert_eq!(activity_text("created", "Rapor", name), "görevi oluşturdu");
    assert_eq!(activity_text("status_changed", "pending → in_progress", name), "durumu değiştirdi: Beklemede → Devam Ediyor");
    assert_eq!(activity_text("priority_changed", "low → high", name), "önceliği değiştirdi: Düşük → Yüksek");
    let id = Uuid::from_u128(7).to_string();
    assert_eq!(activity_text("assigned", &id, name), "görevi atadı: Ayşe");
    assert_eq!(activity_text("assigned", "", name), "atamayı kaldırdı");
    assert_eq!(activity_text("title_changed", "Yeni ad", name), "başlığı değiştirdi: Yeni ad");
}
