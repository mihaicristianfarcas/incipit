//! Markdown's blocks drawn with the theme: each block as its element, the text of a block as
//! runs of styled text and typeset formulas laid out in a flow that wraps.

use super::*;

pub(super) fn draw(id: ElementId, source: &str, t: Theme, cloze: Option<Cloze>) -> AnyElement {
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
    let backward = crate::direction::right_to_left(&inline.text);
    if inline.math.is_empty() && !backward && !breaks_badly(&inline.text) {
        render_inline(key, inline, t, size, weight).into_any_element()
    } else {
        render_flow(key, inline, t, size, weight, backward)
    }
}

/// The marks that end a sentence or a clause: after words written left to right in a paragraph set
/// from the right they take the paragraph's way. Brackets and quotes are not among them: a pair
/// keeps the way of what it encloses.
const SENTENCE_MARKS: &[char] = &['.', ',', ';', ':', '!', '?', '…'];

/// About how wide a space is, as a share of the text's size: the room between the words of a
/// paragraph set from the right, whose pieces lose their spaces (`backward_pieces`).
const SPACE: f32 = 0.27;

/// The pieces of a paragraph that runs right to left, as they are set from the right: each word
/// without the spaces after it (the flow keeps the words apart), and the words written left to
/// right inside it (a name, a phrase in English, a number), with what stands between them, kept
/// together as one piece, so they read in their own order.
pub(super) fn backward_pieces(text: &str, pieces: Vec<Piece>) -> Vec<Piece> {
    let forward = |range: &Range<usize>| {
        let words = &text[range.clone()];
        !words.chars().any(crate::direction::is_right_to_left)
            && words.chars().any(char::is_alphanumeric)
    };
    let mut out: Vec<Piece> = Vec::new();
    // The piece being gathered, and whether it is of words written left to right.
    let mut open: Option<(Range<usize>, bool)> = None;
    for piece in pieces {
        let Piece::Words(range) = piece else {
            out.extend(open.take().map(|(range, _)| Piece::Words(range)));
            out.push(Piece::Break);
            continue;
        };
        let ahead = forward(&range);
        match &mut open {
            Some((gathered, true)) if ahead => gathered.end = range.end,
            _ => {
                out.extend(open.take().map(|(range, _)| Piece::Words(range)));
                open = Some((range, ahead));
            }
        }
    }
    out.extend(open.map(|(range, _)| Piece::Words(range)));
    out.into_iter()
        .filter_map(|piece| {
            let Piece::Words(range) = piece else {
                return Some(piece);
            };
            let end = range.start + text[range.clone()].trim_end().len();
            if end == range.start {
                return None;
            }
            if !forward(&range) {
                return Some(Piece::Words(range.start..end));
            }
            let words = text[range.start..end].trim_end_matches(SENTENCE_MARKS);
            let turn = range.start + words.len();
            Some(if turn == end || turn == range.start {
                Piece::Words(range.start..end)
            } else {
                Piece::Turned {
                    words: range.start..turn,
                    marks: turn..end,
                }
            })
        })
        .collect()
}

/// Whether GPUI's own wrapping would cut `text` where it should not. It may break a line before
/// any character outside its list of word characters (ASCII, Latin, Cyrillic and a few more), so
/// before a prime or an index (`M′`, `x₀`), a combining accent, and inside every word of Greek or
/// of an Indic script. Such text is set as a flow of whole words, as text with formulas is
/// (`render_flow`). A paragraph that runs left to right with words set right to left inside it
/// (a Hebrew name in English) keeps GPUI's lines: a flow would set those words in the order they
/// are written, left to right.
pub(super) fn breaks_badly(text: &str) -> bool {
    let mut bad = false;
    for c in text.chars() {
        if crate::direction::is_right_to_left(c) {
            return false;
        }
        bad |= matches!(c,
            // Greek, and Greek with its accents
            '\u{0370}'..='\u{03FF}' | '\u{1F00}'..='\u{1FFF}'
            // Devanagari, Bengali and the other Indic scripts, Sinhala
            | '\u{0900}'..='\u{0DFF}'
            // Armenian, Georgian
            | '\u{0530}'..='\u{058F}' | '\u{10A0}'..='\u{10FF}'
            // Combining accents GPUI does not keep with their letter
            | '\u{1AB0}'..='\u{1AFF}' | '\u{1DC0}'..='\u{1DFF}' | '\u{20D0}'..='\u{20FF}'
            | '\u{FE20}'..='\u{FE2F}'
            // Primes, superscripts and subscripts
            | '\u{2032}'..='\u{2037}' | '\u{2057}' | '\u{2070}'..='\u{209F}');
    }
    bad
}

/// A piece of a line of text with formulas in it.
#[derive(Debug, PartialEq)]
pub(super) enum Piece {
    /// A word and the spaces after it, or spaces alone where a line starts with them.
    Words(Range<usize>),
    /// In a paragraph set from the right, words written left to right and the sentence marks
    /// after them, which stand before them there, on their left (`,Albert Einstein`), as the
    /// Unicode bidirectional algorithm sets them.
    Turned {
        words: Range<usize>,
        marks: Range<usize>,
    },
    /// The end of a line the text breaks.
    Break,
}

/// The pieces of `range` of `text`: each word with the spaces after it, and its line breaks.
pub(super) fn pieces(text: &str, range: Range<usize>) -> Vec<Piece> {
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

/// Text with formulas in it, or that GPUI would break badly: its words and its formulas side by
/// side, wrapping where its words do, each formula on the words' baseline. Where the text is not
/// to wrap (a line cut short), they stay on one line, cut at its end. A paragraph that runs right
/// to left (`backward`) is set from the right, its words apart by a space's width.
fn render_flow(
    key: ElementId,
    inline: &Inline,
    t: Theme,
    size: f32,
    weight: FontWeight,
    backward: bool,
) -> AnyElement {
    let mut items: Vec<AnyElement> = Vec::new();
    let mut n = 0usize;
    let mut words = |range: Range<usize>, items: &mut Vec<AnyElement>| {
        let mut pieces = pieces(&inline.text, range);
        if backward {
            pieces = backward_pieces(&inline.text, pieces);
        }
        for piece in pieces {
            items.push(match piece {
                Piece::Words(r) => {
                    n += 1;
                    let key = ElementId::Name(format!("{key:?}-{n}").into());
                    render_inline(key, &inline.slice(r), t, size, weight).into_any_element()
                }
                Piece::Turned { words, marks } => {
                    n += 1;
                    let key = ElementId::Name(format!("{key:?}-{n}").into());
                    let mut turned = inline.slice(marks);
                    turned.append(inline.slice(words));
                    render_inline(key, &turned, t, size, weight).into_any_element()
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
            .when(backward, |row| {
                row.flex_row_reverse().gap_x(px(size * SPACE))
            })
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
    let on_link = ON_LINK.with_borrow(Clone::clone);
    let body = InteractiveText::new(key, styled).on_click(
        ranges,
        move |i, window: &mut Window, cx: &mut App| {
            if let Some(u) = urls.get(i) {
                if u.starts_with("http://") || u.starts_with("https://") {
                    cx.open_url(u);
                } else if let Some(on_link) = &on_link {
                    on_link(u, window, cx);
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
