//! Tray icon with "Göster" and "Çıkış". Tray, menu and second-launch events arrive on one channel
//! that is drained on the GPUI main thread.

use crate::APP_NAME;
use crate::ui::shell::open_main_window;
use crate::updater;
use futures::StreamExt;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui::{App, Global};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

pub enum Command {
    Show,
    Quit,
}

/// Keeps the tray icon alive for the whole run.
struct Tray(#[allow(dead_code)] TrayIcon);

impl Global for Tray {}

/// Creates the tray icon and returns a sender that other sources can use too.
pub fn install(cx: &mut App) -> UnboundedSender<Command> {
    let (tx, mut rx) = unbounded();
    match build(tx.clone()) {
        Ok(icon) => cx.set_global(Tray(icon)),
        Err(e) => log::error!("tray icon unavailable: {e}"),
    }
    cx.spawn(async move |cx| {
        while let Some(command) = rx.next().await {
            cx.update(|cx| match command {
                Command::Show => open_main_window(cx),
                Command::Quit => {
                    updater::apply_pending_on_exit();
                    cx.quit();
                }
            });
        }
    })
    .detach();
    tx
}

fn build(tx: UnboundedSender<Command>) -> Result<TrayIcon, Box<dyn std::error::Error>> {
    let show = MenuItem::new("Göster", true, None);
    let quit = MenuItem::new("Çıkış", true, None);
    let menu = Menu::new();
    menu.append(&show)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&quit)?;
    let (show_id, quit_id) = (show.id().clone(), quit.id().clone());
    let menu_tx = tx.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let command = if event.id == show_id {
            Command::Show
        } else if event.id == quit_id {
            Command::Quit
        } else {
            return;
        };
        let _ = menu_tx.unbounded_send(command);
    }));
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        let show = matches!(
            event,
            TrayIconEvent::DoubleClick { .. }
                | TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
        );
        if show {
            let _ = tx.unbounded_send(Command::Show);
        }
    }));
    let icon = Icon::from_rgba(include_bytes!("../assets/tray-32.rgba").to_vec(), 32, 32)?;
    Ok(TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip(APP_NAME).with_icon(icon).build()?)
}
