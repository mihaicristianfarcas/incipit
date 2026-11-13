//! Markdown, as models write it, drawn with the theme: paragraphs,
//! headings, emphasis, inline code and code blocks, lists (nested, numbered,
//! tasks), block quotes, links (opened in the browser), rules, tables and
//! math.
//!
//! Math is typeset (`formula`), as the earlier app set it with KaTeX: `$…$`
//! in a line, sitting on its baseline, and `$$…$$` a block of its own when
//! it has its line or lines to itself (written inside a sentence, it is set
//! in the line, as the earlier app set it). A fenced block whose language is
//! `math` is a block of math too. Models often write `\(…\)` and `\[…\]`:
//! outside code they are read as `$…$` and `$$…$$` first, as in the earlier
//! app. What is math is pulldown-cmark's rule: a `$` opens only before
//! something other than a space and closes only after one, so a lone `$` in
//! prose, a price, stays text, as do code and an escaped `\$`. Math that
//! cannot be typeset, and math still being typeset, stays as written.
//!
//! A flashcard's text is drawn with its cloze (`markdown_cloze`): `{{the
//! hidden words}}` stand out once the answer is shown and are a blank until
//! then. What counts as a cloze is the earlier app's rule: two opening
//! braces, one or more characters that are neither a closing brace nor a
//! line break, two closing braces, all in one stretch of plain text. So
//! braces in code, in math or in the words of a link are left alone, and a cloze does not reach across emphasis or a
//! line break. Its blank is the earlier app's too, as near as text allows: a
//! sunken stretch with `[…]` at its end, about as wide as the words it hides
//! (the mark stands in for their last three characters, and for all of a
//! shorter cloze, where the earlier app let the mark run over the text
//! before). The hidden words that keep the width are in the text and are not
//! drawn.

use std::borrow::Cow;
use std::ops::Range;

use gpui::{
    div, prelude::*, px, AnyElement, App, ElementId, FontStyle, FontWeight, HighlightStyle,
    InteractiveText, SharedString, StrikethroughStyle, StyledText, UnderlineStyle, WhiteSpace,
    Window,
};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use unicode_segmentation::UnicodeSegmentation;

use crate::formula::{formula, lazily};
use crate::theme::{Radius, Theme, Type};

/// How a card's `{{hidden words}}` are drawn ([`markdown_cloze`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cloze {
    /// In full and set apart: semibold, underlined in the accent.
    Shown,
    /// As a blank, until the answer is shown.
    Hidden,
}

/// What a stretch of a cloze is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClozePart {
    /// The words, shown.
    Words,
    /// Words of a blank: they take their room and are not drawn.
    Blank,
    /// The mark at the end of a blank.
    Mark,
}

/// What marks a blank: the earlier app's.
const BLANK_MARK: &str = "[…]";
/// How many of a blank's last characters its mark stands in for. The mark is about as wide as
/// three letters, so the blank stays about as wide as the words it hides.
const MARK_STANDS_FOR: usize = 3;

#[derive(Default, Clone, Copy, PartialEq)]
struct Marks {
    bold: bool,
    italic: bool,
    code: bool,
    strike: bool,
    link: bool,
    cloze: Option<ClozePart>,
}

/// Text with its marked ranges.
#[derive(Default, Clone)]
struct Inline {
    text: String,
    runs: Vec<(Range<usize>, Marks)>,
    links: Vec<(Range<usize>, String)>,
    /// The formulas in it, in order; the text holds each as it was written.
    math: Vec<Math>,
    /// Where a line of the source ended, as the space or line break that stands for it.
    breaks: Vec<usize>,
}

/// A formula in a stretch of text: where its written form is, its TeX, and whether it was
/// written `$$…$$`.
#[derive(Clone, Debug, PartialEq)]
struct Math {
    range: Range<usize>,
    tex: String,
    display: bool,
}

impl Inline {
    fn push_math(&mut self, tex: &str, display: bool, marks: Marks) {
        let fence = if display { "$$" } else { "$" };
        let start = self.text.len();
        self.push(&format!("{fence}{tex}{fence}"), marks);
        self.math.push(Math {
            range: start..self.text.len(),
            tex: tex.to_string(),
            display,
        });
    }

    /// Adds `other` at the end.
    fn append(&mut self, other: Inline) {
        let shift = self.text.len();
        self.text.push_str(&other.text);
        let moved = |r: Range<usize>| r.start + shift..r.end + shift;
        self.runs
            .extend(other.runs.into_iter().map(|(r, m)| (moved(r), m)));
        self.links
            .extend(other.links.into_iter().map(|(r, u)| (moved(r), u)));
        self.math.extend(other.math.into_iter().map(|m| Math {
            range: moved(m.range),
            ..m
        }));
        self.breaks
            .extend(other.breaks.into_iter().map(|b| b + shift));
    }

    /// The stretch `range` of it, with what is marked in it.
    fn slice(&self, range: Range<usize>) -> Inline {
        let within = |r: &Range<usize>| {
            let (start, end) = (r.start.max(range.start), r.end.min(range.end));
            (start < end).then(|| start - range.start..end - range.start)
        };
        Inline {
            text: self.text[range.clone()].to_string(),
            runs: self
                .runs
                .iter()
                .filter_map(|(r, m)| Some((within(r)?, *m)))
                .collect(),
            links: self
                .links
                .iter()
                .filter_map(|(r, u)| Some((within(r)?, u.clone())))
                .collect(),
            math: self
                .math
                .iter()
                .filter(|m| range.start <= m.range.start && m.range.end <= range.end)
                .map(|m| Math {
                    range: m.range.start - range.start..m.range.end - range.start,
                    ..m.clone()
                })
                .collect(),
            breaks: self
                .breaks
                .iter()
                .filter(|b| range.contains(b))
                .map(|b| b - range.start)
                .collect(),
        }
    }

