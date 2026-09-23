//! Small stateless building blocks. Stateful widgets live in text_input.rs and datetime_picker.rs.

use crate::account;
use crate::fonts;
use crate::model::Profile;
use crate::state::{AppState, Auth, SyncStatus};
use crate::theme::Colors;
use crate::ui::icons::{self, icon};
use crate::views::{self, WEEKDAYS_TR};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use gpui::{
    AnyElement, App, Context, Div, ElementId, FontWeight, Hsla, IntoElement, Pixels, Render, SharedString, Stateful,
    Window, div, img, prelude::*, px, white,
};
use std::rc::Rc;
use uuid::Uuid;

/// Bordered button; chain `.on_click(...)`.
pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>, c: &Colors) -> Stateful<Div> {
    let hover = c.hover;
    div()
        .id(id)
        .flex_none()
        .px_3()
        .py_1p5()
        .rounded_lg()
        .border_1()
        .border_color(c.border)
        .bg(c.surface)
        .text_sm()
        .text_color(c.text)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .child(label.into())
}

/// Round avatar: the cached picture, or the name's first letter on an accent circle until it loads.
pub fn avatar(profile: &Profile, size: Pixels, c: &Colors) -> Div {
    let initial: SharedString = profile.name().chars().next().map_or("?".into(), |ch| ch.to_uppercase().collect::<String>()).into();
    let accent = c.accent;
    let letter = move || {
        div()
            .size_full()
            .rounded_full()
            .bg(accent)
            .flex()
            .items_center()
            .justify_center()
            .text_size(size * 0.45)
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(white())
            .child(initial.clone())
            .into_any_element()
    };
    div().flex_none().size(size).child(
        img(account::avatar_path(profile.id)).size_full().rounded_full().with_loading(letter.clone()).with_fallback(letter),
    )
}

/// Cloud glyph and text for the sync state (sidebar and Settings); `None` without an account.
pub fn sync_label(s: &AppState, now: DateTime<Utc>) -> Option<(&'static str, String)> {
    s.data.account.as_ref()?;
    let waiting = s.data.pending.len();
    let label = match (&s.auth, &s.sync) {
        (Auth::Waiting(_), _) => return None,
        (Auth::SignedOut, _) => (icons::CLOUD_DISABLED, "Tekrar giriş gerekli".into()),
        _ if s.data.needs_account_choice() => (icons::CLOUD_DISABLED, "Senkron bekliyor: Ayarlar'a bak".into()),
        (_, SyncStatus::Syncing) => (icons::REFRESH, "Senkronize ediliyor...".into()),
        (_, SyncStatus::Offline) if waiting > 0 => (icons::CLOUD_DISABLED, format!("Çevrimdışı · {waiting} değişiklik bekliyor")),
        (_, SyncStatus::Offline) => (icons::CLOUD_DISABLED, "Çevrimdışı".into()),
        (_, SyncStatus::Failed(e)) => (icons::CLOUD_DISABLED, e.clone()),
        _ if waiting > 0 => (icons::CLOUD, format!("{waiting} değişiklik bekliyor")),
        _ => match s.data.last_sync {
            Some(at) => (icons::CLOUD_CHECK, format!("Senkronize · {}", views::time_ago(at, now).replace("Az önce", "az önce"))),
            None => (icons::CLOUD, "Henüz senkronize edilmedi".into()),
        },
    };
    Some(label)
}

/// Filled accent button for a page's main action; chain `.on_click(...)`.
pub fn primary_button(id: impl Into<ElementId>, label: impl Into<SharedString>, c: &Colors) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .px_4()
        .py_1p5()
        .rounded_lg()
        .bg(c.accent)
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .text_color(white())
        .cursor_pointer()
        .hover(|s| s.opacity(0.9))
        .child(label.into())
}

