//! Paragraphs set right to left (Hebrew, Arabic; `incipit_ui::right_to_left` says which), set
//! flush right. GPUI falls short of such text twice: it draws right-aligned rows at the right
//! edge but finds the letter under the mouse as if each row began at the left, and it paints a
//! run's background by walking a row's glyphs left to right while it walks the runs in the text's
//! order, so a row set right to left gets none. The reader moves the place under the mouse back
//! by what its row was moved (`unaligned`), and paints the marks of such a paragraph itself,
//! behind the text, from where each glyph is drawn (`paint_marks`).

use gpui::{fill, point, px, size, Bounds, Hsla, Pixels, Point, TextLayout, Window};

/// A row of flush-right text as it is drawn.
struct Row {
    top: Pixels,
    /// How far right of the text's left edge the row is drawn: the text's width less its own.
    shift: Pixels,
    /// Where the row starts on its line as shaped, which is drawn at `shift`.
    start: Pixels,
    /// Each glyph's offset in the whole text, where it starts on its line as shaped, and how
    /// wide it is, left to right.
    glyphs: Vec<(usize, Pixels, Pixels)>,
}

/// The rows of the text `layout` laid out, flush right.
fn rows(layout: &TextLayout) -> Vec<Row> {
    let bounds = layout.bounds();
    let line_height = layout.line_height();
    let mut rows = Vec::new();
    let mut top = bounds.top();
    let mut line_start = 0;
    for line in layout.line_layouts() {
        let shaped = &line.unwrapped_layout;
        let glyphs: Vec<(usize, Pixels)> = shaped
            .runs
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|g| (g.index, g.position.x)))
            .collect();
        // Where each row ends among the glyphs, and on the line.
        let mut ends: Vec<(usize, Pixels)> = line
            .wrap_boundaries
            .iter()
            .map(|b| {
                let at = shaped.runs[..b.run_ix]
                    .iter()
                    .map(|run| run.glyphs.len())
                    .sum::<usize>()
                    + b.glyph_ix;
                (at, shaped.runs[b.run_ix].glyphs[b.glyph_ix].position.x)
            })
            .collect();
        ends.push((glyphs.len(), shaped.width));
        let (mut first, mut start) = (0, Pixels::ZERO);
        for (last, end) in ends {
            let row = (first..last).map(|i| {
                let (at, x) = glyphs[i];
                let next = glyphs
                    .get(i + 1)
                    .filter(|_| i + 1 < last)
                    .map_or(end, |g| g.1);
                (line_start + at, x, next - x)
            });
            rows.push(Row {
                top,
                shift: bounds.size.width - (end - start),
                start,
                glyphs: row.collect(),
            });
            top += line_height;
            (first, start) = (last, end);
        }
        line_start += shaped.len + 1;
    }
    rows
}

/// `position` over flush-right text, moved left by as much as the row under it was moved right:
/// where GPUI's `index_for_position`, which reads every row from the left, finds the letter drawn
/// there.
pub(super) fn unaligned(layout: &TextLayout, position: Point<Pixels>) -> Point<Pixels> {
    let line_height = layout.line_height();
    let rows = rows(layout);
    let row = rows
        .iter()
        .find(|row| position.y < row.top + line_height)
        .or(rows.last());
    match row {
        Some(row) => point(position.x - row.shift, position.y),
        None => position,
    }
}

/// Paints the marks on flush-right text behind it: each stretch of bytes in its colour, the last
/// mark over a glyph winning, as the runs of text set left to right choose.
pub(super) fn paint_marks(
    layout: &TextLayout,
    marks: &[(usize, usize, Hsla)],
    window: &mut Window,
) {
    if marks.is_empty() {
        return;
    }
    let left = layout.bounds().left();
    let line_height = layout.line_height();
    for row in rows(layout) {
        let drawn = |x: Pixels| left + row.shift + (x - row.start);
        let mut span: Option<(Pixels, Pixels, Hsla)> = None;
        let paint = |(from, to, ink): (Pixels, Pixels, Hsla), window: &mut Window| {
            let bounds = Bounds::new(point(from, row.top), size(to - from, line_height));
            window.paint_quad(fill(bounds, ink));
        };
        for &(at, x, width) in &row.glyphs {
            let ink = marks
                .iter()
                .rev()
                .find(|m| m.0 <= at && at < m.1)
                .map(|m| m.2);
            let (from, to) = (drawn(x), drawn(x + width));
            match (&mut span, ink) {
                // A glyph next to the stretch before it, in its colour, lengthens it.
                (Some((_, end, same)), Some(ink))
                    if *same == ink && (from - *end).abs() < px(0.5) =>
                {
                    *end = to;
                }
                _ => {
                    if let Some(done) = span.take() {
                        paint(done, window);
                    }
                    span = ink.map(|ink| (from, to, ink));
                }
            }
        }
        if let Some(done) = span {
            paint(done, window);
        }
    }
}
