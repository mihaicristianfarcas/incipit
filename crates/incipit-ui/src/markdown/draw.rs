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
    if inline.math.is_empty() {
        render_inline(key, inline, t, size, weight).into_any_element()
    } else {
        render_flow(key, inline, t, size, weight)
    }
}

/// A piece of a line of text with formulas in it.
#[derive(Debug, PartialEq)]
pub(super) enum Piece {
    /// A word and the spaces after it, or spaces alone where a line starts with them.
    Words(Range<usize>),
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
