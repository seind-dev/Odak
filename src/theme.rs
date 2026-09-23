//! Colors for the dark (neutral black and grays) and light themes, from Tailwind's palettes.

use crate::model::{Priority, Status, Theme};
use crate::state::AppState;
use gpui::{App, Rgba, rgb, rgba};

#[derive(Clone, Copy)]
pub struct Colors {
    pub bg: Rgba,
    pub surface: Rgba,
    pub sidebar: Rgba,
    pub border: Rgba,
    pub hover: Rgba,
    pub text: Rgba,
    pub muted: Rgba,
    pub accent: Rgba,
    pub danger: Rgba,
    pub selection: Rgba,
}

pub fn colors(theme: Theme) -> Colors {
    match theme {
        Theme::Dark => Colors {
            // Neutral black and dark grays (no blue tint).
            bg: rgb(0x0a0a0a),
            surface: rgb(0x171717),
            sidebar: rgb(0x111111),
            border: rgb(0x262626),
            hover: rgb(0x262626),
            text: rgb(0xfafafa),
            muted: rgb(0xa3a3a3),
            accent: rgb(0x3b82f6),
            danger: rgb(0xef4444),
            selection: rgba(0x3b82f666),
        },
        Theme::Light => Colors {
            bg: rgb(0xf9fafb),
            surface: rgb(0xffffff),
            sidebar: rgb(0xf3f4f6),
            border: rgb(0xe5e7eb),
            hover: rgb(0xe5e7eb),
            text: rgb(0x111827),
            muted: rgb(0x6b7280),
            accent: rgb(0x2563eb),
            danger: rgb(0xdc2626),
            selection: rgba(0x2563eb40),
        },
    }
}

/// Colors for the theme currently selected in settings.
pub fn current(cx: &App) -> Colors {
    colors(AppState::global(cx).read(cx).data.settings.theme)
}

pub fn priority_color(p: Priority) -> Rgba {
    match p {
        Priority::High => rgb(0xef4444),
        Priority::Medium => rgb(0xf59e0b),
        Priority::Low => rgb(0x10b981),
    }
}

pub fn status_color(s: Status) -> Rgba {
    match s {
        Status::Pending => rgb(0x9ca3af),
        Status::InProgress => rgb(0x3b82f6),
        Status::Completed => rgb(0x10b981),
    }
}
