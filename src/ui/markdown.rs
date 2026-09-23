//! Markdown descriptions. `parse` turns source text into a small block model (pure, unit-tested)
//! and `render` draws it with GPUI styled text. Supported: headings, paragraphs, nested and task
//! lists, code blocks, quotes, rules, and inline bold, italic, strikethrough, code and links.

use crate::theme::Colors;
use crate::ui::icons::{self, icon};
use gpui::{
    AnyElement, Div, FontStyle, FontWeight, HighlightStyle, Hsla, InteractiveText, IntoElement, SharedString,
    StrikethroughStyle, StyledText, UnderlineStyle, div, prelude::*, px, white,
};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::ops::Range;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub link: bool,
}

/// Text of one paragraph or heading: contiguous, non-overlapping styled spans plus link targets.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inline {
    pub text: String,
    pub spans: Vec<(Range<usize>, Style)>,
    pub links: Vec<(Range<usize>, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Heading(u8, Inline),
    Paragraph(Inline),
    List { start: Option<u64>, items: Vec<Item> },
    Code(String),
    Quote(Vec<Block>),
    Rule,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    /// `Some` for task-list items (`- [x]`).
    pub checked: Option<bool>,
    pub blocks: Vec<Block>,
}

enum Container {
    Root(Vec<Block>),
    Quote(Vec<Block>),
    List { start: Option<u64>, items: Vec<Item> },
    Item(Item),
}

struct Builder {
    stack: Vec<Container>,
    /// Open paragraph (`None` level) or heading (`Some(level)`).
    inline: Option<(Option<u8>, Inline)>,
    bold: u32,
    italic: u32,
    strike: u32,
    link: Option<(usize, String)>,
    code: Option<String>,
}

impl Builder {
    fn push_block(&mut self, block: Block) {
        match self.stack.last_mut() {
            Some(Container::Root(blocks)) | Some(Container::Quote(blocks)) => blocks.push(block),
            Some(Container::Item(item)) => item.blocks.push(block),
            Some(Container::List { .. }) | None => {}
        }
    }

    fn flush_inline(&mut self) {
        if let Some((level, inline)) = self.inline.take() {
            self.push_block(match level {
                Some(level) => Block::Heading(level, inline),
                None => Block::Paragraph(inline),
            });
        }
    }

    /// The open inline; text directly inside a tight list item opens an implicit paragraph.
    fn inline_mut(&mut self) -> &mut Inline {
        &mut self.inline.get_or_insert_with(|| (None, Inline::default())).1
    }

