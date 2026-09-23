//! Task list: search, priority and tag filters, drag to reorder, click to edit, two-click delete.

use crate::model::{Priority, Status, Task};
use crate::state::{AppState, Page};
use crate::theme::{self, Colors, priority_color, status_color};
use crate::ui::text_input::{TextEvent, TextInput};
use crate::ui::icons::{self, icon};
use crate::ui::markdown;
use crate::ui::widgets::{DraggedTask, chip, icon_chip, page_title, primary_button, segmented};
use crate::views::{self, ListFilter};
use chrono::{DateTime, Utc};
use gpui::{
    AnyElement, App, Context, Entity, FontWeight, IntoElement, Render, SharedString, Subscription, Window, div,
    prelude::*, px, white,
};
use uuid::Uuid;

pub struct ListPage {
    filter: ListFilter,
    search: Entity<TextInput>,
    /// Task whose delete button was clicked once; the second click deletes it.
    confirm_delete: Option<Uuid>,
    _subscriptions: Vec<Subscription>,
}

impl ListPage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| TextInput::new("Ara...", false, cx));
        let state = AppState::global(cx);
        let subscriptions = vec![
            cx.subscribe(&search, |this, search, event: &TextEvent, cx| {
                if let TextEvent::Changed = event {
                    this.filter.query = search.read(cx).text().to_string();
                    cx.notify();
                }
            }),
            cx.observe(&state, |_, _, cx| cx.notify()),
        ];
        ListPage { filter: ListFilter::default(), search, confirm_delete: None, _subscriptions: subscriptions }
    }

    fn card(&self, t: &Task, c: &Colors, now: DateTime<Utc>, cx: &Context<Self>) -> impl IntoElement {
        let id = t.id;
        let state = AppState::global(cx);
        let done = t.status == Status::Completed;
        let confirming = self.confirm_delete == Some(id);
        let (accent, hover_border) = (c.accent, c.muted);
        let dot = status_color(t.status);
        let next_status = t.status.next();
        let subtasks_done = t.subtasks.iter().filter(|s| s.completed).count();
        let overdue = views::is_overdue(t, now);
        let summary = markdown::summary(&t.description);

        div()
            .id(id)
            .flex()
            .items_start()
            .gap_3()
            .p_3()
            .rounded_xl()
            .bg(c.surface)
            .border_1()
            .border_color(c.border)
            .cursor_pointer()
            .hover(move |s| s.border_color(hover_border))
            .on_click({
                let state = state.clone();
                move |_, _, cx| state.update(cx, |s, cx| s.edit(id, cx))
            })
            .on_drag(DraggedTask { id, title: t.title.clone().into(), colors: *c }, |drag, _, _, cx| {
                cx.new(|_| drag.clone())
            })
            .drag_over::<DraggedTask>(move |s, _, _, _| s.border_color(accent))
            .on_drop({
                let state = state.clone();
                move |drag: &DraggedTask, _, cx| {
                    let dragged = drag.id;
                    state.update(cx, |s, cx| {
                        let _ = s.mutate(cx, |d| d.move_to(dragged, id));
                    });
                }
            })
            .child(
                div()
                    .id("status")
                    .mt_0p5()
                    .flex_none()
                    .size(px(18.))
                    .rounded_full()
                    .border_2()
                    .border_color(dot)
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(done, |d| d.bg(dot).text_color(white()).text_xs().child(icon(icons::CHECK)))
                    .when(t.status == Status::InProgress, |d| d.child(div().size(px(8.)).rounded_full().bg(dot)))
                    .on_click({
                        let state = state.clone();
                        move |_, _, cx| {
                            cx.stop_propagation();
                            state.update(cx, |s, cx| {
                                let _ = s.mutate(cx, |d| d.set_status(id, next_status, Utc::now()));
                            });
                        }
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .when(done, |d| d.line_through().text_color(c.muted))
                            .child(t.title.clone()),
                    )
                    .when(!summary.is_empty(), |d| d.child(div().text_xs().text_color(c.muted).truncate().child(summary)))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_1p5()
                            .child(chip(t.priority.label(), priority_color(t.priority)))
                            .when_some(t.due_date, |d, due| {
                                d.child(icon_chip(icons::CALENDAR, views::format_date(due), if overdue { c.danger } else { c.muted }))
                            })
                            .when_some(t.reminder.as_ref().filter(|r| r.enabled), |d, r| {
                                d.child(icon_chip(icons::BELL, views::format_date_time(r.next_trigger), c.accent))
                            })
                            .when(!t.subtasks.is_empty(), |d| {
                                d.child(icon_chip(icons::LIST, format!("{}/{}", subtasks_done, t.subtasks.len()), c.muted))
                            })
                            .children(t.tags.iter().map(|tag| chip(format!("#{tag}"), c.muted))),
                    ),
            )
            .child(
                div()
                    .id("delete")
                    .flex_none()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .text_xs()
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap_1()
                    .when(confirming, |d| d.bg(c.danger).text_color(white()).child(icon(icons::TRASH)).child("Emin misin?"))
                    .when(!confirming, |d| d.text_color(c.muted).child(icon(icons::TRASH)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if this.confirm_delete == Some(id) {
                            this.confirm_delete = None;
                            AppState::global(cx).update(cx, |s, cx| {
                                let _ = s.mutate(cx, |d| d.delete_task(id));
                            });
                        } else {
                            this.confirm_delete = Some(id);
                        }
                        cx.notify();
                    })),
            )
    }
}

