//! Ctrl+K search palette: quick commands, task search, and `>` command mode.

use crate::model::Status;
use crate::state::{AppState, Page};
use crate::theme::{self, priority_color, status_color};
use crate::ui::icons::{self, icon};
use crate::ui::markdown;
use crate::ui::text_input::{TextEvent, TextInput};
use crate::views;
use gpui::{
    App, Context, Entity, EventEmitter, Focusable, FontWeight, IntoElement, MouseButton, Render, Subscription, Window,
    div, prelude::*, px,
};
use uuid::Uuid;

/// Tasks listed for a search query.
const MAX_RESULTS: usize = 10;

struct Command {
    label: &'static str,
    glyph: &'static str,
    page: Page,
}

const COMMANDS: [Command; 7] = [
    Command { label: "Yeni Görev Oluştur", glyph: icons::ADD, page: Page::Form },
    Command { label: "Dashboard", glyph: icons::APPS, page: Page::Dashboard },
    Command { label: "Görevler", glyph: icons::LIST, page: Page::List },
    Command { label: "Kanban Panosu", glyph: icons::KANBAN, page: Page::Kanban },
    Command { label: "Takvim", glyph: icons::CALENDAR, page: Page::Calendar },
    Command { label: "Bildirimler", glyph: icons::BELL, page: Page::Notifications },
    Command { label: "Ayarlar", glyph: icons::SETTINGS, page: Page::Settings },
];

#[derive(Clone, Copy)]
enum Entry {
    Command(usize),
    Task(Uuid),
}

pub enum PaletteEvent {
    Dismissed,
}

pub struct SearchPalette {
    input: Entity<TextInput>,
    selected: usize,
    _subscription: Subscription,
}

impl SearchPalette {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::new("Görev ara veya > komut yaz...", false, cx));
        let subscription = cx.subscribe(&input, |this, _, event: &TextEvent, cx| match event {
            TextEvent::Changed => {
                this.selected = 0;
                cx.notify();
            }
            TextEvent::Up => {
                this.selected = this.selected.saturating_sub(1);
                cx.notify();
            }
            TextEvent::Down => {
                let last = this.entries(cx).len().saturating_sub(1);
                this.selected = (this.selected + 1).min(last);
                cx.notify();
            }
            TextEvent::Submit => {
                if let Some(&entry) = this.entries(cx).get(this.selected) {
                    this.activate(entry, cx);
                }
            }
            TextEvent::Cancel => cx.emit(PaletteEvent::Dismissed),
        });
        let focus = input.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        SearchPalette { input, selected: 0, _subscription: subscription }
    }

    /// (command mode, query without the `>` prefix, trimmed)
    fn query(&self, cx: &App) -> (bool, String) {
        let raw = self.input.read(cx).text();
        match raw.strip_prefix('>') {
            Some(rest) => (true, rest.trim().to_string()),
            None => (false, raw.trim().to_string()),
        }
    }

    fn entries(&self, cx: &App) -> Vec<Entry> {
        let (command_mode, query) = self.query(cx);
        if command_mode || query.is_empty() {
            let q = query.to_lowercase();
            COMMANDS
                .iter()
                .enumerate()
                .filter(|(_, command)| command.label.to_lowercase().contains(&q))
                .map(|(ix, _)| Entry::Command(ix))
                .collect()
        } else {
            let state = AppState::global(cx);
            views::search_tasks(&state.read(cx).data.tasks, &query, MAX_RESULTS).iter().map(|t| Entry::Task(t.id)).collect()
        }
    }

    fn activate(&mut self, entry: Entry, cx: &mut Context<Self>) {
        let state = AppState::global(cx);
        match entry {
            Entry::Command(ix) => state.update(cx, |s, cx| s.navigate(COMMANDS[ix].page, cx)),
            Entry::Task(id) => state.update(cx, |s, cx| s.edit(id, cx)),
        }
        cx.emit(PaletteEvent::Dismissed);
    }
}

impl EventEmitter<PaletteEvent> for SearchPalette {}

impl Render for SearchPalette {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme::current(cx);
        let entries = self.entries(cx);
        let (command_mode, query) = self.query(cx);
        let state = AppState::global(cx);
        let tasks = &state.read(cx).data.tasks;
        let rows = entries.iter().enumerate().map(|(ix, &entry)| {
            let row = div()
                .id(("palette-row", ix))
                .flex()
                .items_center()
                .gap_3()
                .px_4()
                .py_2()
                .cursor_pointer()
                .when(ix == self.selected, |d| d.bg(c.hover))
                .on_mouse_move(cx.listener(move |this, _, _, cx| {
                    if this.selected != ix {
                        this.selected = ix;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, _, cx| this.activate(entry, cx)));
            match entry {
                Entry::Command(i) => row
                    .child(icon(COMMANDS[i].glyph).text_color(c.muted))
                    .child(div().text_sm().child(COMMANDS[i].label)),
                Entry::Task(id) => {
                    let Some(t) = tasks.iter().find(|t| t.id == id) else { return row };
                    let done = t.status == Status::Completed;
                    let glyph = match t.status {
                        Status::Pending => icons::CLOCK,
                        Status::InProgress => icons::REFRESH,
                        Status::Completed => icons::CHECK,
                    };
                    let summary = markdown::summary(&t.description);
                    row.child(icon(glyph).text_color(status_color(t.status)))
                        .child(div().flex_none().size(px(8.)).rounded_full().bg(priority_color(t.priority)))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_sm()
                                        .truncate()
                                        .when(done, |d| d.line_through().text_color(c.muted))
                                        .child(t.title.clone()),
                                )
                                .when(!summary.is_empty(), |d| {
                                    d.child(div().text_xs().text_color(c.muted).truncate().child(summary))
                                }),
                        )
                }
            }
        });
        let empty_text = if command_mode || query.is_empty() { "Komut bulunamadı" } else { "Sonuç bulunamadı" };

        div()
            .id("palette")
            .w(px(560.))
            .max_h(px(460.))
            .flex()
            .flex_col()
            .rounded_xl()
            .bg(c.surface)
            .border_1()
            .border_color(c.border)
            .shadow_lg()
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(c.border)
                    .child(icon(icons::SEARCH).text_color(c.muted))
                    .child(div().flex_1().child(self.input.clone()))
                    .child(div().px_1p5().py_0p5().rounded_md().bg(c.hover).text_xs().text_color(c.muted).child("ESC")),
            )
            .child(
                div()
                    .id("palette-results")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .py_2()
                    .when(!command_mode && query.is_empty(), |d| {
                        d.child(
                            div()
                                .px_4()
                                .py_1()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(c.muted)
                                .child("HIZLI ERİŞİM"),
                        )
                    })
                    .when(entries.is_empty(), |d| {
                        d.child(div().py_6().text_center().text_sm().text_color(c.muted).child(empty_text))
                    })
                    .children(rows),
            )
            .child(
                div()
                    .flex()
                    .gap_4()
                    .px_4()
                    .py_2()
                    .border_t_1()
                    .border_color(c.border)
                    .text_xs()
                    .text_color(c.muted)
                    .child("↑↓ gezin")
                    .child("↵ seç")
                    .child("> komutlar")
                    .child("esc kapat"),
            )
    }
}
