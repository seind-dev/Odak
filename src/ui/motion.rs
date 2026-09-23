//! Motion: short (about 150–200 ms), ease-out, never bouncing or looping. Keyboard-initiated
//! changes are left instant by the callers. With Windows' "Animation effects" turned off, GPUI
//! jumps every animation to its end state (see `follow_system_setting`).

use gpui::{
    Animation, AnimationElement, AnimationExt, App, ElementId, IntoElement, SpringAnimation, SpringConfig,
    SpringPlayback, SpringTarget, Styled, ease_out_quint, px,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::{Duration, Instant};
use windows::core::BOOL;
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};

/// How long things take to appear.
pub const ENTER: Duration = Duration::from_millis(180);
/// Items appearing together follow each other by this much (up to `MAX_STAGGER` items).
const STAGGER: Duration = Duration::from_millis(28);
const MAX_STAGGER: u32 = 8;
/// Critically damped (no overshoot); settles in about 200 ms and retargets smoothly when interrupted.
const SNAP: SpringConfig = SpringConfig::new(500.0, 45.0, 1.0);

/// Follows Windows' "Animation effects" setting (Settings → Accessibility → Visual effects).
/// Cheap; called at startup and periodically so a change applies without a restart.
pub fn follow_system_setting(cx: &mut App) {
    let mut on = BOOL(1);
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut on as *mut BOOL as *mut std::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    cx.set_reduce_motion(read.is_ok() && !on.as_bool());
}

/// An element id derived from a name and any hashable value.
pub fn key(name: &'static str, value: impl Hash) -> ElementId {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    ElementId::NamedInteger(name.into(), hasher.finish())
}

/// A spring towards `target` for state changes (selection, toggles). `instant` snaps instead,
/// for changes made from the keyboard.
pub fn spring<T: SpringTarget>(target: T, instant: bool) -> SpringAnimation<T> {
    let animation = SpringAnimation::new(SNAP).to(target);
    if instant { animation.playback(SpringPlayback::Completed) } else { animation }
}

/// Fades in while sliding `(dx, dy)` px into place, every time the element appears (a new `id`
/// or one that was not rendered in the previous frame).
pub fn appear<E: IntoElement + Styled + 'static>(id: impl Into<ElementId>, element: E, dx: f32, dy: f32) -> AnimationElement<E> {
    appear_for(id, element, dx, dy, ENTER)
}

pub fn appear_for<E: IntoElement + Styled + 'static>(
    id: impl Into<ElementId>,
    element: E,
    dx: f32,
    dy: f32,
    duration: Duration,
) -> AnimationElement<E> {
    element.with_animation(id, Animation::new(duration).with_easing(ease_out_quint()), move |el, t| {
        let rest = 1.0 - t;
        el.relative().left(px(dx * rest)).top(px(dy * rest)).opacity(t)
    })
}

/// A list item that fades up the first time it is shown this session (so revisiting a page does
/// not replay it), with items shown together following each other.
pub fn enter_once<E: IntoElement + Styled + 'static>(name: &'static str, value: impl Hash, element: E) -> AnimationElement<E> {
    let id = key(name, value);
    let ElementId::NamedInteger(_, hash) = id else { unreachable!() };
    let delay = entering(hash);
    let total = ENTER + delay.unwrap_or_default();
    let lead = delay.unwrap_or_default().as_secs_f32() / total.as_secs_f32();
    let ease = ease_out_quint();
    // The wrapper stays even when there is nothing to animate, so the item keeps its element state.
    element.with_animation(id, Animation::new(total).with_easing(move |t| ease(((t - lead) / (1.0 - lead)).clamp(0.0, 1.0))), move |el, t| {
        if delay.is_none() {
            return el;
        }
        el.relative().top(px(6.0 * (1.0 - t))).opacity(t)
    })
}

#[derive(Default)]
struct Seen {
    /// When each key was first shown, and its place in the batch shown with it.
    first: HashMap<u64, (Instant, u32)>,
    batch_start: Option<Instant>,
    batch_len: u32,
}

thread_local! {
    static SEEN: RefCell<Seen> = RefCell::default();
}

/// `Some(delay)` while the item with `key` is still entering, `None` once it has been shown.
fn entering(key: u64) -> Option<Duration> {
    SEEN.with_borrow_mut(|seen| {
        let now = Instant::now();
        if !seen.first.contains_key(&key) {
            // Keys first seen within one frame form a batch.
            if seen.batch_start.is_none_or(|start| now - start > Duration::from_millis(50)) {
                seen.batch_start = Some(now);
                seen.batch_len = 0;
            }
            let slot = seen.batch_len.min(MAX_STAGGER);
            seen.batch_len += 1;
            seen.first.insert(key, (now, slot));
        }
        let (start, slot) = seen.first[&key];
        let delay = STAGGER * slot;
        (now - start < ENTER + delay).then_some(delay)
    })
}
