//! Notification pop-ups: small always-on-top windows in the bottom-right corner that do not take
//! focus. `notify` also records each one in the notification history (`Data::notices`).

use crate::APP_NAME;
use crate::fonts;
use raw_window_handle::RawWindowHandle;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DwmSetWindowAttribute,
};
use crate::model::{Notice, NoticeKind, Priority, Task};
use crate::state::AppState;
use crate::theme::priority_color;
use crate::ui::icons::{self, icon};
use crate::ui::motion;
use crate::ui::shell::open_main_window;
use crate::views;
use chrono::Utc;
use gpui::{
    Animation, AnimationExt, AnyWindowHandle, App, Bounds, Context, FontWeight, Global, Hsla, IntoElement, Render, Rgba,
    SharedString, Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div, quadratic,
    point, prelude::*, px, rgb, size,
};
use std::time::Duration;
use uuid::Uuid;

const WIDTH: f32 = 360.;
const HEIGHT: f32 = 92.;
const MARGIN: f32 = 16.;
const GAP: f32 = 8.;
const VISIBLE_FOR: Duration = Duration::from_secs(5);
const FADE_OUT: Duration = Duration::from_millis(200);

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
        glyph: icons::for_notice(notice.kind),
        title: notice.title.clone().into(),
        body: notice.body.clone().into(),
        accent,
        notice_id: notice.id,
        task_id: notice.task_id,
        closing: false,
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
        square_corners(window);
        let handle = window.window_handle();
        cx.spawn(async move |cx| {
            cx.background_executor().timer(VISIBLE_FOR - FADE_OUT).await;
            if let Some(popup) = handle.downcast::<Popup>() {
                let _ = popup.update(cx, |popup, _, cx| {
                    popup.closing = true;
                    cx.notify();
                });
            }
            cx.background_executor().timer(FADE_OUT).await;
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

/// Windows 11 rounds top-level windows and draws its own 1 px frame around them; the pop-up is
/// a plain square card, so both are turned off.
fn square_corners(window: &Window) {
    let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(window) else { return };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else { return };
    let hwnd = HWND(win32.hwnd.get() as *mut std::ffi::c_void);
    let size = std::mem::size_of::<u32>() as u32;
    unsafe {
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &DWMWCP_DONOTROUND as *const _ as *const _, size);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, &DWMWA_COLOR_NONE as *const _ as *const _, size);
    }
}

struct Popup {
    glyph: &'static str,
    title: SharedString,
    body: SharedString,
    accent: Rgba,
    notice_id: Uuid,
    task_id: Option<Uuid>,
    /// Fading out before the window closes.
    closing: bool,
}

impl Render for Popup {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (notice_id, task_id) = (self.notice_id, self.task_id);
        let accent = Hsla::from(self.accent);
        let card = div()
            .size_full()
            .flex()
            .items_center()
            .gap_3()
            .pr_4()
            .bg(rgb(0x171717))
            .border_1()
            .border_color(rgb(0x2e2e2e))
            .child(div().w(px(3.)).h_full().flex_none().bg(accent))
            .child(icon(self.glyph).size(px(34.)).bg(accent.opacity(0.14)).text_color(accent).text_base())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(div().text_xs().font_family(fonts::DISPLAY).text_color(rgb(0x737373)).child(APP_NAME))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(0xfafafa))
                            .truncate()
                            .child(self.title.clone()),
                    )
                    .child(div().text_xs().text_color(rgb(0xa3a3a3)).truncate().child(self.body.clone())),
            );
        // Slides in from the screen edge; leaves more quietly than it came.
        let card = if self.closing {
            card.with_animation("fade-out", Animation::new(FADE_OUT).with_easing(quadratic), |el, t| {
                el.relative().left(px(8. * t)).opacity(1. - t)
            })
            .into_any_element()
        } else {
            motion::appear_for("slide-in", card, 24., 0., Duration::from_millis(320)).into_any_element()
        };
        div()
            .id("popup")
            .size_full()
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
            .child(card)
    }
}