    fn push(&mut self, s: &str, marks: Marks) {
        if s.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(s);
        if marks != Marks::default() {
            self.runs.push((start..self.text.len(), marks));
        }
    }

    fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }
}

enum Block {
    Paragraph(Inline),
    Heading(u8, Inline),
    Code(String),
    List {
        start: Option<u64>,
        items: Vec<Vec<Block>>,
    },
    Quote(Vec<Block>),
    Rule,
    Table(Vec<Vec<Inline>>),
    /// A formula of its own: its TeX, and whether it was a fenced block (else `$$…$$`).
    Math(String, bool),
}

/// The words of each cloze in a stretch of plain text, `{{the words}}`: two opening braces, one
/// or more characters that are neither a closing brace nor a line break, two closing braces.
/// The earliest one counts, and the next is looked for after it.
fn cloze_spans(text: &str) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut at = 0;
    while let Some(found) = text[at..].find("{{") {
        let open = at + found;
        let start = open + 2;
        let len = text[start..]
            .find(['}', '\n'])
            .unwrap_or(text.len() - start);
        let end = start + len;
        if len > 0 && text[end..].starts_with("}}") {
            spans.push(start..end);
            at = end + 2;
        } else {
            at = open + 1;
        }
    }
    spans
}

/// The words of a blank that keep their room: all but the last few, which the mark stands in
/// for. Nothing of a cloze no longer than those.
fn kept_of_blank(words: &str) -> &str {
    let cut = words
        .grapheme_indices(true)
        .rev()
        .nth(MARK_STANDS_FOR - 1)
        .map_or(0, |(at, _)| at);
    &words[..cut]
}

/// The byte ranges of `text` that are code: inline code and code blocks.
fn code_ranges(text: &str) -> Vec<Range<usize>> {
    Parser::new_ext(text, Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS)
        .into_offset_iter()
        .filter(|(event, _)| matches!(event, Event::Code(_) | Event::Start(Tag::CodeBlock(_))))
        .map(|(_, range)| range)
        .collect()
}

/// `\[…\]` read as `$$…$$` and `\(…\)` as `$…$` (what is inside trimmed), where they start
/// outside code: the earlier app's reading of how models often write math. Each runs from its
/// opening to the first closing after at least one character.
fn normalize_math(text: &str) -> Cow<'_, str> {
    if !text.contains("\\(") && !text.contains("\\[") {
        return Cow::Borrowed(text);
    }
    let code = code_ranges(text);
    let in_code = |at: usize| code.iter().any(|r| r.contains(&at));
    let between = |text: &str, open: &str, close: &str, make: &dyn Fn(&str) -> String| {
        let mut out = String::with_capacity(text.len());
        let mut at = 0;
        while let Some(found) = text[at..].find(open) {
            let start = at + found;
            let inner = start + open.len();
            let Some(first) = text[inner..].chars().next() else {
                break;
            };
            let Some(closed) = text[inner + first.len_utf8()..].find(close) else {
                break;
            };
            let end = inner + first.len_utf8() + closed;
            out.push_str(&text[at..start]);
            if in_code(start) {
                out.push_str(&text[start..end + close.len()]);
            } else {
                out.push_str(&make(&text[inner..end]));
            }
            at = end + close.len();
        }
        out.push_str(&text[at..]);
        out
    };
    // `\[` and `\]` are as long as `$$`, so the code is where it was for the second pass.
    let displayed = between(text, "\\[", "\\]", &|tex| format!("$${tex}$$"));
    Cow::Owned(between(&displayed, "\\(", "\\)", &|tex| {
        format!("${}$", tex.trim())
    }))
}

/// A paragraph's blocks: a formula written `$$…$$` that has its line or lines to itself is a
/// block of its own, and the text around it stays paragraphs.
fn paragraphs(inline: Inline) -> Vec<Block> {
    let len = inline.text.len();
    let alone = |m: &&Math| {
        m.display
            && (m.range.start == 0 || inline.breaks.contains(&(m.range.start - 1)))
            && (m.range.end == len || inline.breaks.contains(&m.range.end))
    };
    let blocks: Vec<&Math> = inline.math.iter().filter(alone).collect();
    if blocks.is_empty() {
        return vec![Block::Paragraph(inline)];
    }
    let mut out = Vec::new();
    let mut at = 0;
    for math in blocks {
        // The line ends around it go with it.
        let before = inline.slice(at..math.range.start.saturating_sub(1).max(at));
        if !before.is_empty() {
            out.push(Block::Paragraph(before));
        }
        out.push(Block::Math(math.tex.clone(), false));
        at = (math.range.end + 1).min(len);
    }
    let after = inline.slice(at..len);
    if !after.is_empty() {
        out.push(Block::Paragraph(after));
    }
    out
}

/// Parse `source` into blocks. With `cloze`, `{{…}}` in plain text is a cloze, drawn that way.
fn parse(source: &str, cloze: Option<Cloze>) -> Vec<Block> {
    let source = normalize_math(source);
    let mut opts = Options::empty();
    opts.insert(
        Options::ENABLE_TABLES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            // Math is told apart from text, to be typeset, and so that braces in it are never
            // a cloze.
            | Options::ENABLE_MATH,
    );
    let events: Vec<Event> = Parser::new_ext(&source, opts).collect();
    let mut pos = 0;
    parse_blocks(&events, &mut pos, None, cloze)
}

