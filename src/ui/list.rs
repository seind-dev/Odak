//! Task list: search, priority and tag filters, drag to reorder, click to edit, delete with undo.
//! With the list focused, the keyboard drives it: j/k or arrows move, Enter opens, Space moves
//! the status on, 1-3 set the priority, Delete deletes, "/" jumps to the search box.

use crate::model::{Priority, Status, Task};
use crate::recurrence;
use crate::state::{AppState, Page};
use crate::theme::{self, Colors, priority_color, status_color};
use crate::ui::icons::{self, icon};
use crate::ui::markdown;
use crate::ui::motion;
use crate::ui::text_input::{TextEvent, TextInput};
use crate::ui::widgets::{DraggedTask, chip, icon_chip, page_title, pill, primary_button, segmented, sharing_badges};
use crate::views::{self, ListFilter, Scope};
use chrono::{DateTime, Utc};
use gpui::{
    AnyElement, App, Context, Entity, FocusHandle, Focusable, FontWeight, IntoElement, KeyBinding, Render, ScrollHandle,
    SharedString, Subscription, Window, actions, div, prelude::*, px, white,
};
use uuid::Uuid;

actions!(task_list, [SelectNext, SelectPrevious, OpenSelected, CycleStatus, PriorityHigh, PriorityMedium, PriorityLow, DeleteSelected, FocusSearch]);

const CONTEXT: &str = "TaskList";

/// Keys only act while the list itself has focus, so typing in the search box is unaffected.
pub fn bind_keys(cx: &mut App) {
    let ctx = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("j", SelectNext, ctx),
        KeyBinding::new("down", SelectNext, ctx),
        KeyBinding::new("k", SelectPrevious, ctx),
        KeyBinding::new("up", SelectPrevious, ctx),
        KeyBinding::new("enter", OpenSelected, ctx),
        KeyBinding::new("space", CycleStatus, ctx),
        KeyBinding::new("1", PriorityHigh, ctx),
        KeyBinding::new("2", PriorityMedium, ctx),
        KeyBinding::new("3", PriorityLow, ctx),
        KeyBinding::new("delete", DeleteSelected, ctx),
        KeyBinding::new("backspace", DeleteSelected, ctx),
        KeyBinding::new("/", FocusSearch, ctx),
    ]);
}

pub struct ListPage {
    filter: ListFilter,
    search: Entity<TextInput>,
    focus: FocusHandle,
    /// Keyboard selection (shown while the list has focus).
    selected: Option<Uuid>,
    scroll: ScrollHandle,
    /// Set from the search box (Esc, ↓); applied on the next render, which has the window.
    focus_requested: bool,
    _subscriptions: Vec<Subscription>,
}

