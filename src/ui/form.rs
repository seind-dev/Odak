//! Create/edit form for one task.

use crate::data::{DataError, TaskDraft};
use crate::model::{Priority, Repeat, Status, SubTask};
use crate::state::{AppState, Page};
use crate::theme::{self, Colors};
use crate::ui::datetime_picker::DateTimePicker;
use crate::ui::icons;
use crate::ui::markdown;
use crate::ui::text_input::{TextEvent, TextInput};
use crate::ui::widgets::{
    button, checkbox, chip, field, icon_chip, page_title, pill, primary_button, segmented, user_avatar, user_name,
};
use chrono::Utc;
use gpui::{
    App, Context, Div, Entity, FocusHandle, Focusable, FontWeight, IntoElement, Render, Subscription, Window, div, prelude::*,
    px,
};
use uuid::Uuid;

pub struct FormPage {
    /// Task being edited; `None` for a new task.
    pub editing: Option<Uuid>,
    title: Entity<TextInput>,
    description: Entity<TextInput>,
    tag_input: Entity<TextInput>,
    subtask_input: Entity<TextInput>,
    due: Entity<DateTimePicker>,
    reminder: Entity<DateTimePicker>,
    priority: Priority,
    status: Status,
    repeat: Repeat,
    tags: Vec<String>,
    subtasks: Vec<SubTask>,
    /// `None`: a personal task.
    group: Option<Uuid>,
    assignee: Option<Uuid>,
    error: Option<String>,
    /// Description shows rendered Markdown instead of the editor.
    preview: bool,
    _subscriptions: Vec<Subscription>,
}

fn text_input(placeholder: &'static str, multiline: bool, text: &str, cx: &mut Context<TextInput>) -> TextInput {
    let mut input = TextInput::new(placeholder, multiline, cx);
    input.set_text(text, cx);
    input
}

impl FormPage {
    pub fn new(editing: Option<Uuid>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let draft = editing
            .and_then(|id| AppState::global(cx).read(cx).data.task(id).map(TaskDraft::from_task))
            .unwrap_or_default();
        let title = cx.new(|cx| text_input("Görev başlığı", false, &draft.title, cx));
        let description = cx.new(|cx| text_input("Açıklama (isteğe bağlı)", true, &draft.description, cx));
        let tag_input = cx.new(|cx| TextInput::new("Etiket yaz, Enter'a bas", false, cx));
        let subtask_input = cx.new(|cx| TextInput::new("Alt görev yaz, Enter'a bas", false, cx));
        let due = cx.new(|_| DateTimePicker::new(draft.due_date, false, "Son tarih yok"));
        let reminder = cx.new(|_| DateTimePicker::new(draft.reminder.map(|(at, _)| at), true, "Hatırlatıcı yok"));
        let subscriptions = vec![
            cx.subscribe(&title, |this, _, event: &TextEvent, cx| {
                if let TextEvent::Submit = event {
                    this.save(cx);
                }
            }),
            cx.subscribe(&tag_input, |this, _, event: &TextEvent, cx| {
                if let TextEvent::Submit = event {
                    this.take_tags(cx);
                    cx.notify();
                }
            }),
            cx.subscribe(&subtask_input, |this, _, event: &TextEvent, cx| {
                if let TextEvent::Submit = event {
                    this.take_subtask(cx);
                    cx.notify();
                }
            }),
        ];
        let focus = title.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        FormPage {
            editing,
            title,
            description,
            tag_input,
            subtask_input,
            due,
            reminder,
            priority: draft.priority,
            status: draft.status,
            repeat: draft.reminder.map_or(Repeat::Once, |(_, repeat)| repeat),
            tags: draft.tags,
            subtasks: draft.subtasks,
            group: draft.group_id,
            assignee: draft.assignee_id,
            error: None,
            preview: false,
            _subscriptions: subscriptions,
        }
    }

    /// Focus handle of the title input (focused again when the search palette closes over the form).
    pub fn title_focus(&self, cx: &App) -> FocusHandle {
        self.title.read(cx).focus_handle(cx)
    }

    /// Moves comma-separated tags typed in the tag box into the tag list.
    fn take_tags(&mut self, cx: &mut Context<Self>) {
        let text = self.tag_input.read(cx).text().to_string();
        for tag in text.split(',').map(str::trim).filter(|t| !t.is_empty()) {
            if !self.tags.iter().any(|t| t == tag) {
                self.tags.push(tag.to_string());
            }
        }
        self.tag_input.update(cx, |input, cx| input.set_text("", cx));
    }

