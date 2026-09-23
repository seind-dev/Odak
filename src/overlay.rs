//! Notification pop-ups: small always-on-top windows in the bottom-right corner that do not take
//! focus. `notify` also records each one in the notification history (`Data::notices`).

use crate::APP_NAME;
use crate::fonts;
use crate::model::{Notice, NoticeKind, Priority, Task};
use crate::state::AppState;
use crate::theme::priority_color;
use crate::ui::icons::{self, icon};
use crate::ui::shell::open_main_window;
use crate::views;
use chrono::Utc;
use gpui::{
    Animation, AnimationExt, AnyWindowHandle, App, Bounds, Context, FontWeight, Global, IntoElement, Render, Rgba,
    SharedString, Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div, ease_out_quint,
    point, prelude::*, px, rgb, size,
};
use std::time::Duration;
use uuid::Uuid;

const WIDTH: f32 = 360.;
const HEIGHT: f32 = 92.;
const MARGIN: f32 = 16.;
const GAP: f32 = 8.;
const VISIBLE_FOR: Duration = Duration::from_secs(5);

/// Pop-ups currently on screen, so new ones stack above them.
#[derive(Default)]
struct OpenPopups(Vec<AnyWindowHandle>);

impl Global for OpenPopups {}

/// Records `notice` in the notification history and shows it as a pop-up.
pub fn notify(cx: &mut App, notice: Notice) {
    let accent = match (notice.priority, notice.kind) {
        (Some(priority), _) => priority_color(priority),
        (None, NoticeKind::Update) => priority_color(Priority::Low),
        (None, _) => priority_color(Priority::High),
    };
    let popup = Popup {
        title: notice.title.clone().into(),
        body: notice.body.clone().into(),
        accent,
        notice_id: notice.id,
        task_id: notice.task_id,
    };
    AppState::global(cx).update(cx, |s, cx| s.mutate(cx, |d| d.record_notice(notice)));
    show(cx, popup);
}

/// Pop-up for a reminder that fired.
pub fn reminder(cx: &mut App, task: &Task) {
    notify(
        cx,
        Notice {
            priority: Some(task.priority),
            task_id: Some(task.id),
            ..Notice::new(NoticeKind::Reminder, task.title.clone(), format!("{} öncelik", task.priority.label()), Utc::now())
        },
    );
}

/// At startup: one pop-up listing the open high-priority tasks, if there are any.
pub fn startup_alert(cx: &mut App) {
    let state = AppState::global(cx);
    let titles: Vec<String> =
        views::open_high_priority(&state.read(cx).data.tasks).iter().map(|t| t.title.clone()).collect();
    if !titles.is_empty() {
        let title = format!("{} yüksek öncelikli görev", titles.len());
        notify(
            cx,
            Notice { priority: Some(Priority::High), ..Notice::new(NoticeKind::Alert, title, titles.join(", "), Utc::now()) },
        );
    }
}

fn show(cx: &mut App, popup: Popup) {
    let Some(display) = cx.primary_display() else { return };
    let alive = cx.windows();
    let popups = cx.default_global::<OpenPopups>();
    popups.0.retain(|w| alive.contains(w));
    let slot = popups.0.len() as f32;
    let area = display.visible_bounds();
    let origin = point(
        area.origin.x + area.size.width - px(WIDTH + MARGIN),
        area.origin.y + area.size.height - px(MARGIN + HEIGHT + slot * (HEIGHT + GAP)),
    );
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(origin, size(px(WIDTH), px(HEIGHT))))),
        titlebar: None,
        focus: false,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        let handle = window.window_handle();
        cx.spawn(async move |cx| {
            cx.background_executor().timer(VISIBLE_FOR).await;
            let _ = cx.update_window(handle, |_, window, _| window.remove_window());
        })
        .detach();
        cx.new(|_| popup)
    });
    match opened {
        Ok(window) => cx.default_global::<OpenPopups>().0.push(window.into()),
        Err(e) => log::error!("could not open a notification: {e}"),
    }
}

struct Popup {
    title: SharedString,
    body: SharedString,
    accent: Rgba,
    notice_id: Uuid,
    task_id: Option<Uuid>,
}

impl Render for Popup {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (notice_id, task_id) = (self.notice_id, self.task_id);
        div()
            .id("popup")
            .size_full()
            .p_1()
            .font_family(fonts::UI)
            .cursor_pointer()
            // Clicking opens the app on the notice's task and marks it read.
            .on_click(move |_, window, cx| {
                window.remove_window();
                cx.defer(move |cx| {
                    open_main_window(cx);
                    AppState::global(cx).update(cx, |s, cx| {
                        s.mutate(cx, |d| d.mark_notice_read(notice_id));
                        if let Some(task) = task_id.filter(|t| s.data.task(*t).is_some()) {
                            s.edit(task, cx);
                        }
                    });
                });
            })
            .child(
                div()
                    .size_full()
                    .flex()
                    .rounded_xl()
                    .overflow_hidden()
                    .bg(rgb(0x171717))
                    .border_1()
                    .border_color(rgb(0x2e2e2e))
                    .child(div().w(px(4.)).h_full().flex_none().bg(self.accent))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px_3()
                            .py_2()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .text_xs()
                                    .text_color(rgb(0x737373))
                                    .child(icon(icons::BELL).text_color(self.accent))
                                    .child(div().font_family(fonts::DISPLAY).child(APP_NAME.to_uppercase())),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(0xfafafa))
                                    .truncate()
                                    .child(self.title.clone()),
                            )
                            .child(div().text_xs().text_color(rgb(0xa3a3a3)).truncate().child(self.body.clone())),
                    )
                    .with_animation(
                        "fade-in",
                        Animation::new(Duration::from_millis(300)).with_easing(ease_out_quint()),
                        |el, t| el.opacity(t),
                    ),
            )
    }
}
