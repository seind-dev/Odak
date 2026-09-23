//! Editable text field, single- or multi-line, adapted from GPUI's `examples/input.rs`.
//! Supports IME, selection, clipboard and mouse, and emits `TextEvent`.

use crate::theme;
use gpui::{
    App, AvailableSpace, Bounds, ClipboardItem, Context, CursorStyle, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId, Hsla, KeyBinding, LayoutId,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, SharedString, Style,
    TextAlign, TextRun, UTF16Selection, UnderlineStyle, Window, WrappedLine, actions, div, fill, point,
    prelude::*, px, relative, size,
};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

actions!(
    text_input,
    [Backspace, Delete, Left, Right, Up, Down, SelectLeft, SelectRight, SelectAll, Home, End, Enter, Paste, Cut, Copy, Cancel]
);

const CONTEXT: &str = "TextInput";
/// Minimum visible rows of a multi-line input.
const MIN_ROWS: usize = 4;

/// Key bindings shared by every text input; call once at startup.
pub fn bind_keys(cx: &mut App) {
    let ctx = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, ctx),
        KeyBinding::new("delete", Delete, ctx),
        KeyBinding::new("left", Left, ctx),
        KeyBinding::new("right", Right, ctx),
        KeyBinding::new("up", Up, ctx),
        KeyBinding::new("down", Down, ctx),
        KeyBinding::new("shift-left", SelectLeft, ctx),
        KeyBinding::new("shift-right", SelectRight, ctx),
        KeyBinding::new("ctrl-a", SelectAll, ctx),
        KeyBinding::new("home", Home, ctx),
        KeyBinding::new("end", End, ctx),
        KeyBinding::new("enter", Enter, ctx),
        KeyBinding::new("ctrl-v", Paste, ctx),
        KeyBinding::new("ctrl-x", Cut, ctx),
        KeyBinding::new("ctrl-c", Copy, ctx),
        KeyBinding::new("escape", Cancel, ctx),
    ]);
}

pub enum TextEvent {
    /// The text changed through typing, paste or IME.
    Changed,
    /// Enter was pressed in a single-line input.
    Submit,
    /// Up/Down arrow in a single-line input (lists such as the search palette follow these).
    Up,
    Down,
    /// Escape was pressed.
    Cancel,
}

pub struct TextInput {
    focus_handle: FocusHandle,
    content: String,
    placeholder: SharedString,
    multiline: bool,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<TextLayout>,
    is_selecting: bool,
}

/// Text shaped during the last paint, used for hit-testing, cursor movement and IME bounds.
struct TextLayout {
    /// One entry per `\n`-separated line: (byte offset where it starts, shaped line).
    lines: Vec<(usize, WrappedLine)>,
    bounds: Bounds<Pixels>,
    line_height: Pixels,
}

impl TextLayout {
    fn line_top(&self, ix: usize) -> Pixels {
        self.lines[..ix].iter().fold(px(0.), |top, (_, line)| top + line.size(self.line_height).height)
    }

    /// Top-left of the cursor at byte `offset`, relative to the text origin.
    fn position_for(&self, offset: usize) -> Option<Point<Pixels>> {
        let ix = self.lines.iter().rposition(|(start, _)| *start <= offset)?;
        let (start, line) = &self.lines[ix];
        let p = line.position_for_index(offset - start, self.line_height)?;
        Some(point(p.x, p.y + self.line_top(ix)))
    }

    /// Byte offset closest to `p` (relative to the text origin).
    fn offset_for(&self, p: Point<Pixels>, len: usize) -> usize {
        if p.y < px(0.) {
            return 0;
        }
        let mut top = px(0.);
        for (start, line) in &self.lines {
            let height = line.size(self.line_height).height;
            if p.y < top + height {
                let local = point(p.x, p.y - top);
                return start
                    + match line.closest_index_for_position(local, self.line_height) {
                        Ok(i) | Err(i) => i,
                    };
            }
            top += height;
        }
        len
    }

