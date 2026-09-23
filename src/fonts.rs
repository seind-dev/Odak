//! Bundled fonts (built by tools/make_fonts.py): Manrope for UI text, Unbounded for headings and
//! numbers, Flaticon UIcons for icons (see ui/icons.rs).

use gpui::App;
use std::borrow::Cow;

pub const UI: &str = "Manrope";
pub const DISPLAY: &str = "Unbounded";

pub fn load(cx: &mut App) {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!("../assets/fonts/Manrope-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Manrope-Medium.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Manrope-SemiBold.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Manrope-Bold.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Unbounded-SemiBold.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Unbounded-Bold.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/uicons-regular-rounded.ttf")),
    ];
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        log::error!("could not load the bundled fonts: {e}");
    }
}
