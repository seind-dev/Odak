//! Flaticon UIcons (regular rounded, "Uicons by Flaticon"), drawn as glyphs of the bundled icon
//! font. `tools/make_fonts.py` keeps exactly the code points listed here, so add new icons here
//! first (names and code points come from the package's css/regular/rounded.css) and re-run it.

use crate::model::NoticeKind;
use gpui::{Div, div, prelude::*};

pub const FONT: &str = "uicons-regular-rounded";

pub const ADD: &str = "\u{f11a}"; // add
pub const APPS: &str = "\u{f16c}"; // apps
pub const BELL: &str = "\u{f239}"; // bell
pub const CALENDAR: &str = "\u{f339}"; // calendar
pub const CHECK: &str = "\u{f3c8}"; // check
pub const CHEVRON_LEFT: &str = "\u{f154}"; // angle-small-left
pub const CHEVRON_RIGHT: &str = "\u{f155}"; // angle-small-right
pub const CLOCK: &str = "\u{f45a}"; // clock
pub const CLOSE: &str = "\u{f4f6}"; // cross-small
pub const CLOUD: &str = "\u{f479}"; // cloud
pub const CLOUD_CHECK: &str = "\u{f460}"; // cloud-check
pub const CLOUD_DISABLED: &str = "\u{f462}"; // cloud-disabled
pub const INFO: &str = "\u{f80b}"; // info
pub const KANBAN: &str = "\u{fd3a}"; // table-columns
pub const LIST: &str = "\u{f8c0}"; // list-check
pub const MAXIMIZE: &str = "\u{fcd4}"; // square
pub const MINIMIZE: &str = "\u{f94d}"; // minus-small
pub const PALETTE: &str = "\u{f9ed}"; // palette
pub const POWER: &str = "\u{fac9}"; // power
pub const REFRESH: &str = "\u{fb34}"; // refresh
pub const REPEAT: &str = "\u{f1c4}"; // arrows-repeat
pub const SEARCH: &str = "\u{fbba}"; // search
pub const SETTINGS: &str = "\u{fbd2}"; // settings
pub const SIGN_OUT: &str = "\u{fc0c}"; // sign-out-alt
pub const SNOOZE: &str = "\u{f133}"; // alarm-snooze
pub const TIME_PAST: &str = "\u{fdaa}"; // time-past
pub const TRASH: &str = "\u{fe17}"; // trash
pub const UNDO: &str = "\u{fe6b}"; // undo
pub const USER: &str = "\u{fea0}"; // user
pub const USER_ADD: &str = "\u{fe7a}"; // user-add
pub const USERS: &str = "\u{fea6}"; // users

/// The icon of a notification kind (pop-ups and the Bildirimler page).
pub fn for_notice(kind: NoticeKind) -> &'static str {
    match kind {
        NoticeKind::Reminder => BELL,
        NoticeKind::Alert => INFO,
        NoticeKind::Update => REFRESH,
        NoticeKind::Shared => USERS,
    }
}

/// One icon glyph; size and color follow the surrounding text (chain `.text_*()` to change them).
pub fn icon(glyph: &'static str) -> Div {
    div().flex_none().flex().items_center().justify_center().font_family(FONT).child(glyph)
}