impl Render for ListPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme::current(cx);
        let state = AppState::global(cx);
        let this = cx.entity();
        let now = Utc::now();
        let data = &state.read(cx).data;
        let visible = views::filter_tasks(&data.tasks, &self.filter);
        let tags = views::all_tags(&data.tasks);
        let total = data.tasks.len();
        let cards: Vec<AnyElement> = visible.iter().map(|t| self.card(t, &c, now, cx).into_any_element()).collect();

        let priority_options = [
            (None, "Tümü"),
            (Some(Priority::High), "Yüksek"),
            (Some(Priority::Medium), "Orta"),
            (Some(Priority::Low), "Düşük"),
        ];
        let set_priority = {
            let this = this.clone();
            move |p: Option<Priority>, _: &mut Window, cx: &mut App| {
                this.update(cx, |this, cx| {
                    this.filter.priority = p;
                    cx.notify();
                })
            }
        };
        let tag_chips = tags.into_iter().map(|tag| {
            let active = self.filter.tag.as_deref() == Some(tag.as_str());
            let this = this.clone();
            div()
                .id(SharedString::from(format!("tag-{tag}")))
                .cursor_pointer()
                .child(chip(format!("#{tag}"), if active { c.accent } else { c.muted }))
                .on_click(move |_, _, cx| {
                    let tag = tag.clone();
                    this.update(cx, |this, cx| {
                        this.filter.tag = if this.filter.tag.as_ref() == Some(&tag) { None } else { Some(tag) };
                        cx.notify();
                    })
                })
        });

        let body = if cards.is_empty() {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(c.muted)
                .child(if total == 0 {
                    "Henüz görev yok. Ctrl+N ile yeni görev ekle."
                } else {
                    "Filtreye uyan görev yok."
                })
                .into_any_element()
        } else {
            // Note: plain scroll list, not virtualized; switch to gpui::list if there are thousands of tasks.
            div()
                .id("task-list")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_2()
                .children(cards)
                .into_any_element()
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_4()
            .p_6()
            .child(page_title(format!("Görevler ({total})"), &c).child(primary_button("new-task", "Yeni Görev", &c).on_click({
                let state = state.clone();
                move |_, _, cx| state.update(cx, |s, cx| s.navigate(Page::Form, cx))
            })))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .flex_wrap()
                    .items_center()
                    .gap_3()
                    .child(div().w(px(240.)).child(self.search.clone()))
                    .child(segmented("priority-filter", &priority_options, self.filter.priority, &c, set_priority))
                    .children(tag_chips),
            )
            .child(body)
    }
}
