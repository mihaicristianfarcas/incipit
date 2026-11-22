//! Markdown, as models write it, drawn with the theme: paragraphs,
//! headings, emphasis, inline code and code blocks, lists (nested, numbered,
//! tasks), block quotes, links (opened in the browser), rules, tables and
//! math. A page that draws Markdown whose links lead somewhere of its own (a
//! place in a book) gives a handler for them (`markdown_linked`); a link to
//! the web is opened in the browser either way, and nothing else is.
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
//! braces in code, in math or in the words of a link are left alone, and a
//! cloze does not reach across emphasis or a line break. Its blank is the
//! earlier app's too, as near as text allows: a sunken stretch with `[…]` at
//! its end, about as wide as the words it hides (the mark stands in for their
//! last three characters, and for all of a shorter cloze, where the earlier
//! app let the mark run over the text before). The hidden words that keep the
//! width are in the text and are not drawn.
//!
//! Here the Markdown is read into blocks and runs of marked text; `draw`
//! draws them.

use std::borrow::Cow;
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    div, prelude::*, px, AnyElement, App, ElementId, FontStyle, FontWeight, HighlightStyle,
    InteractiveText, SharedString, StrikethroughStyle, StyledText, UnderlineStyle, WhiteSpace,
    Window,
};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use unicode_segmentation::UnicodeSegmentation;

use crate::formula::{formula, lazily};
use crate::theme::{Radius, Theme, Type};

mod draw;
#[cfg(test)]
mod tests;

use draw::draw;

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

/// What a press on a link that is not to the web does, in the Markdown being drawn.
type OnLink = Rc<dyn Fn(&str, &mut Window, &mut App)>;

thread_local! {
    /// The handler of the Markdown being drawn now (`markdown_linked`): set only while it is
    /// drawn, so each paragraph's links take the handler of the Markdown they are in.
    static ON_LINK: RefCell<Option<OnLink>> = const { RefCell::new(None) };
}

/// Draw Markdown whose links that are not to the web (`place:12`) go to `on_link` with their
/// address when pressed.
pub fn markdown_linked(
    id: impl Into<ElementId>,
    source: &str,
    t: Theme,
    on_link: impl Fn(&str, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let before = ON_LINK.replace(Some(Rc::new(on_link)));
    let drawn = draw(id.into(), source, t, None);
    ON_LINK.set(before);
    drawn
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