/// Blocks until the closing tag `until` (or the end).
fn parse_blocks(
    events: &[Event],
    pos: &mut usize,
    until: Option<TagEnd>,
    cloze: Option<Cloze>,
) -> Vec<Block> {
    let mut out = Vec::new();
    // Loose text in a tight list item is a paragraph of its own.
    let mut loose = Inline::default();
    let flush = |loose: &mut Inline, out: &mut Vec<Block>| {
        if !loose.is_empty() {
            out.extend(paragraphs(std::mem::take(loose)));
        } else {
            *loose = Inline::default();
        }
    };
    while *pos < events.len() {
        let ev = events[*pos].clone();
        *pos += 1;
        match ev {
            Event::End(end) if Some(end) == until => {
                flush(&mut loose, &mut out);
                return out;
            }
            Event::Start(Tag::Paragraph) => {
                flush(&mut loose, &mut out);
                out.extend(paragraphs(parse_inline(
                    events,
                    pos,
                    TagEnd::Paragraph,
                    cloze,
                )));
            }
            Event::Start(Tag::Heading { level, .. }) => {
                flush(&mut loose, &mut out);
                let n = match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    _ => 4,
                };
                out.push(Block::Heading(
                    n,
                    parse_inline(events, pos, TagEnd::Heading(level), cloze),
                ));
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                flush(&mut loose, &mut out);
                let mut code = String::new();
                while *pos < events.len() {
                    let e = events[*pos].clone();
                    *pos += 1;
                    match e {
                        Event::Text(t) => code.push_str(&t),
                        Event::End(TagEnd::CodeBlock) => break,
                        _ => {}
                    }
                }
                let code = code.trim_end_matches('\n').to_string();
                // A fence of `math` is a formula, as the earlier app drew it.
                let math = matches!(&kind, CodeBlockKind::Fenced(info)
                    if info.split_whitespace().next() == Some("math"));
                out.push(if math {
                    Block::Math(code, true)
                } else {
                    Block::Code(code)
                });
            }
            Event::Start(Tag::List(start)) => {
                flush(&mut loose, &mut out);
                let mut items = Vec::new();
                while *pos < events.len() {
                    match events[*pos].clone() {
                        Event::Start(Tag::Item) => {
                            *pos += 1;
                            items.push(parse_blocks(events, pos, Some(TagEnd::Item), cloze));
                        }
                        Event::End(TagEnd::List(_)) => {
                            *pos += 1;
                            break;
                        }
                        _ => *pos += 1,
                    }
                }
                out.push(Block::List { start, items });
            }
            Event::Start(Tag::BlockQuote(_)) => {
                flush(&mut loose, &mut out);
                out.push(Block::Quote(parse_blocks(
                    events,
                    pos,
                    Some(TagEnd::BlockQuote(None)),
                    cloze,
                )));
            }
            Event::Start(Tag::Table(_)) => {
                flush(&mut loose, &mut out);
                let mut rows: Vec<Vec<Inline>> = Vec::new();
                while *pos < events.len() {
                    match events[*pos].clone() {
                        Event::Start(Tag::TableHead) | Event::Start(Tag::TableRow) => {
                            *pos += 1;
                            rows.push(Vec::new());
                        }
                        Event::Start(Tag::TableCell) => {
                            *pos += 1;
                            let cell = parse_inline(events, pos, TagEnd::TableCell, cloze);
                            if let Some(r) = rows.last_mut() {
                                r.push(cell);
                            }
                        }
                        Event::End(TagEnd::Table) => {
                            *pos += 1;
                            break;
                        }
                        _ => *pos += 1,
                    }
                }
                out.push(Block::Table(rows));
            }
            Event::Rule => {
                flush(&mut loose, &mut out);
                out.push(Block::Rule);
            }
            Event::TaskListMarker(done) => {
                loose.push(if done { "☑ " } else { "☐ " }, Marks::default())
            }
            // Inline content outside a paragraph (a tight list item).
            Event::Text(_)
            | Event::Code(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_)
            | Event::Start(Tag::Emphasis)
            | Event::Start(Tag::Strong)
            | Event::Start(Tag::Link { .. })
            | Event::SoftBreak
            | Event::HardBreak => {
                *pos -= 1;
                let inline = parse_inline_until_block(events, pos, cloze);
                if loose.text.is_empty() {
                    loose = inline;
                } else {
                    loose.append(inline);
                }
            }
            _ => {}
        }
    }
    flush(&mut loose, &mut out);
    out
}

/// Inline events being gathered into an [`Inline`].
struct Gather {
    inline: Inline,
    marks: Marks,
    link: Option<(usize, String)>,
    /// How a cloze is drawn; none where `{{…}}` is left as written.
    cloze: Option<Cloze>,
    /// Text events in a row, not yet added. The parser hands one stretch of plain text over in
    /// pieces (at a bracket, at an underscore), and a cloze is looked for in the whole stretch.
    text: String,
    /// The spans open around the text, innermost last: whether each is a link.
    open: Vec<bool>,
}

impl Gather {
    fn new(cloze: Option<Cloze>) -> Gather {
        Gather {
            inline: Inline::default(),
            marks: Marks::default(),
            link: None,
            cloze,
            text: String::new(),
            open: Vec::new(),
        }
    }

