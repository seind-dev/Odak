//! Dashboard: counts, open high-priority tasks and tasks due in the next 7 days.

use crate::model::{Status, Task};
use crate::state::AppState;
use crate::theme::{Colors, priority_color, status_color};
use crate::fonts;
use crate::ui::icons::{self, icon};
use crate::ui::motion;
use crate::ui::widgets::page_title;
use crate::views;
use chrono::Utc;
use gpui::{App, Entity, FontWeight, IntoElement, Rgba, div, prelude::*, px};

pub fn render(c: &Colors, cx: &App) -> impl IntoElement {
    let state = AppState::global(cx);
    let data = &state.read(cx).data;
    let tasks = &data.tasks;
    let now = Utc::now();
    let s = views::stats(tasks, now);
    let mine = data.me().map(|me| views::assigned_to(tasks, me));
    div().id("dashboard").size_full().overflow_y_scroll().child(
        div()
            .p_6()
            .flex()
            .flex_col()
            .gap_5()
            .child(page_title("Dashboard", c))
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(tile(icons::LIST, "Toplam", s.total, c.text, c))
                    .child(tile(icons::CLOCK, "Bekleyen", s.pending, status_color(Status::Pending), c))
                    .child(tile(icons::REFRESH, "Devam Eden", s.in_progress, status_color(Status::InProgress), c))
                    .child(tile(icons::CHECK, "Tamamlanan", s.completed, status_color(Status::Completed), c))
                    .child(tile(icons::TIME_PAST, "Gecikmiş", s.overdue, c.danger, c)),
            )
            .child(
                div()
                    .flex()
                    .gap_4()
                    .child(section(
                        "Yüksek Öncelikli",
                        views::open_high_priority(tasks),
                        "Açık yüksek öncelikli görev yok.",
                        &state,
                        c,
                    ))
                    .child(section(
                        "Yaklaşan (7 gün)",
                        views::due_soon(tasks, now, 7),
                        "Önümüzdeki 7 günde son tarihi olan görev yok.",
                        &state,
                        c,
                    )),
            )
            .when_some(mine, |d, mine| {
                d.child(section("Bana atananlar", mine, "Sana atanmış açık görev yok.", &state, c))
            }),
    )
}

fn tile(glyph: &'static str, label: &'static str, value: usize, color: Rgba, c: &Colors) -> impl IntoElement {
    let tile = div()
        .flex_1()
        .p_4()
        .rounded_xl()
        .bg(c.surface)
        .border_1()
        .border_color(c.border)
        .flex()
        .flex_col()
        .gap_2()
        .child(icon(glyph).size(px(28.)).rounded_lg().bg(gpui::Hsla::from(color).opacity(0.15)).text_color(color).text_sm())
        .child(
            div()
                .font_family(fonts::DISPLAY)
                .text_2xl()
                .font_weight(FontWeight::BOLD)
                .text_color(color)
                .child(value.to_string()),
        )
        .child(div().text_xs().text_color(c.muted).child(label));
    motion::enter_once("tile", label, tile)
}

fn section(
    title: &'static str,
    tasks: Vec<&Task>,
    empty: &'static str,
    state: &Entity<AppState>,
    c: &Colors,
) -> impl IntoElement {
    let hover = c.hover;
    div()
        .id(title)
        .flex_1()
        .min_w_0()
        .p_4()
        .rounded_xl()
        .bg(c.surface)
        .border_1()
        .border_color(c.border)
        .flex()
        .flex_col()
        .gap_1()
        .child(div().pb_1().text_sm().font_weight(FontWeight::SEMIBOLD).child(title))
        .when(tasks.is_empty(), |d| d.child(div().text_sm().text_color(c.muted).child(empty)))
        .children(tasks.into_iter().map(|t| {
            let id = t.id;
            let state = state.clone();
            let row = div()
                .id(id)
                .px_2()
                .py_1p5()
                .rounded_md()
                .flex()
                .items_center()
                .gap_2()
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .on_click(move |_, _, cx| state.update(cx, |s, cx| s.edit(id, cx)))
                .child(div().flex_none().size(px(8.)).rounded_full().bg(priority_color(t.priority)))
                .child(div().flex_1().min_w_0().text_sm().truncate().child(t.title.clone()))
                .when_some(t.due_date, |d, due| {
                    d.child(div().flex_none().text_xs().text_color(c.muted).child(views::format_date(due)))
                });
            motion::enter_once("dashboard-row", (title, id), row)
        }))
}
