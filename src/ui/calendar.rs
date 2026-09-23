//! Calendar: a month grid or a week of columns showing tasks on their due dates. Tasks can be
//! dragged to another day; in the week view, undated tasks can be dragged onto a day too.

use crate::model::{Status, Task};
use crate::state::AppState;
use crate::theme::{self, Colors, priority_color};
use crate::ui::icons;
use crate::ui::motion;
use crate::ui::widgets::{DraggedTask, button, icon_button, month_grid, page_title, segmented};
use crate::views::{self, MONTHS_TR, WEEKDAYS_TR, add_months, first_of_month, month_title};
use chrono::{Datelike, Days, Local, NaiveDate, Timelike, Utc};
use gpui::{
    AnyElement, App, Context, Entity, FontWeight, Hsla, IntoElement, Render, Stateful, Subscription, Window, div,
    prelude::*, px,
};

/// Tasks listed in a month cell before "+N daha".
const MAX_PER_DAY: usize = 3;

#[derive(Clone, Copy, PartialEq)]
enum View {
    Month,
    Week,
}

pub struct CalendarPage {
    view: View,
    /// First day of the month shown (month view).
    month: NaiveDate,
    /// Monday of the week shown (week view).
    week: NaiveDate,
    /// Where the next page slides in from: +1 later, -1 earlier, 0 none.
    direction: f32,
    _observe: Subscription,
}

fn monday_of(day: NaiveDate) -> NaiveDate {
    day - Days::new(u64::from(day.weekday().num_days_from_monday()))
}

impl CalendarPage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let today = Local::now().date_naive();
        CalendarPage {
            view: View::Month,
            month: first_of_month(today),
            week: monday_of(today),
            direction: 0.0,
            _observe: cx.observe(&state, |_, _, cx| cx.notify()),
        }
    }

    /// Shows the month or week containing `day` (the page's start for the current view).
    fn show(&mut self, start: NaiveDate, cx: &mut Context<Self>) {
        let current = if self.view == View::Month { self.month } else { self.week };
        self.direction = match start.cmp(&current) {
            std::cmp::Ordering::Greater => 1.0,
            std::cmp::Ordering::Less => -1.0,
            std::cmp::Ordering::Equal => self.direction,
        };
        match self.view {
            View::Month => self.month = start,
            View::Week => self.week = start,
        }
        cx.notify();
    }

    fn step(&mut self, by: i32, cx: &mut Context<Self>) {
        let start = match self.view {
            View::Month => add_months(self.month, by),
            View::Week if by < 0 => self.week - Days::new(7),
            View::Week => self.week + Days::new(7),
        };
        self.show(start, cx);
    }

    fn show_today(&mut self, cx: &mut Context<Self>) {
        let today = Local::now().date_naive();
        let start = if self.view == View::Month { first_of_month(today) } else { monday_of(today) };
        self.show(start, cx);
    }

    /// Switching keeps the place: the week of today if it is in the month shown, else its first week.
    fn set_view(&mut self, view: View, cx: &mut Context<Self>) {
        if view == self.view {
            return;
        }
        let today = Local::now().date_naive();
        match view {
            View::Week => {
                let in_month = first_of_month(today) == self.month;
                self.week = monday_of(if in_month { today } else { self.month });
            }
            View::Month => self.month = first_of_month(self.week),
        }
        self.view = view;
        self.direction = 0.0;
        cx.notify();
    }

    fn title(&self) -> String {
        match self.view {
            View::Month => month_title(self.month),
            View::Week => {
                let end = self.week + Days::new(6);
                let month = |d: NaiveDate| MONTHS_TR[d.month0() as usize];
                if self.week.month() == end.month() {
                    format!("{}–{} {} {}", self.week.day(), end.day(), month(end), end.year())
                } else {
                    format!("{} {} – {} {} {}", self.week.day(), month(self.week), end.day(), month(end), end.year())
                }
            }
        }
    }
}

/// Dropping a task on a day moves its due date there.
fn drop_on_day<E: InteractiveElement + Styled>(element: E, day: NaiveDate, state: &Entity<AppState>, c: &Colors) -> E {
    let accent = c.accent;
    let state = state.clone();
    element.drag_over::<DraggedTask>(move |s, _, _, _| s.border_color(accent).bg(Hsla::from(accent).opacity(0.08))).on_drop(
        move |drag: &DraggedTask, _, cx| {
            let id = drag.id;
            state.update(cx, |s, cx| {
                let _ = s.mutate(cx, |d| d.reschedule(id, day, Utc::now()));
            });
        },
    )
}

/// A task that opens on click and can be dragged to a day.
fn task_chip(t: &Task, state: &Entity<AppState>, c: &Colors) -> Stateful<gpui::Div> {
    let id = t.id;
    let state = state.clone();
    let color: Hsla = priority_color(t.priority).into();
    div()
        .id(id)
        .cursor_pointer()
        .text_color(color)
        .bg(color.opacity(0.12))
        .hover(move |s| s.bg(color.opacity(0.25)))
        .when(t.status == Status::Completed, |d| d.line_through().opacity(0.6))
        .on_click(move |_, _, cx| state.update(cx, |s, cx| s.edit(id, cx)))
        .on_drag(DraggedTask { id, title: t.title.clone().into(), colors: *c }, |drag, _, _, cx| cx.new(|_| drag.clone()))
}

/// "14:30", or nothing for a date-only due date (those end the day at 23:59).
fn time_label(t: &Task) -> Option<String> {
    let due = t.due_date?.with_timezone(&Local);
    (due.hour(), due.minute()).ne(&(23, 59)).then(|| format!("{:02}:{:02}", due.hour(), due.minute()))
}