    fn event(&mut self, ev: Event) {
        if let Event::Text(t) = &ev {
            self.text.push_str(t);
            return;
        }
        self.flush();
        match ev {
            Event::Code(t) => self.inline.push(
                &t,
                Marks {
                    code: true,
                    ..self.marks
                },
            ),
            Event::InlineMath(t) => self.inline.push_math(&t, false, self.marks),
            Event::DisplayMath(t) => self.inline.push_math(&t, true, self.marks),
            Event::SoftBreak => {
                self.inline.breaks.push(self.inline.text.len());
                self.inline.push(" ", self.marks);
            }
            Event::HardBreak => {
                self.inline.breaks.push(self.inline.text.len());
                self.inline.push("\n", self.marks);
            }
            Event::Start(Tag::Emphasis) => self.span(|m| m.italic = true, false),
            Event::End(TagEnd::Emphasis) => self.span_end(|m| m.italic = false),
            Event::Start(Tag::Strong) => self.span(|m| m.bold = true, false),
            Event::End(TagEnd::Strong) => self.span_end(|m| m.bold = false),
            Event::Start(Tag::Strikethrough) => self.span(|m| m.strike = true, false),
            Event::End(TagEnd::Strikethrough) => self.span_end(|m| m.strike = false),
            Event::Start(Tag::Link { dest_url, .. }) => {
                self.link = Some((self.inline.text.len(), dest_url.to_string()));
                self.span(|m| m.link = true, true);
            }
            Event::End(TagEnd::Link) => {
                self.span_end(|m| m.link = false);
                if let Some((start, url)) = self.link.take() {
                    self.inline.links.push((start..self.inline.text.len(), url));
                }
            }
            _ => {}
        }
    }

    fn span(&mut self, mark: impl FnOnce(&mut Marks), is_link: bool) {
        mark(&mut self.marks);
        self.open.push(is_link);
    }

    fn span_end(&mut self, unmark: impl FnOnce(&mut Marks)) {
        unmark(&mut self.marks);
        self.open.pop();
    }

    /// Add the text gathered so far, its clozes set apart. The words of a link itself are
    /// never a cloze, as in the earlier app.
    fn flush(&mut self) {
        if self.text.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.text);
        let in_a_link = self.open.last() == Some(&true);
        let Some(cloze) = self.cloze.filter(|_| !in_a_link) else {
            self.inline.push(&text, self.marks);
            return;
        };
        let part = |part| Marks {
            cloze: Some(part),
            ..self.marks
        };
        let mut at = 0;
        for words in cloze_spans(&text) {
            self.inline.push(&text[at..words.start - 2], self.marks);
            at = words.end + 2;
            let words = &text[words];
            match cloze {
                Cloze::Shown => self.inline.push(words, part(ClozePart::Words)),
                Cloze::Hidden => {
                    self.inline
                        .push(kept_of_blank(words), part(ClozePart::Blank));
                    self.inline.push(BLANK_MARK, part(ClozePart::Mark));
                }
            }
        }
        self.inline.push(&text[at..], self.marks);
    }

    fn finish(mut self) -> Inline {
        self.flush();
        self.inline
    }
}

/// Inline events up to `end`.
fn parse_inline(events: &[Event], pos: &mut usize, end: TagEnd, cloze: Option<Cloze>) -> Inline {
    let mut gather = Gather::new(cloze);
    while *pos < events.len() {
        let ev = events[*pos].clone();
        *pos += 1;
        if matches!(&ev, Event::End(e) if *e == end) {
            break;
        }
        gather.event(ev);
    }
    gather.finish()
}

/// Inline events up to the next block start or end.
fn parse_inline_until_block(events: &[Event], pos: &mut usize, cloze: Option<Cloze>) -> Inline {
    let mut gather = Gather::new(cloze);
    while *pos < events.len() {
        let ev = events[*pos].clone();
        let inline_kind = matches!(
            ev,
            Event::Text(_)
                | Event::Code(_)
                | Event::InlineMath(_)
                | Event::DisplayMath(_)
                | Event::SoftBreak
                | Event::HardBreak
                | Event::Start(Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link { .. })
                | Event::End(
                    TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link
                )
        );
        if !inline_kind {
            break;
        }
        *pos += 1;
        gather.event(ev);
    }
    gather.finish()
}

/// Draw `source` as Markdown.
pub fn markdown(id: impl Into<ElementId>, source: &str, t: Theme) -> AnyElement {
    draw(id.into(), source, t, None)
}

/// Draw a flashcard's text: Markdown in which `{{the hidden words}}` is a cloze, shown in full
/// or as a blank.
pub fn markdown_cloze(
    id: impl Into<ElementId>,
    source: &str,
    t: Theme,
    cloze: Cloze,
) -> AnyElement {
    draw(id.into(), source, t, Some(cloze))
}

fn draw(id: ElementId, source: &str, t: Theme, cloze: Option<Cloze>) -> AnyElement {
    let blocks = parse(source, cloze);
    let mut n = 0usize;
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .children(blocks.iter().map(|b| render_block(&id, &mut n, b, t, 0)))
        .into_any_element()
}