    /// Moves the text in the subtask box into the subtask list.
    fn take_subtask(&mut self, cx: &mut Context<Self>) {
        let title = self.subtask_input.read(cx).text().trim().to_string();
        if !title.is_empty() {
            self.subtasks.push(SubTask { id: Uuid::new_v4(), title, completed: false });
            self.subtask_input.update(cx, |input, cx| input.set_text("", cx));
        }
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        // Text left in the tag/subtask boxes counts as if Enter had been pressed.
        self.take_tags(cx);
        self.take_subtask(cx);
        let draft = TaskDraft {
            title: self.title.read(cx).text().to_string(),
            description: self.description.read(cx).text().trim_end().to_string(),
            priority: self.priority,
            status: self.status,
            due_date: self.due.read(cx).value(),
            reminder: self.reminder.read(cx).value().map(|at| (at, self.repeat)),
            tags: self.tags.clone(),
            subtasks: self.subtasks.clone(),
            group_id: self.group,
            assignee_id: self.assignee,
        };
        if draft.title.trim().is_empty() {
            self.error = Some(DataError::EmptyTitle.to_string());
            cx.notify();
            return;
        }
        let editing = self.editing;
        let state = AppState::global(cx);
        let result = state.update(cx, |s, cx| {
            s.mutate(cx, |d| match editing {
                Some(id) => d.update_task(id, draft, Utc::now()),
                None => d.add_task(draft, Utc::now()).map(|_| ()),
            })
        });
        match result {
            Ok(()) => state.update(cx, |s, cx| s.navigate(Page::List, cx)),
            Err(e) => {
                self.error = Some(e.to_string());
                cx.notify();
            }
        }
    }

    /// Moves the task to a group (or back to personal); an assignee outside the group is cleared.
    fn set_group(&mut self, group: Option<Uuid>, cx: &mut Context<Self>) {
        let data = &AppState::global(cx).read(cx).data;
        let members = group.and_then(|g| data.group(g)).map(|g| g.members.clone()).unwrap_or_default();
        self.group = group;
        self.assignee = self.assignee.filter(|a| members.contains(a));
        cx.notify();
    }

    /// Group and assignee pickers; `None` when the account has no groups (nothing to pick).
    fn sharing_fields(&self, c: &Colors, cx: &Context<Self>) -> Option<Div> {
        let data = &AppState::global(cx).read(cx).data;
        if data.groups.is_empty() && self.group.is_none() {
            return None;
        }
        // Only the owner may move a task between groups (the server enforces it too).
        let owner = self.editing.and_then(|id| data.task(id)).and_then(|t| t.owner_id);
        let can_move = owner.is_none() || owner == data.me();
        let group_name = |id: Uuid| data.group(id).map_or_else(|| "Grup".to_string(), |g| g.name.clone());
        let group_picker = if can_move {
            let options = std::iter::once((None, "Kişisel".to_string()))
                .chain(data.groups.iter().map(|g| (Some(g.id), g.name.clone())));
            div()
                .flex()
                .flex_wrap()
                .gap_1p5()
                .children(options.enumerate().map(|(ix, (id, name))| {
                    pill(("group", ix), self.group == id, c)
                        .when(id.is_some(), |d| d.child(icons::icon(icons::USERS)))
                        .child(name)
                        .on_click(cx.listener(move |this, _, _, cx| this.set_group(id, cx)))
                }))
                .into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(icon_chip(icons::USERS, self.group.map_or("Kişisel".into(), group_name), c.accent))
                .child(div().text_xs().text_color(c.muted).child("Grubu yalnızca görevin sahibi değiştirebilir"))
                .into_any_element()
        };
        let members = self.group.and_then(|g| data.group(g)).map(|g| g.members.clone()).unwrap_or_default();
        let assignee_picker = self.group.is_some().then(|| {
            let options = std::iter::once(None).chain(members.iter().copied().map(Some));
            div().flex().flex_wrap().gap_1p5().children(options.enumerate().map(|(ix, id)| {
                pill(("assignee", ix), self.assignee == id, c)
                    .map(|d| match id {
                        Some(id) => d.child(user_avatar(data, id, px(18.), c)).child(user_name(data, id)),
                        None => d.child("Kimse"),
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.assignee = id;
                        cx.notify();
                    }))
            }))
        });
        Some(
            div()
                .flex()
                .flex_col()
                .gap_5()
                .child(field("Grup", group_picker, c))
                .when_some(assignee_picker, |d, picker| d.child(field("Atanan", picker, c))),
        )
    }

    fn subtask_row(&self, ix: usize, s: &SubTask, c: &Colors, cx: &Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(checkbox(("subtask-check", ix), s.completed, c).on_click(cx.listener(move |this, _, _, cx| {
                if let Some(s) = this.subtasks.get_mut(ix) {
                    s.completed = !s.completed;
                }
                cx.notify();
            })))
            .child(div().flex_1().text_sm().when(s.completed, |d| d.line_through().text_color(c.muted)).child(s.title.clone()))
            .child(
                div()
                    .id(("subtask-remove", ix))
                    .px_1()
                    .text_xs()
                    .text_color(c.muted)
                    .cursor_pointer()
                    .child("✕")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if ix < this.subtasks.len() {
                            this.subtasks.remove(ix);
                        }
                        cx.notify();
                    })),
            )
    }
}

