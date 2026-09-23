//! Main window: custom titlebar, sidebar, error banner, current page and keyboard shortcuts.

use crate::APP_NAME;
use crate::fonts;
use crate::state::{AppState, Page};
use crate::sync;
use crate::theme::{self, Colors};
use crate::ui::icons::{self, icon};
use crate::ui::palette::{PaletteEvent, SearchPalette};
use crate::ui::widgets::sync_label;
use crate::ui::{
    calendar::CalendarPage, dashboard, form::FormPage, groups::GroupsPage, kanban, list::ListPage, notifications,
    settings::SettingsPage,
};
use crate::views;
use chrono::Utc;
use gpui::{
    Animation, AnimationExt, AnyElement, App, Bounds, Context, Entity, FocusHandle, FontWeight, Hsla, IntoElement,
    KeyBinding, MouseButton, Render, Rgba, Subscription, TitlebarOptions, Window, WindowBounds, WindowControlArea,
    WindowOptions, actions, div, prelude::*, px, rgb, size, white,
};
use std::time::Duration;

actions!(seindtask, [NewTask, ShowDashboard, ShowKanban, ShowCalendar, ShowSettings, OpenSearch]);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-n", NewTask, None),
        KeyBinding::new("ctrl-d", ShowDashboard, None),
        KeyBinding::new("ctrl-b", ShowKanban, None),
        KeyBinding::new("ctrl-l", ShowCalendar, None),
        KeyBinding::new("ctrl-,", ShowSettings, None),
        KeyBinding::new("ctrl-k", OpenSearch, None),
    ]);
}

/// Brings the main window forward, opening a new one if it was closed to the tray.
pub fn open_main_window(cx: &mut App) {
    if let Some(window) = cx.windows().into_iter().find_map(|w| w.downcast::<Shell>()) {
        let _ = window.update(cx, |_, window, _| window.activate_window());
        return;
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1040.), px(720.)), cx))),
        titlebar: Some(TitlebarOptions {
            title: Some(APP_NAME.into()),
            appears_transparent: true,
            traffic_light_position: None,
        }),
        window_min_size: Some(size(px(760.), px(520.))),
        ..Default::default()
    };
    match cx.open_window(options, |window, cx| cx.new(|cx| Shell::new(window, cx))) {
        Ok(_) => cx.activate(true),
        Err(e) => log::error!("could not open the main window: {e}"),
    }
}

pub struct Shell {
    state: Entity<AppState>,
    focus: FocusHandle,
    list: Entity<ListPage>,
    calendar: Entity<CalendarPage>,
    groups: Entity<GroupsPage>,
    settings: Entity<SettingsPage>,
    form: Option<Entity<FormPage>>,
    palette: Option<Entity<SearchPalette>>,
    palette_subscription: Option<Subscription>,
    _observe: Subscription,
}

