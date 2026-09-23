//! Settings: theme, launch at startup, start minimized, version and update check.

use crate::autostart;
use crate::model::Theme;
use crate::state::AppState;
use crate::theme::{self, Colors};
use crate::ui::icons::{self, icon};
use crate::ui::widgets::{button, page_title, segmented, toggle};
use crate::updater;
use gpui::{Context, Div, FontWeight, IntoElement, Render, SharedString, Subscription, Window, div, prelude::*, px};

pub struct SettingsPage {
    update_status: Option<String>,
    checking: bool,
    _observe: Subscription,
}

impl SettingsPage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let observe = cx.observe(&state, |_, _, cx| cx.notify());
        SettingsPage { update_status: None, checking: false, _observe: observe }
    }

    fn check_updates(&mut self, cx: &mut Context<Self>) {
        if self.checking {
            return;
        }
        self.checking = true;
        self.update_status = Some("Denetleniyor...".into());
        cx.notify();
        let check = cx.background_executor().spawn(async { updater::check_and_download() });
        cx.spawn(async move |this, cx| {
            let message = check.await.message();
            let _ = this.update(cx, |this, cx| {
                this.checking = false;
                this.update_status = Some(message);
                cx.notify();
            });
        })
        .detach();
    }
}

fn section(glyph: &'static str, title: &'static str, c: &Colors) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .p_4()
        .rounded_xl()
        .bg(c.surface)
        .border_1()
        .border_color(c.border)
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .child(icon(glyph).text_color(c.accent))
                .child(title),
        )
}

fn row(title: impl Into<SharedString>, description: &'static str, control: impl IntoElement, c: &Colors) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_sm().child(title.into()))
                .child(div().text_xs().text_color(c.muted).child(description)),
        )
        .child(control)
}

impl Render for SettingsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme::current(cx);
        let state = AppState::global(cx);
        let settings = state.read(cx).data.settings.clone();

        let theme_choice = segmented("theme", &[(Theme::Dark, "Koyu"), (Theme::Light, "Açık")], settings.theme, &c, {
            let state = state.clone();
            move |theme, _, cx| state.update(cx, |s, cx| s.mutate(cx, |d| d.settings.theme = theme))
        });
        let auto_launch = toggle("auto-launch", settings.auto_launch, &c).on_click({
            let state = state.clone();
            let enable = !settings.auto_launch;
            move |_, _, cx| {
                autostart::apply(enable);
                state.update(cx, |s, cx| s.mutate(cx, |d| d.settings.auto_launch = enable));
            }
        });
        let start_minimized = toggle("start-minimized", settings.start_minimized, &c).on_click({
            let state = state.clone();
            let enable = !settings.start_minimized;
            move |_, _, cx| state.update(cx, |s, cx| s.mutate(cx, |d| d.settings.start_minimized = enable))
        });
        let check = button("check-updates", if self.checking { "Denetleniyor..." } else { "Güncellemeleri denetle" }, &c)
            .on_click(cx.listener(|this, _, _, cx| this.check_updates(cx)));

        div().id("settings").size_full().overflow_y_scroll().child(
            div()
                .max_w(px(680.))
                .p_6()
                .flex()
                .flex_col()
                .gap_5()
                .child(page_title("Ayarlar", &c))
                .child(section(icons::PALETTE, "Görünüm", &c).child(row("Tema", "Uygulamanın renk teması", theme_choice, &c)))
                .child(
                    section(icons::POWER, "Başlangıç", &c)
                        .child(row(
                            "Windows başlangıcında aç",
                            "Oturum açılınca sistem tepsisinde başlar",
                            auto_launch,
                            &c,
                        ))
                        .child(row(
                            "Küçültülmüş başlat",
                            "Açılışta pencere yerine yalnızca tepsi simgesi görünür",
                            start_minimized,
                            &c,
                        )),
                )
                .child(
                    section(icons::INFO, "Hakkında", &c)
                        .child(row(
                            format!("Sürüm {}", env!("CARGO_PKG_VERSION")),
                            "Güncellemeler GitHub üzerinden gelir",
                            check,
                            &c,
                        ))
                        .when_some(self.update_status.clone(), |d, status| {
                            d.child(div().text_sm().text_color(c.muted).child(status))
                        })
                        .child(
                            div()
                                .text_xs()
                                .text_color(c.muted)
                                .child("İkonlar: Uicons by Flaticon (flaticon.com) · Yazı tipleri: Manrope, Unbounded (SIL OFL)"),
                        ),
                ),
        )
    }
}