fn render_block(
    id: &ElementId,
    n: &mut usize,
    block: &Block,
    t: Theme,
    depth: usize,
) -> AnyElement {
    *n += 1;
    let key = ElementId::Name(format!("{id:?}-{n}").into());
    match block {
        Block::Paragraph(inline) => render_text(key, inline, t, Type::BODY, FontWeight::NORMAL),
        Block::Heading(level, inline) => {
            let size = match level {
                1 => 17.0,
                2 => 15.0,
                _ => Type::BODY + 0.5,
            };
            div()
                .pt(px(if *level <= 2 { 4.0 } else { 2.0 }))
                .child(render_text(key, inline, t, size, FontWeight::SEMIBOLD))
                .into_any_element()
        }
        Block::Math(tex, fenced) => {
            let (tex, fenced) = (tex.clone(), *fenced);
            formula(
                tex.clone(),
                true,
                Type::BODY,
                Type::BODY * 1.5,
                t.primary,
                move || {
                    if fenced {
                        code_block(&tex, t)
                    } else {
                        render_inline(
                            key,
                            &Inline {
                                text: format!("$${tex}$$"),
                                ..Inline::default()
                            },
                            t,
                            Type::BODY,
                            FontWeight::NORMAL,
                        )
                        .into_any_element()
                    }
                },
            )
            .into_any_element()
        }
        Block::Code(code) => code_block(code, t),
        Block::List { start, items } => div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .children(items.iter().enumerate().map(|(i, item)| {
                let marker = match start {
                    Some(s) => format!("{}.", s + i as u64),
                    None => if depth.is_multiple_of(2) {
                        "•"
                    } else {
                        "◦"
                    }
                    .to_string(),
                };
                div()
                    .flex()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex_none()
                            .min_w(px(if start.is_some() { 18.0 } else { 10.0 }))
                            .text_size(px(Type::BODY))
                            .line_height(px(20.0))
                            .text_color(t.tertiary)
                            .child(marker),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .children(item.iter().map(|b| render_block(id, n, b, t, depth + 1))),
                    )
            }))
            .into_any_element(),
        Block::Quote(blocks) => div()
            .pl(px(12.0))
            .border_l_2()
            .border_color(t.primary.alpha(0.15))
            .text_color(t.secondary)
            .flex()
            .flex_col()
            .gap(px(6.0))
            .children(blocks.iter().map(|b| render_block(id, n, b, t, depth)))
            .into_any_element(),
        Block::Rule => div()
            .my(px(4.0))
            .h(px(1.0))
            .w_full()
            .bg(t.hairline)
            .into_any_element(),
        Block::Table(rows) => div()
            .rounded(px(Radius::ROW))
            .border_1()
            .border_color(t.card_stroke)
            .overflow_hidden()
            .flex()
            .flex_col()
            .children(rows.iter().enumerate().map(|(r, row)| {
                div()
                    .flex()
                    .when(r > 0, |d| d.border_t_1().border_color(t.hairline))
                    .when(r == 0, |d| d.bg(t.primary.alpha(0.04)))
                    .children(row.iter().enumerate().map(|(c, cell)| {
                        *n += 1;
                        div()
                            .flex_1()
                            .min_w_0()
                            .px(px(10.0))
                            .py(px(6.0))
                            .when(c > 0, |d| d.border_l_1().border_color(t.hairline))
                            .child(render_text(
                                ElementId::Name(format!("{id:?}-{n}").into()),
                                cell,
                                t,
                                12.5,
                                if r == 0 {
                                    FontWeight::SEMIBOLD
                                } else {
                                    FontWeight::NORMAL
                                },
                            ))
                    }))
            }))
            .into_any_element(),
    }
}

fn code_block(code: &str, t: Theme) -> AnyElement {
    div()
        .w_full()
        .px(px(10.0))
        .py(px(8.0))
        .rounded(px(Radius::ROW))
        .bg(t.primary.alpha(if t.dark { 0.05 } else { 0.045 }))
        .border_1()
        .border_color(t.card_stroke)
        .font_family(crate::mono_font())
        .text_size(px(12.0))
        .line_height(px(17.0))
        .text_color(t.primary.alpha(0.9))
        .child(code.to_string())
        .into_any_element()
}

/// A stretch of text, with its formulas typeset when it has any.
fn render_text(
    key: ElementId,
    inline: &Inline,
    t: Theme,
    size: f32,
    weight: FontWeight,
) -> AnyElement {
    if inline.math.is_empty() {
        render_inline(key, inline, t, size, weight).into_any_element()
    } else {
        render_flow(key, inline, t, size, weight)
    }
}

/// A piece of a line of text with formulas in it.
#[derive(Debug, PartialEq)]
enum Piece {
    /// A word and the spaces after it, or spaces alone where a line starts with them.
    Words(Range<usize>),
    /// The end of a line the text breaks.
    Break,
}

/// The pieces of `range` of `text`: each word with the spaces after it, and its line breaks.
fn pieces(text: &str, range: Range<usize>) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut start = range.start;
    let mut spaced = false;
    for (i, c) in text[range.clone()].char_indices() {
        let at = range.start + i;
        if c == '\n' {
            if start < at {
                out.push(Piece::Words(start..at));
            }
            out.push(Piece::Break);
            start = at + 1;
            spaced = false;
        } else if c.is_whitespace() {
            spaced = true;
        } else if spaced {
            if start < at {
                out.push(Piece::Words(start..at));
            }
            start = at;
            spaced = false;
        }
    }
    if start < range.end {
        out.push(Piece::Words(start..range.end));
    }
    out
}