impl Shell {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let observe = cx.observe_in(&state, window, |this, _, window, cx| {
            this.sync_form(window, cx);
            cx.notify();
        });
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut shell = Shell {
            list: cx.new(ListPage::new),
            calendar: cx.new(CalendarPage::new),
            groups: cx.new(GroupsPage::new),
            settings: cx.new(SettingsPage::new),
            form: None,
            palette: None,
            palette_subscription: None,
            state,
            focus,
            _observe: observe,
        };
        shell.sync_form(window, cx);
        shell
    }

    /// A fresh form is built when the form page opens or switches task, and dropped when leaving it.
    fn sync_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (page, editing) = {
            let s = self.state.read(cx);
            (s.page, s.editing)
        };
        if page != Page::Form {
            if self.form.take().is_some() {
                window.focus(&self.focus, cx);
            }
            return;
        }
        if self.form.as_ref().is_some_and(|f| f.read(cx).editing == editing) {
            return;
        }
        self.form = Some(cx.new(|cx| FormPage::new(editing, window, cx)));
    }

    fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_some() {
            return;
        }
        let palette = cx.new(|cx| SearchPalette::new(window, cx));
        self.palette_subscription =
            Some(cx.subscribe_in(&palette, window, |this, _, _: &PaletteEvent, window, cx| this.close_palette(window, cx)));
        self.palette = Some(palette);
        cx.notify();
    }

    /// Closes the palette and puts focus back where typing makes sense.
    fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette = None;
        self.palette_subscription = None;
        let focus = match &self.form {
            Some(form) => form.read(cx).title_focus(cx),
            None => self.focus.clone(),
        };
        window.focus(&focus, cx);
        cx.notify();
    }

    fn go(&mut self, page: Page, cx: &mut Context<Self>) {
        self.state.update(cx, |s, cx| s.navigate(page, cx));
    }

    fn sidebar(&self, c: &Colors, cx: &Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        let stats = views::stats(&state.data.tasks, Utc::now());
        let (current, editing) = (state.page, state.editing);
        let unread = state.data.unread_notices();
        let sync_status = sync_label(state, Utc::now());
        let nav = [
            (Page::Dashboard, icons::APPS, "Dashboard", "Ctrl+D"),
            (Page::List, icons::LIST, "Görevler", ""),
            (Page::Kanban, icons::KANBAN, "Kanban", "Ctrl+B"),
            (Page::Calendar, icons::CALENDAR, "Takvim", "Ctrl+L"),
            (Page::Groups, icons::USERS, "Gruplar", ""),
            (Page::Notifications, icons::BELL, "Bildirimler", ""),
            (Page::Form, icons::ADD, "Yeni Görev", "Ctrl+N"),
            (Page::Settings, icons::SETTINGS, "Ayarlar", "Ctrl+,"),
        ];
        let (text, muted, hover) = (c.text, c.muted, c.hover);
        div()
            .w(px(224.))
            .flex_none()
            .flex()
            .flex_col()
            .bg(c.sidebar)
            .border_r_1()
            .border_color(c.border)
            .child(
                div()
                    .p_4()
                    .flex()
                    .gap_2()
                    .child(stat_box(stats.pending, "BEKLEYEN", c.text, c))
                    .child(stat_box(stats.in_progress, "DEVAM", c.accent, c)),
            )
            .child(
                div().px_3().pb_2().child(
                    div()
                        .id("search")
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .py_1p5()
                        .rounded_lg()
                        .border_1()
                        .border_color(c.border)
                        .bg(c.bg)
                        .text_sm()
                        .text_color(c.muted)
                        .cursor_pointer()
                        .hover(move |s| s.border_color(muted))
                        .child(icon(icons::SEARCH))
                        .child(div().flex_1().child("Ara…"))
                        .child(div().text_xs().child("Ctrl+K"))
                        .on_click(cx.listener(|this, _, window, cx| this.open_palette(window, cx))),
                ),
            )
            .child(div().flex_1().px_3().flex().flex_col().gap_0p5().children(nav.into_iter().map(
                |(page, glyph, label, keys)| {
                    // "Yeni Görev" is only active for a new task, not while editing one.
                    let active = current == page && !(page == Page::Form && editing.is_some());
                    div()
                        .id(label)
                        .px_3()
                        .py_2()
                        .rounded_lg()
                        .flex()
                        .items_center()
                        .justify_between()
                        .text_sm()
                        .cursor_pointer()
                        .when(active, |d| d.bg(hover).text_color(text).font_weight(FontWeight::MEDIUM))
                        .when(!active, |d| d.text_color(muted))
                        // Always attached so GPUI keeps tracking hover (see widgets::segmented).
                        .hover(move |s| s.bg(hover).text_color(text))
                        .child(div().flex().items_center().gap_3().child(icon(glyph)).child(label))
                        .map(|d| {
                            if page == Page::Notifications && unread > 0 {
                                d.child(
                                    div()
                                        .px_1p5()
                                        .rounded_full()
                                        .bg(gpui::rgb(0xef4444))
                                        .text_xs()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(white())
                                        .child(if unread > 9 { "9+".to_string() } else { unread.to_string() }),
                                )
                            } else {
                                d.child(div().text_xs().text_color(muted).child(keys))
                            }
                        })
                        .on_click(cx.listener(move |this, _, _, cx| this.go(page, cx)))
                },
            )))
            .child(
                div()
                    .p_4()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .text_xs()
                    .text_color(c.muted)
                    .when_some(sync_status, |d, (glyph, label)| {
                        d.child(
                            div()
                                .id("sync-status")
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .cursor_pointer()
                                .hover(move |s| s.text_color(text))
                                .child(icon(glyph))
                                .child(div().truncate().child(label))
                                .on_click(|_, _, cx| sync::request(cx, sync::NOW)),
                        )
                    })
                    .child(div().flex_none().child(concat!("v", env!("CARGO_PKG_VERSION")))),
            )
    }
}

fn stat_box(count: usize, label: &'static str, color: Rgba, c: &Colors) -> impl IntoElement {
    div()
        .flex_1()
        .px_3()
        .py_2()
        .rounded_lg()
        .bg(c.hover)
        .flex()
        .flex_col()
        .child(
            div()
                .font_family(fonts::DISPLAY)
                .text_lg()
                .font_weight(FontWeight::BOLD)
                .text_color(color)
                .child(count.to_string()),
        )
        .child(div().text_xs().text_color(c.muted).child(label))
}