/// Square bordered button showing only an icon; chain `.on_click(...)`.
pub fn icon_button(id: impl Into<ElementId>, glyph: &'static str, c: &Colors) -> Stateful<Div> {
    let hover = c.hover;
    div()
        .id(id)
        .flex_none()
        .size(px(32.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_lg()
        .border_1()
        .border_color(c.border)
        .bg(c.surface)
        .text_sm()
        .text_color(c.text)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .child(icon(glyph))
}

/// Row of mutually exclusive options, used instead of dropdowns.
pub fn segmented<T: Copy + PartialEq + 'static>(
    id: &'static str,
    options: &[(T, &'static str)],
    selected: T,
    c: &Colors,
    on_select: impl Fn(T, &mut Window, &mut App) + 'static,
) -> Div {
    let on_select = Rc::new(on_select);
    let (accent, muted, hover) = (c.accent, c.muted, c.hover);
    let group = div()
        .flex()
        .gap_0p5()
        .p_0p5()
        .rounded_lg()
        .bg(c.bg)
        .border_1()
        .border_color(c.border)
        .children(options.iter().enumerate().map(|(ix, &(value, label))| {
            let on_select = on_select.clone();
            let active = value == selected;
            div()
                .id((id, ix))
                .px_3()
                .py_1()
                .rounded_md()
                .text_sm()
                .cursor_pointer()
                .when(active, |d| d.bg(accent).text_color(white()))
                .when(!active, |d| d.text_color(muted))
                // Always attached: GPUI only tracks hover while a hover style exists, so adding it
                // only to inactive items leaves a stale highlight after the selection changes.
                .hover(move |s| if active { s } else { s.bg(hover) })
                .child(label)
                .on_click(move |_, window, cx| on_select(value, window, cx))
        }));
    // The wrapping row keeps the group at its natural width inside column layouts.
    div().flex().flex_none().child(group)
}

/// On/off switch; chain `.on_click(...)`.
pub fn toggle(id: impl Into<ElementId>, on: bool, c: &Colors) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .w(px(36.))
        .h(px(20.))
        .p(px(2.))
        .rounded_full()
        .flex()
        .when(on, |d| d.justify_end())
        .bg(if on { c.accent } else { c.border })
        .cursor_pointer()
        .child(div().size(px(16.)).rounded_full().bg(white()))
}

/// Square checkbox; chain `.on_click(...)`.
pub fn checkbox(id: impl Into<ElementId>, checked: bool, c: &Colors) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .size(px(16.))
        .rounded_sm()
        .border_1()
        .border_color(if checked { c.accent } else { c.muted })
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .cursor_pointer()
        .when(checked, |d| d.bg(c.accent).text_color(white()).child(icon(icons::CHECK)))
}

/// Small rounded label (priority, tag, status).
pub fn chip(label: impl Into<SharedString>, color: impl Into<Hsla>) -> Div {
    let color: Hsla = color.into();
    div()
        .flex_none()
        .px_2()
        .py_0p5()
        .rounded_full()
        .text_xs()
        .text_color(color)
        .bg(color.opacity(0.15))
        .child(label.into())
}

/// Chip with a leading icon (due date, reminder, subtasks).
pub fn icon_chip(glyph: &'static str, label: impl Into<SharedString>, color: impl Into<Hsla>) -> Div {
    let color: Hsla = color.into();
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap_1()
        .px_2()
        .py_0p5()
        .rounded_full()
        .text_xs()
        .text_color(color)
        .bg(color.opacity(0.15))
        .child(icon(glyph))
        .child(label.into())
}

/// Page heading row; chain `.child(...)` to add actions on the right.
pub fn page_title(title: impl Into<SharedString>, c: &Colors) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_between()
        .gap_4()
        .child(
            div()
                .font_family(fonts::DISPLAY)
                .text_lg()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(c.text)
                .child(title.into()),
        )
}

/// Labelled form row.
pub fn field(label: &'static str, content: impl IntoElement, c: &Colors) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1p5()
        .child(div().text_xs().font_weight(FontWeight::MEDIUM).text_color(c.muted).child(label))
        .child(content)
}

/// Monday-first 6x7 grid for the month starting at `first`; `cell(day, in_month)` renders each day.
pub fn month_grid(first: NaiveDate, c: &Colors, cell: &dyn Fn(NaiveDate, bool) -> AnyElement) -> Div {
    let days = views::month_days(first);
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().flex().gap_1().children(WEEKDAYS_TR.iter().map(|w| {
            div().flex_1().text_xs().text_center().text_color(c.muted).child(*w)
        })))
        .children(days.chunks(7).map(|week| {
            div().flex().flex_1().gap_1().children(
                week.iter().map(|&day| div().flex_1().min_w_0().child(cell(day, day.month() == first.month()))),
            )
        }))
}

/// Drag payload for task cards (list reorder, Kanban columns); also renders the drag preview.
#[derive(Clone)]
pub struct DraggedTask {
    pub id: Uuid,
    pub title: SharedString,
    pub colors: Colors,
}

impl Render for DraggedTask {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let c = self.colors;
        div()
            .font_family(fonts::UI)
            .max_w(px(320.))
            .px_3()
            .py_2()
            .rounded_lg()
            .bg(c.surface)
            .border_1()
            .border_color(c.accent)
            .shadow_lg()
            .text_sm()
            .text_color(c.text)
            .child(self.title.clone())
    }
}
