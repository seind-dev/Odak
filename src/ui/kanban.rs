//! Kanban board: one column per status; dropping a card on a column changes its status.

use crate::model::{Status, Task};
use crate::state::AppState;
use crate::theme::{Colors, priority_color, status_color};
use crate::recurrence;
use crate::ui::icons;
use crate::ui::motion;
use crate::data::Data;
use crate::ui::widgets::{DraggedTask, chip, icon_chip, page_title, sharing_badges};
use crate::views;
use gpui::{App, Entity, FontWeight, Hsla, IntoElement, div, prelude::*, px};

pub fn render(c: &Colors, cx: &App) -> impl IntoElement {
    let state = AppState::global(cx);
    let data = &state.read(cx).data;
    let tasks = &data.tasks;
    div().size_full().flex().flex_col().gap_4().p_6().child(page_title("Kanban", c)).child(
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .gap_4()
            .children(Status::ALL.map(|status| column(&state, data, status, views::with_status(tasks, status), c))),
    )
}

fn column(state: &Entity<AppState>, data: &Data, status: Status, tasks: Vec<&Task>, c: &Colors) -> impl IntoElement {
    let accent = c.accent;
    let color = status_color(status);
    div()
        .id(("column", status as usize))
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_3()
        .p_3()
        .rounded_xl()
        .bg(c.surface)
        .border_1()
        .border_color(c.border)
        .drag_over::<DraggedTask>(move |s, _, _, _| s.border_color(accent).bg(Hsla::from(accent).opacity(0.06)))
        .on_drop({
            let state = state.clone();
            move |drag: &DraggedTask, _, cx| {
                let id = drag.id;
                state.update(cx, |s, cx| s.set_status(id, status, cx));
            }
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().size(px(8.)).rounded_full().bg(color))
                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(status.label()))
                .child(div().text_xs().text_color(c.muted).child(tasks.len().to_string())),
        )
        .child(
            div()
                .id(("cards", status as usize))
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_2()
                .children(tasks.into_iter().map(|t| card(state, data, t, c))),
        )
}

fn card(state: &Entity<AppState>, data: &Data, t: &Task, c: &Colors) -> impl IntoElement {
    let id = t.id;
    let hover_border = c.muted;
    // Keyed by column too, so a card dropped on another column fades in there.
    let card = div()
        .id(id)
        .p_3()
        .rounded_lg()
        .bg(c.bg)
        .border_1()
        .border_color(c.border)
        .flex()
        .flex_col()
        .gap_2()
        .cursor_pointer()
        .hover(move |s| s.border_color(hover_border))
        .on_click({
            let state = state.clone();
            move |_, _, cx| state.update(cx, |s, cx| s.edit(id, cx))
        })
        .on_drag(DraggedTask { id, title: t.title.clone().into(), colors: *c }, |drag, _, _, cx| {
            cx.new(|_| drag.clone())
        })
        .child(div().text_sm().child(t.title.clone()))
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_1p5()
                .child(chip(t.priority.label(), priority_color(t.priority)))
                .items_center()
                .when_some(t.due_date, |d, due| d.child(icon_chip(icons::CALENDAR, views::format_date(due), c.muted)))
                .when_some(t.recurrence.as_ref(), |d, rule| d.child(icon_chip(icons::REPEAT, recurrence::label(rule), c.muted)))
                .children(sharing_badges(data, t, c)),
        );
    motion::enter_once("kanban", (id, t.status), card)
}