/// Text with formulas in it: its words and its formulas side by side, wrapping where its words
/// do, each formula on the words' baseline. Where the text is not to wrap (a line cut short),
/// they stay on one line, cut at its end.
fn render_flow(
    key: ElementId,
    inline: &Inline,
    t: Theme,
    size: f32,
    weight: FontWeight,
) -> AnyElement {
    let mut items: Vec<AnyElement> = Vec::new();
    let mut n = 0usize;
    let mut words = |range: Range<usize>, items: &mut Vec<AnyElement>| {
        for piece in pieces(&inline.text, range) {
            items.push(match piece {
                Piece::Words(r) => {
                    n += 1;
                    let key = ElementId::Name(format!("{key:?}-{n}").into());
                    render_inline(key, &inline.slice(r), t, size, weight).into_any_element()
                }
                // A piece as wide as the line ends it.
                Piece::Break => div().w_full().h_0().into_any_element(),
            });
        }
    };
    let mut at = 0;
    for math in &inline.math {
        words(at..math.range.start, &mut items);
        let written = inline.slice(math.range.clone());
        let written_key = ElementId::Name(format!("{key:?}-math-{}", math.range.start).into());
        items.push(
            formula(
                math.tex.clone(),
                false,
                size,
                size * 1.5,
                t.primary,
                move || render_inline(written_key, &written, t, size, weight).into_any_element(),
            )
            .into_any_element(),
        );
        at = math.range.end;
    }
    words(at..inline.text.len(), &mut items);
    lazily(move |window, _| {
        let wraps = window.text_style().white_space != WhiteSpace::Nowrap;
        div()
            .flex()
            .items_baseline()
            .when(wraps, |row| row.flex_wrap())
            .when(!wraps, |row| row.overflow_hidden())
            .children(items)
            .into_any_element()
    })
    .into_any_element()
}