impl CalendarPage {
    fn month_view(&self, tasks: &[Task], state: &Entity<AppState>, c: &Colors) -> AnyElement {
        let today = Local::now().date_naive();
        let cell = |day: NaiveDate, in_month: bool| -> AnyElement {
            let due = views::due_on(tasks, day);
            let extra = due.len().saturating_sub(MAX_PER_DAY);
            let is_today = day == today;
            let cell = div()
                .id(("day", day.num_days_from_ce() as usize))
                .h_full()
                .min_h(px(92.))
                .p_1p5()
                .rounded_lg()
                .bg(c.surface)
                .border_1()
                .border_color(if is_today { c.accent } else { c.border })
                .flex()
                .flex_col()
                .gap_1()
                .when(!in_month, |d| d.opacity(0.45))
                .child(
                    div()
                        .text_xs()
                        .font_weight(if is_today { FontWeight::BOLD } else { FontWeight::NORMAL })
                        .text_color(if is_today { c.accent } else { c.text })
                        .child(day.day().to_string()),
                )
                .children(
                    due.into_iter()
                        .take(MAX_PER_DAY)
                        .map(|t| task_chip(t, state, c).px_1().rounded_sm().text_xs().truncate().child(t.title.clone())),
                )
                .when(extra > 0, |d| d.child(div().text_xs().text_color(c.muted).child(format!("+{extra} daha"))));
            drop_on_day(cell, day, state, c).into_any_element()
        };
        month_grid(self.month, c, &cell).into_any_element()
    }

    fn week_view(&self, tasks: &[Task], state: &Entity<AppState>, c: &Colors) -> AnyElement {
        let today = Local::now().date_naive();
        let columns = (0..7).map(|offset| {
            let day = self.week + Days::new(offset);
            let is_today = day == today;
            let due = views::due_on(tasks, day);
            let column = div()
                .id(("week-day", offset as usize))
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1p5()
                .p_1p5()
                .rounded_lg()
                .bg(c.surface)
                .border_1()
                .border_color(if is_today { c.accent } else { c.border })
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap_1()
                        .pb_1()
                        .text_color(if is_today { c.accent } else { c.muted })
                        .child(div().text_xs().child(WEEKDAYS_TR[offset as usize]))
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(if is_today { c.accent } else { c.text })
                                .child(day.day().to_string()),
                        ),
                )
                .children(due.into_iter().map(|t| {
                    task_chip(t, state, c)
                        .px_1p5()
                        .py_1()
                        .rounded_md()
                        .text_xs()
                        .flex()
                        .flex_col()
                        .child(div().font_weight(FontWeight::MEDIUM).child(t.title.clone()))
                        .when_some(time_label(t), |d, time| d.child(div().opacity(0.8).child(time)))
                }));
            drop_on_day(column, day, state, c)
        });
        let mut undated: Vec<&Task> = tasks.iter().filter(|t| t.due_date.is_none() && t.status != Status::Completed).collect();
        undated.sort_by_key(|t| t.order);
        let panel = div()
            .id("undated")
            .w(px(168.))
            .flex_none()
            .flex()
            .flex_col()
            .gap_1p5()
            .p_2()
            .rounded_lg()
            .border_1()
            .border_color(c.border)
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .pb_1()
                    .text_xs()
                    .text_color(c.muted)
                    .child("Tarihsiz")
                    .child(undated.len().to_string()),
            )
            .when(undated.is_empty(), |d| d.child(div().text_xs().text_color(c.muted).child("Tarihsiz görev yok.")))
            .when(!undated.is_empty(), |d| d.child(div().text_xs().text_color(c.muted).child("Bir güne sürükleyerek planla.")))
            .children(undated.into_iter().map(|t| task_chip(t, state, c).p_1p5().rounded_md().text_xs().child(t.title.clone())));
        div().h_full().flex().gap_2().child(div().flex_1().min_w_0().flex().gap_1p5().children(columns)).child(panel).into_any_element()
    }
}

impl Render for CalendarPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme::current(cx);
        let state = AppState::global(cx);
        let tasks = &state.read(cx).data.tasks;
        let this = cx.entity();
        let (page, start) = match self.view {
            View::Month => (self.month_view(tasks, &state, &c), self.month),
            View::Week => (self.week_view(tasks, &state, &c), self.week),
        };
        let view_choice = segmented("calendar-view", &[(View::Month, "Ay"), (View::Week, "Hafta")], self.view, &c, {
            move |view, _, cx: &mut App| this.update(cx, |this, cx| this.set_view(view, cx))
        });
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_4()
            .p_6()
            .child(
                page_title(self.title(), &c).child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(view_choice)
                        .child(icon_button("prev", icons::CHEVRON_LEFT, &c).on_click(cx.listener(|this, _, _, cx| this.step(-1, cx))))
                        .child(button("today", "Bugün", &c).on_click(cx.listener(|this, _, _, cx| this.show_today(cx))))
                        .child(icon_button("next", icons::CHEVRON_RIGHT, &c).on_click(cx.listener(|this, _, _, cx| this.step(1, cx)))),
                ),
            )
            .child(
                div().id("calendar").flex_1().min_h_0().overflow_y_scroll().child(motion::appear(
                    motion::key("calendar-page", (self.view == View::Week, start)),
                    div().size_full().child(page),
                    16.0 * self.direction,
                    0.,
                )),
            )
    }
}
