//! Date (optionally with time) picker: a button that opens a pop-over month grid.

use crate::theme::{self, Colors};
use crate::ui::icons;
use crate::ui::motion;
use crate::ui::widgets::{button, icon_button, month_grid};
use crate::views::{add_months, first_of_month, format_date, format_date_time, month_title};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike, Utc};
use gpui::{
    Anchor, AnyElement, Context, FontWeight, Hsla, IntoElement, Window, anchored, deferred, div, prelude::*, px,
    white,
};

pub struct DateTimePicker {
    value: Option<DateTime<Local>>,
    with_time: bool,
    open: bool,
    /// First day of the month shown in the pop-over.
    month: NaiveDate,
    placeholder: &'static str,
}

impl DateTimePicker {
    /// Without time, a picked day is stored as 23:59 local, so it is due at the end of that day.
    pub fn new(value: Option<DateTime<Utc>>, with_time: bool, placeholder: &'static str) -> Self {
        let value = value.map(|v| v.with_timezone(&Local));
        let shown = value.map_or_else(|| Local::now().date_naive(), |v| v.date_naive());
        DateTimePicker { value, with_time, open: false, month: first_of_month(shown), placeholder }
    }

    pub fn value(&self) -> Option<DateTime<Utc>> {
        self.value.map(|v| v.with_timezone(&Utc))
    }

    fn pick(&mut self, day: NaiveDate, cx: &mut Context<Self>) {
        let time = match self.value {
            Some(v) => v.time(),
            None if self.with_time => NaiveTime::from_hms_opt(9, 0, 0).expect("valid time"),
            None => NaiveTime::from_hms_opt(23, 59, 0).expect("valid time"),
        };
        self.value = to_local(day.and_time(time));
        if !self.with_time {
            self.open = false;
        }
        cx.notify();
    }

    fn shift(&mut self, minutes: i64, cx: &mut Context<Self>) {
        if let Some(v) = self.value {
            self.value = Some(v + Duration::minutes(minutes));
        }
        cx.notify();
    }

    fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.open = open;
        cx.notify();
    }

    fn popover(&self, c: &Colors, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let today = Local::now().date_naive();
        let selected = self.value.map(|v| v.date_naive());
        let (accent, text, muted, hover): (Hsla, Hsla, Hsla, Hsla) =
            (c.accent.into(), c.text.into(), c.muted.into(), c.hover.into());
        let entity = cx.entity();
        let cell = move |day: NaiveDate, in_month: bool| -> AnyElement {
            let is_selected = selected == Some(day);
            let entity = entity.clone();
            div()
                .id(("day", day.num_days_from_ce() as usize))
                .h(px(30.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .text_sm()
                .cursor_pointer()
                .text_color(if is_selected { white() } else if in_month { text } else { muted })
                .hover(move |s| s.bg(hover))
                .when(is_selected, |d| d.bg(accent))
                .when(day == today && !is_selected, |d| d.border_1().border_color(accent))
                .child(day.day().to_string())
                .on_click(move |_, _, cx| entity.update(cx, |this, cx| this.pick(day, cx)))
                .into_any_element()
        };
        let time_row = self.value.filter(|_| self.with_time).map(|v| {
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_1()
                .child(button("h-", "−1 sa", c).on_click(cx.listener(|this, _, _, cx| this.shift(-60, cx))))
                .child(button("m-", "−5 dk", c).on_click(cx.listener(|this, _, _, cx| this.shift(-5, cx))))
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("{:02}:{:02}", v.hour(), v.minute())),
                )
                .child(button("m+", "+5 dk", c).on_click(cx.listener(|this, _, _, cx| this.shift(5, cx))))
                .child(button("h+", "+1 sa", c).on_click(cx.listener(|this, _, _, cx| this.shift(60, cx))))
        });
        div()
            .id("popover")
            .occlude()
            .mt_1()
            .w(px(340.))
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .rounded_xl()
            .bg(c.surface)
            .border_1()
            .border_color(c.border)
            .shadow_lg()
            .text_color(c.text)
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.set_open(false, cx)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(icon_button("prev", icons::CHEVRON_LEFT, c).on_click(cx.listener(|this, _, _, cx| {
                        this.month = add_months(this.month, -1);
                        cx.notify();
                    })))
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(month_title(self.month)))
                    .child(icon_button("next", icons::CHEVRON_RIGHT, c).on_click(cx.listener(|this, _, _, cx| {
                        this.month = add_months(this.month, 1);
                        cx.notify();
                    }))),
            )
            .child(month_grid(self.month, c, &cell))
            .children(time_row)
            .child(
                div()
                    .flex()
                    .justify_end()
                    .child(button("done", "Tamam", c).on_click(cx.listener(|this, _, _, cx| this.set_open(false, cx)))),
            )
    }
}

/// Local time for a wall-clock value; a time inside a DST gap moves one hour forward.
fn to_local(naive: NaiveDateTime) -> Option<DateTime<Local>> {
    Local
        .from_local_datetime(&naive)
        .earliest()
        .or_else(|| Local.from_local_datetime(&(naive + Duration::hours(1))).earliest())
}

impl Render for DateTimePicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme::current(cx);
        let label = match self.value {
            Some(v) if self.with_time => format_date_time(v.with_timezone(&Utc)),
            Some(v) => format_date(v.with_timezone(&Utc)),
            None => self.placeholder.to_string(),
        };
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(button("open", label, &c).on_click(cx.listener(|this, _, _, cx| this.set_open(true, cx))))
                    .when(self.value.is_some(), |d| {
                        d.child(button("clear", "Temizle", &c).on_click(cx.listener(|this, _, _, cx| {
                            this.value = None;
                            this.set_open(false, cx);
                        })))
                    }),
            )
            .when(self.open, |d| {
                d.child(
                    deferred(anchored().anchor(Anchor::TopLeft).snap_to_window_with_margin(px(8.)).child(motion::appear(
                        "popover-in",
                        self.popover(&c, cx),
                        0.,
                        -4.,
                    )))
                        .priority(1),
                )
            })
    }
}