impl Render for FormPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme::current(cx);
        let this = cx.entity();
        let has_reminder = self.reminder.read(cx).value().is_some();
        let priorities = Priority::ALL.map(|p| (p, p.label()));
        let statuses = Status::ALL.map(|s| (s, s.label()));
        let repeats = Repeat::ALL.map(|r| (r, r.label()));

        let priority = segmented("priority", &priorities, self.priority, &c, {
            let this = this.clone();
            move |p, _, cx| {
                this.update(cx, |this, cx| {
                    this.priority = p;
                    cx.notify();
                })
            }
        });
        let status = segmented("status", &statuses, self.status, &c, {
            let this = this.clone();
            move |s, _, cx| {
                this.update(cx, |this, cx| {
                    this.status = s;
                    cx.notify();
                })
            }
        });
        let repeat = segmented("repeat", &repeats, self.repeat, &c, {
            let this = this.clone();
            move |r, _, cx| {
                this.update(cx, |this, cx| {
                    this.repeat = r;
                    cx.notify();
                })
            }
        });
        let tag_chips = self.tags.iter().enumerate().map(|(ix, tag)| {
            div()
                .id(("tag", ix))
                .cursor_pointer()
                .child(chip(format!("#{tag}  ✕"), c.accent))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if ix < this.tags.len() {
                        this.tags.remove(ix);
                    }
                    cx.notify();
                }))
        });
        let subtask_rows: Vec<_> = self.subtasks.iter().enumerate().map(|(ix, s)| self.subtask_row(ix, s, &c, cx)).collect();
        let description_text = self.description.read(cx).text().to_string();
        let preview_box = || div().min_h(px(96.)).px_3().py_2().rounded_lg().border_1().border_color(c.border);
        let description_body = if !self.preview {
            self.description.clone().into_any_element()
        } else if description_text.trim().is_empty() {
            preview_box().text_sm().text_color(c.muted).child("Önizlenecek içerik yok").into_any_element()
        } else {
            preview_box()
                .child(markdown::render(&markdown::parse(&description_text), "description", &c))
                .into_any_element()
        };
        let description = div()
            .flex()
            .flex_col()
            .gap_1p5()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_xs().font_weight(FontWeight::MEDIUM).text_color(c.muted).child("Açıklama (Markdown)"))
                    .child(
                        div()
                            .id("preview-toggle")
                            .text_xs()
                            .text_color(c.accent)
                            .cursor_pointer()
                            .child(if self.preview { "Düzenle" } else { "Önizle" })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.preview = !this.preview;
                                cx.notify();
                            })),
                    ),
            )
            .child(description_body);

        div().id("form").size_full().overflow_y_scroll().child(
            div()
                .max_w(px(680.))
                .p_6()
                .flex()
                .flex_col()
                .gap_5()
                .child(page_title(if self.editing.is_some() { "Görevi Düzenle" } else { "Yeni Görev" }, &c))
                .child(field("Başlık", self.title.clone(), &c))
                .child(description)
                .child(field("Öncelik", priority, &c))
                .when_some(self.sharing_fields(&c, cx), |d, fields| d.child(fields))
                .when(self.editing.is_some(), |d| d.child(field("Durum", status, &c)))
                .child(field("Son tarih", self.due.clone(), &c))
                .child(field(
                    "Hatırlatıcı",
                    div().flex().flex_col().gap_2().child(self.reminder.clone()).when(has_reminder, |d| d.child(repeat)),
                    &c,
                ))
                .child(field(
                    "Etiketler",
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(self.tag_input.clone())
                        .child(div().flex().flex_wrap().gap_1p5().children(tag_chips)),
                    &c,
                ))
                .child(field(
                    "Alt görevler",
                    div().flex().flex_col().gap_2().children(subtask_rows).child(self.subtask_input.clone()),
                    &c,
                ))
                .when_some(self.error.clone(), |d, e| d.child(div().text_sm().text_color(c.danger).child(e)))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(primary_button("save", "Kaydet", &c).on_click(cx.listener(|this, _, _, cx| this.save(cx))))
                        .child(button("cancel", "İptal", &c).on_click(|_, _, cx| {
                            AppState::global(cx).update(cx, |s, cx| s.navigate(Page::List, cx))
                        })),
                ),
        )
    }
}