fn render_inline(
    key: ElementId,
    inline: &Inline,
    t: Theme,
    size: f32,
    weight: FontWeight,
) -> impl IntoElement {
    let text: SharedString = inline.text.clone().into();
    let highlights: Vec<(Range<usize>, HighlightStyle)> = inline
        .runs
        .iter()
        .map(|(r, m)| {
            let mut h = HighlightStyle::default();
            if m.bold {
                h.font_weight = Some(FontWeight::SEMIBOLD);
            }
            if m.italic {
                h.font_style = Some(FontStyle::Italic);
            }
            if m.strike {
                h.strikethrough = Some(StrikethroughStyle {
                    thickness: px(1.0),
                    color: None,
                });
            }
            if m.code {
                h.background_color = Some(t.primary.alpha(if t.dark { 0.07 } else { 0.06 }).into());
            }
            if m.link {
                h.color = Some(t.accent.into());
                h.underline = Some(UnderlineStyle {
                    thickness: px(1.0),
                    color: Some(t.accent.alpha(0.45).into()),
                    wavy: false,
                });
            }
            // A blank is sunken into the page: a little darker than code's ground.
            let sunken = t.primary.alpha(if t.dark { 0.12 } else { 0.08 });
            match m.cloze {
                // The text's own ink, so it reads on either theme; the accent is the line
                // under it (the one that shows on this surface).
                Some(ClozePart::Words) => {
                    h.font_weight = Some(FontWeight::SEMIBOLD);
                    h.underline = Some(UnderlineStyle {
                        thickness: px(1.5),
                        color: Some(t.focus.into()),
                        wavy: false,
                    });
                }
                Some(ClozePart::Blank) => {
                    // Faded out entirely: a colour given here is blended over the text's own
                    // ink, so even a transparent one would leave the words showing.
                    h.color = None;
                    h.fade_out = Some(1.0);
                    h.background_color = Some(sunken.into());
                    h.underline = None;
                    h.strikethrough = None;
                }
                Some(ClozePart::Mark) => {
                    h.color = Some(t.tertiary.into());
                    h.background_color = Some(sunken.into());
                    h.underline = None;
                    h.strikethrough = None;
                }
                None => {}
            }
            (r.clone(), h)
        })
        .collect();
    let mono: Vec<(Range<usize>, SharedString)> = inline
        .runs
        .iter()
        .filter(|(_, m)| m.code)
        .map(|(r, _)| (r.clone(), crate::mono_font()))
        .collect();
    let styled = StyledText::new(text)
        .with_highlights(highlights)
        .with_font_family_overrides(mono);
    let urls: Vec<String> = inline.links.iter().map(|(_, u)| u.clone()).collect();
    let ranges: Vec<Range<usize>> = inline.links.iter().map(|(r, _)| r.clone()).collect();
    let body = InteractiveText::new(key, styled).on_click(
        ranges,
        move |i, _: &mut Window, cx: &mut App| {
            if let Some(u) = urls.get(i) {
                if u.starts_with("http://") || u.starts_with("https://") {
                    cx.open_url(u);
                }
            }
        },
    );
    div()
        .text_size(px(size))
        .line_height(px(size * 1.5))
        .font_weight(weight)
        .text_color(t.primary.alpha(0.92))
        .child(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_plain(source: &str) -> Vec<Block> {
        parse(source, None)
    }

    /// The one paragraph of `source`, read as a card's text.
    fn card(source: &str, cloze: Cloze) -> Inline {
        match parse(source, Some(cloze)).into_iter().next() {
            Some(Block::Paragraph(inline)) => inline,
            _ => panic!("a paragraph"),
        }
    }

    /// The stretches of `inline` that are `part` of a cloze.
    fn parts(inline: &Inline, part: ClozePart) -> Vec<&str> {
        inline
            .runs
            .iter()
            .filter(|(_, m)| m.cloze == Some(part))
            .map(|(r, _)| &inline.text[r.clone()])
            .collect()
    }

    #[test]
    fn a_cloze_is_two_braces_around_words_on_one_line() {
        let words = |text: &'static str| -> Vec<&'static str> {
            cloze_spans(text).into_iter().map(|r| &text[r]).collect()
        };
        assert_eq!(
            words("The {{mitochondria}} of {{a cell}}."),
            ["mitochondria", "a cell"]
        );
        assert!(words("{{}} {one} {{two} {{ three\nfour }}").is_empty());
        // The earliest opening braces count, and a brace may be among the words.
        assert_eq!(words("{{{a}}}"), ["{a"]);
        assert_eq!(words("{{a}b}} {{c}}"), ["c"]);
        assert_eq!(words("{{ spaced out }}"), [" spaced out "]);
    }

    #[test]
    fn a_shown_cloze_loses_its_braces_and_stands_out() {
        let shown = card(
            "The {{mitochondria}} powers **the {{cell}}**.",
            Cloze::Shown,
        );
        assert_eq!(shown.text, "The mitochondria powers the cell.");
        assert_eq!(parts(&shown, ClozePart::Words), ["mitochondria", "cell"]);
        // The emphasis around a cloze stays on it.
        assert!(shown
            .runs
            .iter()
            .any(|(r, m)| m.bold && m.cloze.is_some() && &shown.text[r.clone()] == "cell"));
    }

    #[test]
    fn a_hidden_cloze_is_a_blank_that_ends_in_its_mark() {
        let hidden = card("The {{mitochondria}} powers the cell.", Cloze::Hidden);
        // The mark stands in for the last three letters: the blank is about as wide.
        assert_eq!(hidden.text, "The mitochond[…] powers the cell.");
        assert_eq!(parts(&hidden, ClozePart::Blank), ["mitochond"]);
        assert_eq!(parts(&hidden, ClozePart::Mark), ["[…]"]);
        // Every other word is drawn as it is.
        let plain: Vec<&(Range<usize>, Marks)> = hidden
            .runs
            .iter()
            .filter(|(_, m)| m.cloze.is_none())
            .collect();
        assert!(plain.is_empty());
    }

    #[test]
    fn a_short_cloze_is_only_its_mark() {
        for source in ["2 + 2 = {{4}}", "2 + 2 = {{four}}"] {
            let hidden = card(source, Cloze::Hidden);
            assert!(parts(&hidden, ClozePart::Blank)
                .iter()
                .all(|b| b.len() <= 1));
            assert!(!hidden.text.contains('4') || source.contains("four"));
        }
        assert_eq!(card("2 + 2 = {{4}}", Cloze::Hidden).text, "2 + 2 = […]");
        assert_eq!(card("It is {{DNA}}.", Cloze::Hidden).text, "It is […].");
        // Characters are whole ones: an accent written apart stays with its letter.
        assert_eq!(kept_of_blank("ae\u{301}iou"), "ae\u{301}");
    }

    #[test]
    fn a_cloze_is_found_in_text_the_parser_hands_over_in_pieces() {
        let shown = card("An {{array [0] of_ints}} here", Cloze::Shown);
        assert_eq!(parts(&shown, ClozePart::Words), ["array [0] of_ints"]);
        assert_eq!(shown.text, "An array [0] of_ints here");
    }

    #[test]
    fn braces_in_code_math_and_a_links_words_are_not_a_cloze() {
        for source in [
            "Write `{{name}}` in the template.",
            "So $x^{{2}}$ grows.",
            "See [{{this}}](https://example.com).",
        ] {
            let read = card(source, Cloze::Hidden);
            assert!(read.runs.iter().all(|(_, m)| m.cloze.is_none()), "{source}");
            assert!(!read.text.contains(BLANK_MARK), "{source}");
        }
        assert_eq!(
            card("So $x^{{2}}$ grows.", Cloze::Shown).text,
            "So $x^{{2}}$ grows."
        );
        // Emphasis inside a link is text of its own, as in the earlier app.
        let inside = card("See [*{{this}}*](https://example.com).", Cloze::Shown);
        assert_eq!(parts(&inside, ClozePart::Words), ["this"]);
    }

    #[test]
    fn a_cloze_does_not_reach_across_emphasis_or_a_line_break() {
        for source in ["{{the *cell* wall}}", "{{the cell\nwall}}"] {
            let read = card(source, Cloze::Hidden);
            assert!(read.runs.iter().all(|(_, m)| m.cloze.is_none()), "{source}");
        }
    }

    #[test]
    fn text_that_is_not_a_cards_keeps_its_braces() {
        let Some(Block::Paragraph(p)) = parse_plain("A {{template}} here.").into_iter().next()
        else {
            panic!("a paragraph")
        };
        assert_eq!(p.text, "A {{template}} here.");
        assert!(p.runs.is_empty());
    }

    /// The formulas of the blocks of `source`, by block: (TeX, written `$$…$$`), and a block of
    /// math as (TeX, true) alone.
    fn formulas(source: &str) -> Vec<Vec<(String, bool)>> {
        parse_plain(source)
            .into_iter()
            .map(|block| match block {
                Block::Paragraph(p) => p.math.into_iter().map(|m| (m.tex, m.display)).collect(),
                Block::Math(tex, _) => vec![(tex, true)],
                _ => Vec::new(),
            })
            .collect()
    }

    fn math(tex: &str, display: bool) -> (String, bool) {
        (tex.to_string(), display)
    }

    #[test]
    fn math_in_a_line_is_found_and_keeps_its_written_form() {
        let Some(Block::Paragraph(p)) = parse_plain(r"The area is $\pi r^2$ here.").pop() else {
            panic!("a paragraph")
        };
        assert_eq!(p.text, r"The area is $\pi r^2$ here.");
        assert_eq!(p.math.len(), 1);
        assert_eq!(&p.text[p.math[0].range.clone()], r"$\pi r^2$");
        assert_eq!(p.math[0].tex, r"\pi r^2");
        assert!(!p.math[0].display);
    }

    #[test]
    fn display_math_on_lines_of_its_own_is_a_block_and_in_a_sentence_is_not() {
        assert_eq!(
            formulas("Before\n\n$$\nE = mc^2\n$$\n\nAfter"),
            [vec![], vec![math("\nE = mc^2\n", true)], vec![]]
        );
        // On its own line between lines of text, it parts them.
        let blocks = parse_plain("So:\n$$x^2$$\nand more.");
        assert!(matches!(&blocks[0], Block::Paragraph(p) if p.text == "So:"));
        assert!(matches!(&blocks[1], Block::Math(tex, false) if tex == "x^2"));
        assert!(matches!(&blocks[2], Block::Paragraph(p) if p.text == "and more."));
        // Inside a sentence, it stays in the line.
        assert_eq!(formulas("We have $$x^2$$ here."), [vec![math("x^2", true)]]);
        // A fence of math is a block of math.
        let blocks = parse_plain("```math\n\\frac{a}{b}\n```");
        assert!(matches!(&blocks[0], Block::Math(tex, true) if tex == "\\frac{a}{b}"));
    }

    #[test]
    fn a_lone_dollar_code_and_escaped_dollars_stay_text() {
        for source in [
            "It costs $5.",
            "It costs $5 and $10 is more.",
            r"Write \$x\$ for a dollar.",
            "Write `$x$` in code.",
            "```\n$x$\n```",
            "A $ x $ with spaces.",
        ] {
            assert!(formulas(source).iter().all(Vec::is_empty), "{source}");
        }
        assert_eq!(
            formulas("It costs $5. The formula $x$ holds."),
            [vec![math("x", false)]]
        );
    }

    #[test]
    fn math_written_with_backslashes_is_read_as_dollars_outside_code() {
        assert_eq!(
            normalize_math(r"So \( x^2 \) and \[y\]."),
            r"So $x^2$ and $$y$$."
        );
        assert_eq!(
            normalize_math(r"Code `\(x\)` stays."),
            r"Code `\(x\)` stays."
        );
        assert_eq!(normalize_math(r"No math \(here."), r"No math \(here.");
        assert_eq!(
            formulas("Lines:\n\\[\na+b\n\\]\nend"),
            [vec![], vec![math("\na+b\n", true)], vec![]]
        );
    }

    #[test]
    fn a_cards_cloze_and_its_math_stay_apart() {
        let shown = card(r"The {{area}} is $\pi r^{2}$.", Cloze::Shown);
        assert_eq!(parts(&shown, ClozePart::Words), ["area"]);
        assert_eq!(shown.math.len(), 1);
        assert_eq!(&shown.text[shown.math[0].range.clone()], r"$\pi r^{2}$");
        // Braces inside math are never a cloze, and the braces around math are not either.
        for source in [r"So $\frac{{a}}{b}$ holds.", "So {{$x$}} holds."] {
            let read = card(source, Cloze::Hidden);
            assert!(read.runs.iter().all(|(_, m)| m.cloze.is_none()), "{source}");
            assert_eq!(read.math.len(), 1, "{source}");
        }
    }

    #[test]
    fn a_line_with_math_is_cut_into_words_and_line_breaks() {
        let text = " is a bc  d\nnext";
        let words: Vec<_> = pieces(text, 0..text.len())
            .into_iter()
            .map(|p| match p {
                Piece::Words(r) => &text[r],
                Piece::Break => "|",
            })
            .collect();
        assert_eq!(words, [" ", "is ", "a ", "bc  ", "d", "|", "next"]);
    }

    #[test]
    fn blocks_and_marks_come_out_of_markdown() {
        let b = parse_plain(
            "# Title\n\nSome **bold** and `code`, a [link](https://x.y).\n\n- \
                one\n- two\n  1. deep\n\n```\nfn a() {}\n```\n\n> quoted\n\n| a | b \
                |\n|---|---|\n| 1 | 2 |\n",
        );
        assert!(matches!(b[0], Block::Heading(1, _)));
        let Block::Paragraph(p) = &b[1] else {
            panic!("a paragraph")
        };
        assert_eq!(p.text, "Some bold and code, a link.");
        assert!(p
            .runs
            .iter()
            .any(|(r, m)| m.bold && &p.text[r.clone()] == "bold"));
        assert!(p
            .runs
            .iter()
            .any(|(r, m)| m.code && &p.text[r.clone()] == "code"));
        assert_eq!(p.links[0].1, "https://x.y");
        let Block::List { items, start: None } = &b[2] else {
            panic!("a list")
        };
        assert_eq!(items.len(), 2);
        assert!(items[1]
            .iter()
            .any(|b| matches!(b, Block::List { start: Some(1), .. })));
        assert!(matches!(&b[3], Block::Code(c) if c == "fn a() {}"));
        assert!(matches!(b[4], Block::Quote(_)));
        assert!(matches!(&b[5], Block::Table(rows) if rows.len() == 2 && rows[1][1].text == "2"));
    }
}