fn titlebar(c: &Colors) -> impl IntoElement {
    let control = |id: &'static str, glyph: &'static str, area: WindowControlArea, danger: bool| {
        let hover = c.hover;
        div()
            .id(id)
            .w(px(46.))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .text_sm()
            .text_color(c.muted)
            .hover(move |s| if danger { s.bg(rgb(0xdc2626)).text_color(white()) } else { s.bg(hover) })
            .window_control_area(area)
            .child(icon(glyph))
    };
    div()
        .flex_none()
        .h(px(32.))
        .flex()
        .items_center()
        .bg(c.sidebar)
        .border_b_1()
        .border_color(c.border)
        // The drag area must be a sibling of the buttons, not their parent: GPUI answers the
        // Windows hit test with the first matching control area in paint order, so a parent
        // Drag area would win over Min/Max/Close and the buttons would never receive clicks.
        .child(
            div()
                .id("titlebar-drag")
                .flex_1()
                .h_full()
                .pl_3()
                .flex()
                .items_center()
                .gap_2()
                .text_xs()
                .window_control_area(WindowControlArea::Drag)
                .child(icon(icons::CHECK).size(px(16.)).rounded_sm().bg(c.accent).text_color(white()))
                .child(div().font_family(fonts::DISPLAY).font_weight(FontWeight::SEMIBOLD).text_color(c.muted).child(APP_NAME)),
        )
        .child(
            div()
                .flex()
                .h_full()
                .child(control("min", icons::MINIMIZE, WindowControlArea::Min, false))
                .child(control("max", icons::MAXIMIZE, WindowControlArea::Max, false))
                .child(control("close", icons::CLOSE, WindowControlArea::Close, true)),
        )
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme::current(cx);
        let (page, banner) = {
            let s = self.state.read(cx);
            (s.page, s.banner.clone())
        };
        let content: AnyElement = match page {
            Page::Dashboard => dashboard::render(&c, cx).into_any_element(),
            Page::List => self.list.clone().into_any_element(),
            Page::Kanban => kanban::render(&c, cx).into_any_element(),
            Page::Calendar => self.calendar.clone().into_any_element(),
            Page::Groups => self.groups.clone().into_any_element(),
            Page::Settings => self.settings.clone().into_any_element(),
            Page::Notifications => notifications::render(&c, cx).into_any_element(),
            Page::Form => match &self.form {
                Some(form) => form.clone().into_any_element(),
                None => div().into_any_element(),
            },
        };
        let banner = banner.map(|text| {
            div()
                .flex_none()
                .mx_6()
                .mt_4()
                .px_4()
                .py_2()
                .rounded_lg()
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .bg(Hsla::from(c.danger).opacity(0.15))
                .text_sm()
                .text_color(c.danger)
                .child(text)
                .child(
                    div()
                        .id("banner-close")
                        .flex_none()
                        .cursor_pointer()
                        .child("Kapat")
                        .on_click(cx.listener(|this, _, _, cx| this.state.update(cx, |s, cx| s.dismiss_banner(cx)))),
                )
        });

        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .font_family(fonts::UI)
            .bg(c.bg)
            .text_color(c.text)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &NewTask, _, cx| this.go(Page::Form, cx)))
            .on_action(cx.listener(|this, _: &ShowDashboard, _, cx| this.go(Page::Dashboard, cx)))
            .on_action(cx.listener(|this, _: &ShowKanban, _, cx| this.go(Page::Kanban, cx)))
            .on_action(cx.listener(|this, _: &ShowCalendar, _, cx| this.go(Page::Calendar, cx)))
            .on_action(cx.listener(|this, _: &ShowSettings, _, cx| this.go(Page::Settings, cx)))
            .on_action(cx.listener(|this, _: &OpenSearch, window, cx| this.open_palette(window, cx)))
            .child(titlebar(&c))
            .child(
                div().flex_1().min_h_0().flex().child(self.sidebar(&c, cx)).child(
                    div().flex_1().min_w_0().flex().flex_col().children(banner).child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .child(content)
                            .with_animation(("page", page as usize), Animation::new(Duration::from_millis(150)), |el, t| {
                                el.opacity(t)
                            }),
                    ),
                ),
            )
            .when_some(self.palette.clone(), |d, palette| {
                d.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .flex()
                        .justify_center()
                        .items_start()
                        .pt(px(110.))
                        .bg(gpui::black().opacity(0.5))
                        .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| this.close_palette(window, cx)))
                        .child(palette),
                )
            })
    }
}
