//! Month calendar that shows tasks on their due dates.

use crate::state::AppState;
use crate::theme::{self, priority_color};
use crate::ui::icons;
use crate::ui::motion;
use crate::ui::widgets::{button, icon_button, month_grid, page_title};
use crate::views::{self, add_months, first_of_month, month_title};
use chrono::{Datelike, Local, NaiveDate};
use gpui::{
    AnyElement, Context, FontWeight, Hsla, IntoElement, Render, Subscription, Window, div, prelude::*, px,
};

/// Tasks listed in a day cell before "+N daha".
const MAX_PER_DAY: usize = 3;

pub struct CalendarPage {
    /// First day of the month shown.
    month: NaiveDate,
    /// Where the next month grid slides in from: +1 later month, -1 earlier, 0 none.
    direction: f32,
    _observe: Subscription,
}

impl CalendarPage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        CalendarPage {
            month: first_of_month(Local::now().date_naive()),
            direction: 0.0,
            _observe: cx.observe(&state, |_, _, cx| cx.notify()),
        }
    }

    fn show_month(&mut self, month: NaiveDate, cx: &mut Context<Self>) {
        self.direction = match month.cmp(&self.month) {
            std::cmp::Ordering::Greater => 1.0,
            std::cmp::Ordering::Less => -1.0,
            std::cmp::Ordering::Equal => self.direction,
        };
        self.month = month;
        cx.notify();
    }
}

impl Render for CalendarPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme::current(cx);
        let state = AppState::global(cx);
        let tasks = &state.read(cx).data.tasks;
        let today = Local::now().date_naive();
        let cell = |day: NaiveDate, in_month: bool| -> AnyElement {
            let due = views::due_on(tasks, day);
            let extra = due.len().saturating_sub(MAX_PER_DAY);
            let is_today = day == today;
            div()
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
                .children(due.into_iter().take(MAX_PER_DAY).map(|t| {
                    let id = t.id;
                    let state = state.clone();
                    let color: Hsla = priority_color(t.priority).into();
                    div()
                        .id(id)
                        .px_1()
                        .rounded_sm()
                        .text_xs()
                        .truncate()
                        .cursor_pointer()
                        .text_color(color)
                        .bg(color.opacity(0.12))
                        .hover(move |s| s.bg(color.opacity(0.25)))
                        .on_click(move |_, _, cx| state.update(cx, |s, cx| s.edit(id, cx)))
                        .child(t.title.clone())
                }))
                .when(extra > 0, |d| d.child(div().text_xs().text_color(c.muted).child(format!("+{extra} daha"))))
                .into_any_element()
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_4()
            .p_6()
            .child(
                page_title(month_title(self.month), &c).child(
                    div()
                        .flex()
                        .gap_2()
                        .child(icon_button("prev-month", icons::CHEVRON_LEFT, &c).on_click(cx.listener(|this, _, _, cx| {
                            this.show_month(add_months(this.month, -1), cx)
                        })))
                        .child(button("this-month", "Bugün", &c).on_click(cx.listener(|this, _, _, cx| {
                            this.show_month(first_of_month(Local::now().date_naive()), cx)
                        })))
                        .child(icon_button("next-month", icons::CHEVRON_RIGHT, &c).on_click(cx.listener(|this, _, _, cx| {
                            this.show_month(add_months(this.month, 1), cx)
                        }))),
                ),
            )
            .child(div().id("calendar").flex_1().min_h_0().overflow_y_scroll().child(motion::appear(
                motion::key("month", self.month),
                month_grid(self.month, &c, &cell),
                16.0 * self.direction,
                0.,
            )))
    }
}
