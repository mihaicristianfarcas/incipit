//! Markdown, as models write it, drawn with the theme: paragraphs,
//! headings, emphasis, inline code and code blocks, lists (nested, numbered,
//! tasks), block quotes, links (opened in the browser), rules and tables.
//!
//! A flashcard's text is drawn with its cloze (`markdown_cloze`): `{{the
//! hidden words}}` stand out once the answer is shown and are a blank until
//! then. What counts as a cloze is the earlier app's rule: two opening
//! braces, one or more characters that are neither a closing brace nor a
//! line break, two closing braces, all in one stretch of plain text. So
//! braces in code, in math (`$…$`, which stays as written) or in the words of
//! a link are left alone, and a cloze does not reach across emphasis or a
//! line break. Its blank is the earlier app's too, as near as text allows: a
//! sunken stretch with `[…]` at its end, about as wide as the words it hides
//! (the mark stands in for their last three characters, and for all of a
//! shorter cloze, where the earlier app let the mark run over the text
//! before). The hidden words that keep the width are in the text and are not
//! drawn.

use std::ops::Range;

use gpui::{
    div, prelude::*, px, AnyElement, App, ElementId, FontStyle, FontWeight, HighlightStyle,
    InteractiveText, SharedString, StrikethroughStyle, StyledText, UnderlineStyle, Window,
};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use unicode_segmentation::UnicodeSegmentation;

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
}

impl Inline {
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

/// Parse `source` into blocks. With `cloze`, `{{…}}` in plain text is a cloze, drawn that way.
fn parse(source: &str, cloze: Option<Cloze>) -> Vec<Block> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS);
    if cloze.is_some() {
        // Math is told apart from text, so that braces in it are never a cloze.
        opts.insert(Options::ENABLE_MATH);
    }
    let events: Vec<Event> = Parser::new_ext(source, opts).collect();
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
            out.push(Block::Paragraph(std::mem::take(loose)));
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
                out.push(Block::Paragraph(parse_inline(
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
                let _ = matches!(kind, CodeBlockKind::Fenced(_));
                out.push(Block::Code(code.trim_end_matches('\n').to_string()));
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
                let mut inline = parse_inline_until_block(events, pos, cloze);
                if loose.text.is_empty() {
                    loose = inline;
                } else {
                    let shift = loose.text.len();
                    loose.text.push_str(&inline.text);
                    loose.runs.extend(
                        inline
                            .runs
                            .drain(..)
                            .map(|(r, m)| (r.start + shift..r.end + shift, m)),
                    );
                    loose.links.extend(
                        inline
                            .links
                            .drain(..)
                            .map(|(r, u)| (r.start + shift..r.end + shift, u)),
                    );
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
            // Math is not typeset here: it stays as it was written.
            Event::InlineMath(t) => self.inline.push(&format!("${t}$"), self.marks),
            Event::DisplayMath(t) => self.inline.push(&format!("$${t}$$"), self.marks),
            Event::SoftBreak => self.inline.push(" ", self.marks),
            Event::HardBreak => self.inline.push("\n", self.marks),
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
        Block::Paragraph(inline) => {
            render_inline(key, inline, t, Type::BODY, FontWeight::NORMAL).into_any_element()
        }
        Block::Heading(level, inline) => {
            let size = match level {
                1 => 17.0,
                2 => 15.0,
                _ => Type::BODY + 0.5,
            };
            div()
                .pt(px(if *level <= 2 { 4.0 } else { 2.0 }))
                .child(render_inline(key, inline, t, size, FontWeight::SEMIBOLD))
                .into_any_element()
        }
        Block::Code(code) => div()
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
            .child(code.clone())
            .into_any_element(),
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
                            .child(render_inline(
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
                    h.color = Some(gpui::transparent_black());
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