    fn text(&mut self, text: &str, code: bool) {
        let style = Style {
            bold: self.bold > 0,
            italic: self.italic > 0,
            strike: self.strike > 0,
            code,
            link: self.link.is_some(),
        };
        let inline = self.inline_mut();
        let start = inline.text.len();
        inline.text.push_str(text);
        let end = inline.text.len();
        match inline.spans.last_mut() {
            Some((range, s)) if *s == style && range.end == start => range.end = end,
            _ => inline.spans.push((start..end, style)),
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => {
                self.flush_inline();
                self.inline = Some((None, Inline::default()));
            }
            Tag::Heading { level, .. } => {
                self.flush_inline();
                self.inline = Some((Some(level as u8), Inline::default()));
            }
            Tag::BlockQuote(_) => {
                self.flush_inline();
                self.stack.push(Container::Quote(Vec::new()));
            }
            Tag::CodeBlock(_) => {
                self.flush_inline();
                self.code = Some(String::new());
            }
            Tag::List(start) => {
                self.flush_inline();
                self.stack.push(Container::List { start, items: Vec::new() });
            }
            Tag::Item => {
                self.flush_inline();
                self.stack.push(Container::Item(Item::default()));
            }
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { dest_url, .. } => {
                let start = self.inline_mut().text.len();
                self.link = Some((start, dest_url.to_string()));
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Heading(_) => self.flush_inline(),
            TagEnd::BlockQuote(_) => {
                self.flush_inline();
                if let Some(Container::Quote(blocks)) = self.stack.pop() {
                    self.push_block(Block::Quote(blocks));
                }
            }
            TagEnd::CodeBlock => {
                if let Some(code) = self.code.take() {
                    self.push_block(Block::Code(code.trim_end_matches('\n').to_string()));
                }
            }
            TagEnd::List(_) => {
                self.flush_inline();
                if let Some(Container::List { start, items }) = self.stack.pop() {
                    self.push_block(Block::List { start, items });
                }
            }
            TagEnd::Item => {
                self.flush_inline();
                if let Some(Container::Item(item)) = self.stack.pop()
                    && let Some(Container::List { items, .. }) = self.stack.last_mut()
                {
                    items.push(item);
                }
            }
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::Link => {
                if let Some((start, url)) = self.link.take() {
                    let inline = self.inline_mut();
                    let end = inline.text.len();
                    if end > start {
                        inline.links.push((start..end, url));
                    }
                }
            }
            _ => {}
        }
    }
}

pub fn parse(source: &str) -> Vec<Block> {
    let mut b = Builder {
        stack: vec![Container::Root(Vec::new())],
        inline: None,
        bold: 0,
        italic: 0,
        strike: 0,
        link: None,
        code: None,
    };
    for event in Parser::new_ext(source, Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS) {
        match event {
            Event::Start(tag) => b.start(tag),
            Event::End(tag) => b.end(tag),
            Event::Text(text) => match &mut b.code {
                Some(code) => code.push_str(&text),
                None => b.text(&text, false),
            },
            Event::Code(text) => b.text(&text, true),
            Event::SoftBreak => b.text(" ", false),
            Event::HardBreak => b.text("\n", false),
            Event::Html(text) | Event::InlineHtml(text) => b.text(&text, false),
            Event::Rule => {
                b.flush_inline();
                b.push_block(Block::Rule);
            }
            Event::TaskListMarker(checked) => {
                if let Some(Container::Item(item)) = b.stack.last_mut() {
                    item.checked = Some(checked);
                }
            }
            _ => {}
        }
    }
    b.flush_inline();
    match b.stack.into_iter().next() {
        Some(Container::Root(blocks)) => blocks,
        _ => Vec::new(),
    }
}

/// The first block of `source` as one line of plain text (task cards show this).
pub fn summary(source: &str) -> String {
    fn first(blocks: &[Block]) -> Option<String> {
        blocks.iter().find_map(|block| match block {
            Block::Heading(_, inline) | Block::Paragraph(inline) => Some(inline.text.clone()),
            Block::Code(code) => code.lines().next().map(str::to_string),
            Block::Quote(inner) => first(inner),
            Block::List { items, .. } => items.iter().find_map(|item| first(&item.blocks)),
            Block::Rule => None,
        })
    }
    first(&parse(source)).and_then(|text| text.lines().next().map(|l| l.trim().to_string())).unwrap_or_default()
}

/// Links in descriptions may come from other people later (shared tasks), so only web and mail
/// links are handed to the OS.
pub fn is_safe_url(url: &str) -> bool {
    ["https://", "http://", "mailto:"].iter().any(|scheme| url.starts_with(scheme))
}

/// Draws parsed Markdown. `id` must be unique on the page; it keys the clickable link texts.
pub fn render(blocks: &[Block], id: &str, c: &Colors) -> Div {
    let mut counter = 0;
    blocks_element(blocks, id, &mut counter, c).text_sm().text_color(c.text)
}

fn blocks_element(blocks: &[Block], id: &str, counter: &mut usize, c: &Colors) -> Div {
    let mut element = div().flex().flex_col().gap_2();
    for block in blocks {
        element = element.child(block_element(block, id, counter, c));
    }
    element
}

fn block_element(block: &Block, id: &str, counter: &mut usize, c: &Colors) -> AnyElement {
    match block {
        Block::Heading(level, inline) => div()
            .font_weight(FontWeight::BOLD)
            .map(|d| match level {
                1 => d.text_lg(),
                2 => d.text_base(),
                _ => d.text_sm(),
            })
            .child(inline_element(inline, id, counter, c))
            .into_any_element(),
        Block::Paragraph(inline) => div().child(inline_element(inline, id, counter, c)).into_any_element(),
        Block::Code(code) => div()
            .p_2()
            .rounded_md()
            .bg(c.bg)
            .border_1()
            .border_color(c.border)
            .font_family("Consolas")
            .text_xs()
            .child(code.clone())
            .into_any_element(),
        Block::Quote(inner) => div()
            .pl_3()
            .border_l_2()
            .border_color(c.border)
            .text_color(c.muted)
            .child(blocks_element(inner, id, counter, c))
            .into_any_element(),
        Block::Rule => div().h(px(1.)).bg(c.border).into_any_element(),
        Block::List { start, items } => {
            let mut list = div().flex().flex_col().gap_1();
            for (ix, item) in items.iter().enumerate() {
                let marker: AnyElement = match (item.checked, start) {
                    (Some(done), _) => div()
                        .mt(px(3.))
                        .size(px(14.))
                        .rounded_sm()
                        .border_1()
                        .border_color(if done { c.accent } else { c.muted })
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .when(done, |d| d.bg(c.accent).text_color(white()).child(icon(icons::CHECK)))
                        .into_any_element(),
                    (None, Some(first)) => div().child(format!("{}.", first + ix as u64)).into_any_element(),
                    (None, None) => div().child("•").into_any_element(),
                };
                list = list.child(
                    div()
                        .flex()
                        .gap_2()
                        .child(div().flex_none().min_w(px(16.)).text_color(c.muted).child(marker))
                        .child(div().flex_1().min_w_0().child(blocks_element(&item.blocks, id, counter, c))),
                );
            }
            list.into_any_element()
        }
    }
}

fn inline_element(inline: &Inline, id: &str, counter: &mut usize, c: &Colors) -> AnyElement {
    let accent: Hsla = c.accent.into();
    let code_bg = Hsla::from(c.muted).opacity(0.18);
    let highlights: Vec<(Range<usize>, HighlightStyle)> = inline
        .spans
        .iter()
        .filter(|(_, style)| *style != Style::default())
        .map(|(range, s)| {
            let style = HighlightStyle {
                font_weight: s.bold.then_some(FontWeight::BOLD),
                font_style: s.italic.then_some(FontStyle::Italic),
                strikethrough: s.strike.then_some(StrikethroughStyle { thickness: px(1.), color: None }),
                background_color: s.code.then_some(code_bg),
                color: s.link.then_some(accent),
                underline: s.link.then_some(UnderlineStyle { thickness: px(1.), color: Some(accent), wavy: false }),
                ..Default::default()
            };
            (range.clone(), style)
        })
        .collect();
    let text = StyledText::new(inline.text.clone()).with_highlights(highlights);
    if inline.links.is_empty() {
        return text.into_any_element();
    }
    *counter += 1;
    let ranges = inline.links.iter().map(|(range, _)| range.clone()).collect();
    let urls: Vec<String> = inline.links.iter().map(|(_, url)| url.clone()).collect();
    InteractiveText::new(SharedString::from(format!("{id}-links-{counter}")), text)
        .on_click(ranges, move |ix, _, cx| {
            if is_safe_url(&urls[ix]) {
                cx.open_url(&urls[ix]);
            }
        })
        .into_any_element()
}

#[cfg(test)]
#[path = "markdown_tests.rs"]
mod tests;