impl Focusable for ListPage {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ListPage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| TextInput::new("Ara...", false, cx));
        let state = AppState::global(cx);
        let subscriptions = vec![
            cx.subscribe(&search, |this, search, event: &TextEvent, cx| match event {
                TextEvent::Changed => {
                    this.filter.query = search.read(cx).text().to_string();
                    cx.notify();
                }
                // Down or Esc in the search box hands the keyboard to the list.
                TextEvent::Down | TextEvent::Cancel => {
                    if this.selected.is_none() {
                        this.select_by(1, cx);
                    }
                    this.focus_requested = true;
                    cx.notify();
                }
                _ => {}
            }),
            cx.observe(&state, |_, _, cx| cx.notify()),
        ];
        ListPage {
            filter: ListFilter::default(),
            search,
            focus: cx.focus_handle(),
            selected: None,
            scroll: ScrollHandle::new(),
            focus_requested: false,
            _subscriptions: subscriptions,
        }
    }

    fn visible_ids(&self, cx: &App) -> Vec<Uuid> {
        views::filter_tasks(&AppState::global(cx).read(cx).data.tasks, &self.filter).iter().map(|t| t.id).collect()
    }

    /// Moves the selection by `step` among the visible tasks (from either end when none is selected).
    fn select_by(&mut self, step: isize, cx: &mut Context<Self>) {
        let ids = self.visible_ids(cx);
        if ids.is_empty() {
            return;
        }
        let last = ids.len() as isize - 1;
        let at = match self.selected.and_then(|s| ids.iter().position(|id| *id == s)) {
            Some(at) => (at as isize + step).clamp(0, last),
            None if step > 0 => 0,
            None => last,
        } as usize;
        self.selected = Some(ids[at]);
        self.scroll.scroll_to_item(at);
        cx.notify();
    }

    /// The selected task, if it is still visible.
    fn current(&self, cx: &App) -> Option<Uuid> {
        self.selected.filter(|id| self.visible_ids(cx).contains(id))
    }

    fn delete_selected(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.current(cx) else { return };
        let ids = self.visible_ids(cx);
        let at = ids.iter().position(|i| *i == id).unwrap_or(0);
        AppState::global(cx).update(cx, |s, cx| s.delete_task(id, cx));
        // The next task (or the new last one) takes the selection.
        let ids = self.visible_ids(cx);
        self.selected = ids.get(at.min(ids.len().saturating_sub(1))).copied();
        cx.notify();
    }

    fn set_priority(&mut self, priority: Priority, cx: &mut Context<Self>) {
        let Some(id) = self.current(cx) else { return };
        AppState::global(cx).update(cx, |s, cx| {
            let _ = s.mutate(cx, |d| d.set_priority(id, priority, Utc::now()));
        });
    }

    fn card(&self, t: &Task, selected: bool, c: &Colors, now: DateTime<Utc>, cx: &Context<Self>) -> impl IntoElement {
        let id = t.id;
        let state = AppState::global(cx);
        let done = t.status == Status::Completed;
        let (accent, hover_border) = (c.accent, c.muted);
        let dot = status_color(t.status);
        let next_status = t.status.next();
        let subtasks_done = t.subtasks.iter().filter(|s| s.completed).count();
        let overdue = views::is_overdue(t, now);
        let summary = markdown::summary(&t.description);
        let badges = sharing_badges(&state.read(cx).data, t, c);

        let card = div()
            .id(id)
            .flex()
            .items_start()
            .gap_3()
            .p_3()
            .rounded_xl()
            .bg(c.surface)
            .border_1()
            .border_color(if selected { c.accent } else { c.border })
            .cursor_pointer()
            // Always attached (see widgets::segmented); the selection keeps its accent border.
            .hover(move |s| if selected { s } else { s.border_color(hover_border) })
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
                    // The new state's mark fades in (keyed by status, so each change replays it).
                    .when(done, |d| {
                        d.child(motion::appear(
                            motion::key("status", t.status),
                            div().size_full().rounded_full().bg(dot).flex().items_center().justify_center().text_color(white()).text_xs().child(icon(icons::CHECK)),
                            0.,
                            0.,
                        ))
                    })
                    .when(t.status == Status::InProgress, |d| {
                        d.child(motion::appear(motion::key("status", t.status), div().size(px(8.)).rounded_full().bg(dot), 0., 0.))
                    })
                    .on_click({
                        let state = state.clone();
                        move |_, _, cx| {
                            cx.stop_propagation();
                            state.update(cx, |s, cx| s.set_status(id, next_status, cx));
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
                            .when_some(t.recurrence.as_ref(), |d, rule| d.child(icon_chip(icons::REPEAT, recurrence::label(rule), c.muted)))
                            .when_some(t.reminder.as_ref().filter(|r| r.enabled), |d, r| {
                                d.child(icon_chip(icons::BELL, views::format_date_time(r.next_trigger), c.accent))
                            })
                            .when(!t.subtasks.is_empty(), |d| {
                                d.child(icon_chip(icons::LIST, format!("{}/{}", subtasks_done, t.subtasks.len()), c.muted))
                            })
                            .children(t.tags.iter().map(|tag| chip(format!("#{tag}"), c.muted)))
                            .children(badges),
                    ),
            )
            .child({
                let (muted, danger, hover) = (c.muted, c.danger, c.hover);
                div()
                    .id("delete")
                    .flex_none()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .text_xs()
                    .text_color(muted)
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover).text_color(danger))
                    .child(icon(icons::TRASH))
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        AppState::global(cx).update(cx, |s, cx| s.delete_task(id, cx));
                    })
            });
        motion::enter_once("task", id, card)
    }
}

