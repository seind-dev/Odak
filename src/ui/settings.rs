//! Settings: account, theme, launch at startup, start minimized, version and update check.

use crate::account;
use crate::autostart;
use crate::model::{Profile, Theme};
use crate::state::{AppState, Auth};
use crate::supabase;
use crate::sync;
use crate::theme::{self, Colors};
use crate::ui::icons::{self, icon};
use crate::ui::motion;
use crate::ui::widgets::{avatar, button, page_title, primary_button, segmented, sync_label, toggle};
use chrono::Utc;
use crate::updater::{self, Phase, Trigger};
use crate::views;
use gpui::{
    AnyElement, App, Context, Div, FontWeight, IntoElement, Render, SharedString, Subscription, Window, div, prelude::*,
    px, relative,
};

pub struct SettingsPage {
    _observe: [Subscription; 2],
}

impl SettingsPage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let updater = updater::entity(cx);
        SettingsPage { _observe: [cx.observe(&state, |_, _, cx| cx.notify()), cx.observe(&updater, |_, _, cx| cx.notify())] }
    }
}

/// The version row: what the updater is doing and the one action that makes sense now.
fn update_row(phase: &Phase, c: &Colors) -> Div {
    let check = || button("check-updates", "Güncellemeleri denetle", c).on_click(|_, _, cx| updater::check(Trigger::Manual, cx));
    let busy = |label: &'static str| button("check-updates", label, c).opacity(0.6);
    let (description, control): (String, AnyElement) = match phase {
        Phase::Idle => ("Güncellemeler GitHub üzerinden kendiliğinden gelir".into(), check().into_any_element()),
        Phase::NotInstalled => ("Güncelleme yalnızca kurulu sürümde çalışır".into(), check().into_any_element()),
        Phase::Checking => ("Yeni sürüm aranıyor...".into(), busy("Denetleniyor...").into_any_element()),
        Phase::UpToDate(at) => (
            format!("Güncel · son denetim {}", views::time_ago(*at, chrono::Utc::now()).to_lowercase()),
            check().into_any_element(),
        ),
        Phase::Downloading { version, percent } => (format!("v{version} indiriliyor · %{percent}"), busy("İndiriliyor...").into_any_element()),
        Phase::Ready(version) => (
            format!("v{version} indirildi; yeniden başlatınca kurulur"),
            primary_button("install-update", "Yeniden başlat ve güncelle", c).on_click(|_, _, cx| updater::install(cx)).into_any_element(),
        ),
        Phase::Installing(version) => (format!("v{version} kuruluyor..."), busy("Kuruluyor...").into_any_element()),
        Phase::Failed(e) => (e.clone(), check().into_any_element()),
    };
    let percent = match phase {
        Phase::Downloading { percent, .. } => Some(*percent),
        _ => None,
    };
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(row(format!("Sürüm {}", env!("CARGO_PKG_VERSION")), description, control, c))
        .when_some(percent, |d, percent| {
            d.child(
                div()
                    .h(px(3.))
                    .w_full()
                    .rounded_full()
                    .bg(c.border)
                    .child(div().h_full().rounded_full().bg(c.accent).w(relative(f32::from(percent) / 100.))),
            )
        })
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
        .when_some(state.auth_error.clone(), |d, error| {
            d.child(motion::appear(motion::key("auth-error", &error), div().text_sm().text_color(c.danger).child(error), 0., -4.))
        })
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
                motion::appear(
                    motion::key("sync-label", &label),
                    div().flex().items_center().gap_2().child(icon(glyph).text_color(c.muted)).child(label),
                    0.,
                    0.,
                ),
                "Görevler her değişiklikten sonra ve beş dakikada bir senkronize olur",
                button("sync-now", "Şimdi senkronize et", c).on_click(|_, _, cx| sync::request(cx, sync::NOW)),
                c,
            ))
        })
}

/// A question inside a section, with its two answers; it drops in when it appears.
fn notice(message: String, yes: impl IntoElement, no: impl IntoElement, c: &Colors) -> impl IntoElement {
    let id = motion::key("notice", &message);
    let question = div()
        .flex()
        .flex_col()
        .gap_3()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(c.border)
        .bg(c.bg)
        .child(div().text_sm().child(message))
        .child(div().flex().gap_2().child(yes).child(no));
    motion::appear(id, question, 0., -4.)
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
        let update_phase = updater::entity(cx).read(cx).phase.clone();

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
                        .child(update_row(&update_phase, &c))
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
