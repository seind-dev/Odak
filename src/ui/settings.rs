//! Settings: account, theme, launch at startup, start minimized, version and update check.

use crate::account;
use crate::autostart;
use crate::model::{Profile, Theme};
use crate::state::{AppState, Auth};
use crate::supabase;
use crate::sync;
use crate::theme::{self, Colors};
use crate::ui::icons::{self, icon};
use crate::ui::widgets::{avatar, button, page_title, primary_button, segmented, sync_label, toggle};
use chrono::Utc;
use crate::updater;
use gpui::{App, Context, Div, FontWeight, IntoElement, Render, SharedString, Subscription, Window, div, prelude::*, px};

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

fn row(title: impl IntoElement, description: impl Into<SharedString>, control: impl IntoElement, c: &Colors) -> Div {
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
                .child(div().text_sm().child(title))
                .child(div().text_xs().text_color(c.muted).child(description.into())),
        )
        .child(control)
}

/// The Hesap section: sign-in button, the waiting state, or the account with sign-out.
fn account_section(cx: &App, c: &Colors) -> Div {
    let section = section(icons::USER, "Hesap", c);
    if !supabase::enabled() {
        return section.child(div().text_sm().text_color(c.muted).child("Bu sürümde hesap özellikleri kapalı."));
    }
    let state = AppState::global(cx);
    let state = state.read(cx);
    let body = match (&state.auth, &state.data.account) {
        (Auth::Waiting(_), _) => row(
            "Tarayıcıda giriş bekleniyor...",
            "Discord'da izin verince Odak'a dönülür",
            button("cancel-sign-in", "İptal", c).on_click(|_, _, cx| account::cancel_sign_in(cx)),
            c,
        ),
        (Auth::SignedIn(_), Some(profile)) => profile_row(
            profile,
            format!("@{} · Discord", profile.username),
            button("sign-out", "Çıkış yap", c)
                .flex()
                .items_center()
                .gap_2()
                .child(icon(icons::SIGN_OUT))
                .on_click(|_, _, cx| account::request_sign_out(cx)),
            c,
        ),
        (_, Some(profile)) => profile_row(
            profile,
            "Oturumun sona erdi".into(),
            primary_button("sign-in", "Tekrar giriş yap", c).on_click(|_, _, cx| account::sign_in(cx)),
            c,
        ),
        _ => row(
            "Discord ile giriş yap",
            "Görevlerin hesabına kaydedilir, cihazlar arasında senkronize olur",
            primary_button("sign-in", "Giriş yap", c).on_click(|_, _, cx| account::sign_in(cx)),
            c,
        ),
    };
    let signed_in = matches!(state.auth, Auth::SignedIn(_));
    let waiting = state.data.pending.len();
    section
        .child(body)
        .when_some(state.auth_error.clone(), |d, error| d.child(div().text_sm().text_color(c.danger).child(error)))
        .when(signed_in && state.data.needs_account_choice(), |d| {
            d.child(notice(
                format!(
                    "Bu cihazda başka bir hesaba ait {} görev var. Bu hesaba kopyalansın mı, yoksa bu cihazdan kaldırılsın mı? \
                     Kaldırılırlarsa diğer hesapta kalırlar.",
                    state.data.tasks.len()
                ),
                primary_button("copy-tasks", "Kopyala", c).on_click(|_, _, cx| account::adopt_local_tasks(true, cx)),
                button("drop-tasks", "Bu cihazdan kaldır", c).on_click(|_, _, cx| account::adopt_local_tasks(false, cx)),
                c,
            ))
        })
        .when(signed_in && state.confirm_sign_out, |d| {
            let message = if waiting > 0 {
                format!("{waiting} değişiklik henüz gönderilmedi; aynı hesapla tekrar girişte gönderilecek. Yine de çıkılsın mı?")
            } else {
                "Tüm değişiklikler gönderildi. Çıkılsın mı?".to_string()
            };
            d.child(notice(
                message,
                primary_button("confirm-sign-out", "Çıkış yap", c).on_click(|_, _, cx| account::sign_out(cx)),
                button("keep-signed-in", "Vazgeç", c).on_click(|_, _, cx| account::keep_signed_in(cx)),
                c,
            ))
        })
        .when_some(sync_label(state, Utc::now()).filter(|_| signed_in), |d, (glyph, label)| {
            d.child(row(
                div().flex().items_center().gap_2().child(icon(glyph).text_color(c.muted)).child(label),
                "Görevler her değişiklikten sonra ve beş dakikada bir senkronize olur",
                button("sync-now", "Şimdi senkronize et", c).on_click(|_, _, cx| sync::request(cx, sync::NOW)),
                c,
            ))
        })
}

/// A question inside a section, with its two answers.
fn notice(message: String, yes: impl IntoElement, no: impl IntoElement, c: &Colors) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(c.border)
        .bg(c.bg)
        .child(div().text_sm().child(message))
        .child(div().flex().gap_2().child(yes).child(no))
}

fn profile_row(profile: &Profile, subtitle: String, control: impl IntoElement, c: &Colors) -> Div {
    let name = if profile.name().is_empty() { "Discord hesabı".to_string() } else { profile.name().to_string() };
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(avatar(profile, px(40.), c))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(name))
                        .child(div().text_xs().text_color(c.muted).child(subtitle)),
                ),
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
                .child(account_section(cx, &c))
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