impl Render for ListPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.focus_requested) {
            window.focus(&self.focus, cx);
        }
        let c = theme::current(cx);
        let state = AppState::global(cx);
        let this = cx.entity();
        let now = Utc::now();
        let focused = self.focus.contains_focused(window, cx);
        let data = &state.read(cx).data;
        let visible = views::filter_tasks(&data.tasks, &self.filter);
        let tags = views::all_tags(&data.tasks);
        let total = data.tasks.len();
        let selected = self.selected.filter(|_| focused);
        let cards: Vec<AnyElement> =
            visible.iter().map(|t| self.card(t, selected == Some(t.id), &c, now, cx).into_any_element()).collect();

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

        let scopes: Vec<(Scope, String)> = if data.groups.is_empty() {
            Vec::new()
        } else {
            [(Scope::All, "Tümü".to_string()), (Scope::Personal, "Kişisel".to_string())]
                .into_iter()
                .chain(data.groups.iter().map(|g| (Scope::Group(g.id), g.name.clone())))
                .collect()
        };
        let scope_pills = scopes.into_iter().enumerate().map(|(ix, (scope, label))| {
            let this = this.clone();
            pill(("scope", ix), self.filter.scope == scope, &c).child(label).on_click(move |_, _, cx| {
                this.update(cx, |this, cx| {
                    this.filter.scope = scope;
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
                .track_scroll(&self.scroll)
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
            .child(div().flex().flex_none().flex_wrap().gap_1p5().children(scope_pills))
            .child(
                // The keyboard target: the cards and their shortcuts, without the search box.
                div()
                    .id("list-keys")
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .key_context(CONTEXT)
                    .track_focus(&self.focus)
                    .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.select_by(1, cx)))
                    .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| this.select_by(-1, cx)))
                    .on_action(cx.listener(|this, _: &OpenSelected, _, cx| {
                        if let Some(id) = this.current(cx) {
                            AppState::global(cx).update(cx, |s, cx| s.edit(id, cx));
                        }
                    }))
                    .on_action(cx.listener(|this, _: &CycleStatus, _, cx| {
                        let Some(id) = this.current(cx) else { return };
                        let state = AppState::global(cx);
                        let Some(status) = state.read(cx).data.task(id).map(|t| t.status.next()) else { return };
                        state.update(cx, |s, cx| s.set_status(id, status, cx));
                    }))
                    .on_action(cx.listener(|this, _: &PriorityHigh, _, cx| this.set_priority(Priority::High, cx)))
                    .on_action(cx.listener(|this, _: &PriorityMedium, _, cx| this.set_priority(Priority::Medium, cx)))
                    .on_action(cx.listener(|this, _: &PriorityLow, _, cx| this.set_priority(Priority::Low, cx)))
                    .on_action(cx.listener(|this, _: &DeleteSelected, _, cx| this.delete_selected(cx)))
                    .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
                        let search = this.search.read(cx).focus_handle(cx);
                        window.focus(&search, cx);
                    }))
                    .child(body)
                    .when(focused && !visible.is_empty(), |d| {
                        d.child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(c.muted)
                                .child("↑↓ gezin · Enter aç · Boşluk durum · 1-3 öncelik · Del sil · / ara · Ctrl+Z geri al"),
                        )
                    }),
            )
    }
}