    fn selection_quads(&self, range: &Range<usize>, color: Hsla) -> Vec<PaintQuad> {
        let (Some(a), Some(b)) = (self.position_for(range.start), self.position_for(range.end)) else {
            return Vec::new();
        };
        let origin = self.bounds.origin;
        let width = self.bounds.size.width;
        let lh = self.line_height;
        let rect = |x0: Pixels, y: Pixels, x1: Pixels| {
            fill(Bounds::from_corners(point(origin.x + x0, origin.y + y), point(origin.x + x1, origin.y + y + lh)), color)
        };
        if (a.y - b.y).abs() < lh / 2. {
            return vec![rect(a.x, a.y, b.x)];
        }
        let mut quads = vec![rect(a.x, a.y, width)];
        let mut y = a.y + lh;
        while y < b.y - lh / 2. {
            quads.push(rect(px(0.), y, width));
            y += lh;
        }
        quads.push(rect(px(0.), b.y, b.x));
        quads
    }
}

impl TextInput {
    pub fn new(placeholder: impl Into<SharedString>, multiline: bool, cx: &mut Context<Self>) -> Self {
        TextInput {
            focus_handle: cx.focus_handle(),
            content: String::new(),
            placeholder: placeholder.into(),
            multiline,
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            is_selecting: false,
        }
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    /// Replaces the text (no `Changed` event) and puts the cursor at the end.
    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        let text = text.into();
        self.content = if self.multiline { text } else { text.replace('\n', " ") };
        self.selected_range = self.content.len()..self.content.len();
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            self.move_line(-1., cx);
        } else {
            cx.emit(TextEvent::Up);
        }
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            self.move_line(1., cx);
        } else {
            cx.emit(TextEvent::Down);
        }
    }

    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(TextEvent::Cancel);
    }

    fn move_line(&mut self, direction: f32, cx: &mut Context<Self>) {
        let Some(layout) = &self.last_layout else { return };
        let Some(pos) = layout.position_for(self.cursor_offset()) else { return };
        let target = point(pos.x, pos.y + layout.line_height * direction + layout.line_height / 2.);
        let offset = layout.offset_for(target, self.content.len()).min(self.content.len());
        self.move_to(offset, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        let cursor = self.cursor_offset();
        let start = self.content[..cursor].rfind('\n').map_or(0, |i| i + 1);
        self.move_to(start, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        let cursor = self.cursor_offset();
        let end = self.content[cursor..].find('\n').map_or(self.content.len(), |i| cursor + i);
        self.move_to(end, cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let prev = self.previous_boundary(self.cursor_offset());
            if prev == self.cursor_offset() {
                return;
            }
            self.select_to(prev, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let next = self.next_boundary(self.cursor_offset());
            if next == self.cursor_offset() {
                return;
            }
            self.select_to(next, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn enter(&mut self, _: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            self.replace_text_in_range(None, "\n", window, cx);
        } else {
            cx.emit(TextEvent::Submit);
        }
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let text = text.replace("\r\n", "\n");
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.content[self.selected_range.clone()].to_string()));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.content[self.selected_range.clone()].to_string()));
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle, cx);
        self.is_selecting = true;
        let offset = self.offset_for_mouse(event.position);
        if event.modifiers.shift {
            self.select_to(offset, cx);
        } else {
            self.move_to(offset, cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.offset_for_mouse(event.position), cx);
        }
    }

    fn offset_for_mouse(&self, position: Point<Pixels>) -> usize {
        let Some(layout) = &self.last_layout else { return 0 };
        let local = point(position.x - layout.bounds.origin.x, position.y - layout.bounds.origin.y);
        layout.offset_for(local, self.content.len()).min(self.content.len())
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        cx.notify();
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed { self.selected_range.start } else { self.selected_range.end }
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify();
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content.grapheme_indices(true).rev().find_map(|(idx, _)| (idx < offset).then_some(idx)).unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }

    /// Text to draw (content, or the placeholder when empty) and its style runs.
    fn display_runs(&self, window: &Window, placeholder: Hsla) -> (SharedString, Vec<TextRun>) {
        let style = window.text_style();
        let (text, color) = if self.content.is_empty() {
            (self.placeholder.clone(), placeholder)
        } else {
            (SharedString::from(self.content.clone()), style.color)
        };
        let run = TextRun {
            len: text.len(),
            font: style.font(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = match &self.marked_range {
            Some(marked) if !self.content.is_empty() => vec![
                TextRun { len: marked.start, ..run.clone() },
                TextRun {
                    len: marked.end - marked.start,
                    underline: Some(UnderlineStyle { color: Some(run.color), thickness: px(1.), wavy: false }),
                    ..run.clone()
                },
                TextRun { len: text.len() - marked.end, ..run },
            ]
            .into_iter()
            .filter(|r| r.len > 0)
            .collect(),
            _ => vec![run],
        };
        (text, runs)
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection { range: self.range_to_utf16(&self.selected_range), reversed: self.selection_reversed })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range.as_ref().map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let new_text = if self.multiline { new_text.to_string() } else { new_text.replace('\n', " ") };
        self.content = format!("{}{}{}", &self.content[..range.start], new_text, &self.content[range.end..]);
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.marked_range = None;
        cx.emit(TextEvent::Changed);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        self.content = format!("{}{}{}", &self.content[..range.start], new_text, &self.content[range.end..]);
        self.marked_range = (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .map(|r| r.start + range.start..r.end + range.start)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        cx.emit(TextEvent::Changed);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        let start = layout.position_for(range.start)?;
        let end = layout.position_for(range.end)?;
        Some(Bounds::from_corners(
            point(bounds.left() + start.x, bounds.top() + start.y),
            point(bounds.left() + end.x, bounds.top() + end.y + layout.line_height),
        ))
    }

    fn character_index_for_point(&mut self, p: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        let layout = self.last_layout.as_ref()?;
        let local = layout.bounds.localize(&p)?;
        let offset = layout.offset_for(local, self.content.len()).min(self.content.len());
        Some(self.offset_to_utf16(offset))
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    layout: Option<TextLayout>,
    cursor: Option<PaintQuad>,
    selections: Vec<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let line_height = window.line_height();
        let input = self.input.read(cx);
        if !input.multiline {
            // Note: single-line inputs clip long text instead of scrolling horizontally.
            style.size.height = line_height.into();
            return (window.request_layout(style, [], cx), ());
        }
        let (text, runs) = input.display_runs(window, theme::current(cx).muted.into());
        let font_size = window.text_style().font_size.to_pixels(window.rem_size());
        let layout_id = window.request_measured_layout(style, move |known, available, window, _| {
            let width = known.width.or(match available.width {
                AvailableSpace::Definite(w) => Some(w),
                _ => None,
            });
            let rows = window
                .text_system()
                .shape_text(text.clone(), font_size, &runs, width, None)
                .map(|lines| lines.iter().map(|l| l.wrap_boundaries().len() + 1).sum::<usize>())
                .unwrap_or(1);
            size(width.unwrap_or_default(), line_height * rows.max(MIN_ROWS) as f32)
        });
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let colors = theme::current(cx);
        let input = self.input.read(cx);
        let (text, runs) = input.display_runs(window, colors.muted.into());
        let font_size = window.text_style().font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let wrap_width = input.multiline.then_some(bounds.size.width);
        let shaped = window
            .text_system()
            .shape_text(text.clone(), font_size, &runs, wrap_width, None)
            .unwrap_or_default();
        let mut start = 0;
        let lines = shaped
            .into_iter()
            .zip(text.split('\n'))
            .map(|(line, segment)| {
                let line_start = start;
                start += segment.len() + 1;
                (line_start, line)
            })
            .collect();
        let layout = TextLayout { lines, bounds, line_height };
        let (selections, cursor) = if input.selected_range.is_empty() {
            let cursor = layout.position_for(input.cursor_offset()).map(|p| {
                fill(Bounds::new(point(bounds.left() + p.x, bounds.top() + p.y), size(px(1.5), line_height)), colors.accent)
            });
            (Vec::new(), cursor)
        } else {
            (layout.selection_quads(&input.selected_range, colors.selection.into()), None)
        };
        PrepaintState { layout: Some(layout), cursor, selections }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(&focus_handle, ElementInputHandler::new(bounds, self.input.clone()), cx);
        for quad in prepaint.selections.drain(..) {
            window.paint_quad(quad);
        }
        let Some(layout) = prepaint.layout.take() else { return };
        let mut top = bounds.top();
        for (_, line) in &layout.lines {
            let _ = line.paint(point(bounds.left(), top), layout.line_height, TextAlign::Left, None, window, cx);
            top += line.size(layout.line_height).height;
        }
        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }
        self.input.update(cx, |input, _| input.last_layout = Some(layout));
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme::current(cx);
        let focused = self.focus_handle.is_focused(window);
        div()
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cancel))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .w_full()
            .px_3()
            .py_2()
            .rounded_lg()
            .border_1()
            .border_color(if focused { c.accent } else { c.border })
            .bg(c.bg)
            .text_sm()
            .text_color(c.text)
            .overflow_hidden()
            .child(TextElement { input: cx.entity() })
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<TextEvent> for TextInput {}
