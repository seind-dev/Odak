//! Bildirimler: every pop-up shown so far (newest first), with read state.

use crate::model::Notice;
use crate::state::AppState;
use crate::theme::{Colors, priority_color};
use crate::ui::icons::{self, icon};
use crate::ui::motion;
use crate::ui::widgets::{button, page_title};
use crate::views;
use chrono::{DateTime, Utc};
use gpui::{App, Entity, FontWeight, Hsla, IntoElement, div, prelude::*, px};

pub fn render(c: &Colors, cx: &App) -> impl IntoElement {
    let state = AppState::global(cx);
    let notices = &state.read(cx).data.notices;
    let unread = notices.iter().filter(|n| !n.read).count();
    let now = Utc::now();
    let actions = div()
        .flex()
        .gap_2()
        .when(unread > 0, |d| {
            d.child(button("read-all", "Tümünü okundu say", c).on_click({
                let state = state.clone();
                move |_, _, cx| state.update(cx, |s, cx| s.mutate(cx, |d| d.mark_all_notices_read()))
            }))
        })
        .when(!notices.is_empty(), |d| {
            d.child(button("clear-all", "Tümünü Temizle", c).on_click({
                let state = state.clone();
                move |_, _, cx| state.update(cx, |s, cx| s.mutate(cx, |d| d.clear_notices()))
            }))
        });
    let body = if notices.is_empty() {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .child(icon(icons::BELL).size(px(56.)).rounded_2xl().bg(c.surface).text_2xl().text_color(c.muted))
            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Bildirim yok"))
            .child(div().text_xs().text_color(c.muted).child("Henüz hiç bildirim almadınız"))
            .into_any_element()
    } else {
        div()
            .id("notices")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1()
            .children(notices.iter().map(|n| row(n, &state, now, c)))
            .into_any_element()
    };
    div()
        .size_full()
        .flex()
        .flex_col()
        .gap_4()
        .p_6()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(page_title("Bildirimler", c).child(actions))
                .child(div().text_xs().text_color(c.muted).child(if unread > 0 {
                    format!("{unread} okunmamış")
                } else {
                    "Tüm bildirimler okundu".to_string()
                })),
        )
        .child(body)
}

fn row(n: &Notice, state: &Entity<AppState>, now: DateTime<Utc>, c: &Colors) -> impl IntoElement {
    let (id, task_id) = (n.id, n.task_id);
    let glyph = icons::for_notice(n.kind);
    let dot = n.priority.map(priority_color).unwrap_or(c.muted);
    let hover = c.hover;
    let row = div()
        .id(id)
        .flex()
        .items_start()
        .gap_3()
        .px_4()
        .py_3()
        .rounded_lg()
        .cursor_pointer()
        .when(!n.read, |d| d.bg(Hsla::from(c.accent).opacity(0.07)))
        .hover(move |s| s.bg(hover))
        .on_click({
            let state = state.clone();
            move |_, _, cx| {
                state.update(cx, |s, cx| {
                    s.mutate(cx, |d| d.mark_notice_read(id));
                    if let Some(task) = task_id.filter(|t| s.data.task(*t).is_some()) {
                        s.edit(task, cx);
                    }
                })
            }
        })
        .child(icon(glyph).mt_0p5().text_color(c.muted))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().flex_none().size(px(8.)).rounded_full().bg(dot))
                        .child(
                            div()
                                .text_sm()
                                .truncate()
                                .font_weight(if n.read { FontWeight::NORMAL } else { FontWeight::SEMIBOLD })
                                .text_color(if n.read { c.muted } else { c.text })
                                .child(n.title.clone()),
                        )
                        .when(!n.read, |d| d.child(div().flex_none().size(px(8.)).rounded_full().bg(c.accent))),
                )
                .when(!n.body.is_empty(), |d| d.child(div().text_xs().text_color(c.muted).truncate().child(n.body.clone()))),
        )
        .child(div().flex_none().text_xs().text_color(c.muted).child(views::time_ago(n.at, now)));
    motion::enter_once("notice", id, row)
}
