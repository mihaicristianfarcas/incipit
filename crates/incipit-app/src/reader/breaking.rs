//! Where the lines of a book's paragraph break. GPUI's wrapping may break a line before any
//! character outside its own list of word characters, so a prime, an index or a Greek, Hebrew,
//! Arabic or Devanagari letter can start a line ("Let M" at a line's end, "′ be" on the next),
//! and a word in those scripts can be cut in two; text set right to left (Hebrew, Arabic) has
//! its lines cut where its glyphs fall on the screen, not where its words end, and they come in
//! the wrong order. The reader chooses a paragraph's line ends itself, at spaces, and shows each
//! space it breaks at as a line end. A newline is one byte, as a space is, so every offset in the
//! text (a selection, a note's anchor, a found phrase, the place under the mouse) stays where it
//! was.
//!
//! How wide a stretch of the text is comes from the paragraph shaped as one line, as GPUI shapes
//! it: each glyph's advance is added up in the text's order, not the screen's, so a stretch of a
//! script set right to left, or with letters drawn out of order inside a word (a Devanagari
//! vowel sign before its consonant), measures what it will take on its own line.
//!
//! A line that has no space to end at before the column's edge (a long web address, text written
//! without spaces) stops the choosing: from there to the paragraph's end the lines are GPUI's.
//!
//! A paragraph set right to left is broken here too, and then set flush right
//! (`right_to_left`).

use gpui::{Pixels, SharedString, TextRun, Window};

/// How much narrower than the column the chosen lines are, in pixels: a line shaped alone can
/// measure a fraction of a pixel more than its stretch of the whole paragraph (kerning across
/// the break), and a line GPUI found a hair too long it would wrap a second time.
const SLACK: f32 = 1.0;

/// The paragraph `text` as shown in a column `width` wide: the spaces its lines end at made line
/// ends. Lines it already has (a book's `<br>`) are each broken alone.
pub(super) fn broken(
    text: &str,
    runs: &[TextRun],
    size: Pixels,
    width: f32,
    window: &Window,
) -> String {
    let mut shown: Option<String> = None;
    let mut from = 0;
    for line in text.split('\n') {
        let to = from + line.len();
        if line.contains(' ') {
            let shaped = window.text_system().shape_line(
                SharedString::from(line.to_owned()),
                size,
                &runs_within(runs, from, to),
                None,
            );
            let glyphs = shaped.runs.iter().flat_map(|run| run.glyphs.iter());
            let along = advanced(
                line.len(),
                glyphs.map(|g| (g.index, g.position.x.as_f32())),
                shaped.width.as_f32(),
            );
            let x = |at: usize| along[at];
            for end in line_ends(line, x, shaped.width.as_f32(), width - SLACK) {
                let at = from + end;
                shown
                    .get_or_insert_with(|| text.to_string())
                    .replace_range(at..at + 1, "\n");
            }
        }
        from = to + 1;
    }
    shown.unwrap_or_else(|| text.to_owned())
}

/// How far along a line of `len` bytes each offset is, in the text's order: the advances of the
/// glyphs of the bytes before it, added up. `glyphs` are each glyph's offset and where it is
/// drawn, left to right, and `width` the line's; a glyph's advance is the room to the next one.
fn advanced(len: usize, glyphs: impl Iterator<Item = (usize, f32)>, width: f32) -> Vec<f32> {
    let mut advance = vec![0.0; len + 1];
    let mut glyphs = glyphs.peekable();
    while let Some((at, x)) = glyphs.next() {
        let next = glyphs.peek().map_or(width, |&(_, x)| x);
        if let Some(room) = advance.get_mut(at) {
            *room += next - x;
        }
    }
    let mut along = 0.0;
    for room in &mut advance {
        let own = *room;
        *room = along;
        along += own;
    }
    advance
}

/// The runs of the bytes `from..to` of the text, cut to fit them.
fn runs_within(runs: &[TextRun], from: usize, to: usize) -> Vec<TextRun> {
    let mut within = Vec::new();
    let mut start = 0;
    for run in runs {
        let end = start + run.len;
        let (a, b) = (start.max(from), end.min(to));
        if a < b {
            within.push(TextRun {
                len: b - a,
                ..run.clone()
            });
        }
        start = end;
    }
    within
}

/// Where the lines of one line of text end so that each fits `width`: the byte offsets of the
/// spaces they end at, each line as long as it can be. `x` is where the character at an offset
/// begins, and `whole` how wide the text is. A space breaks only between two words, not inside a
/// run of spaces. A line with no space to end at before `width` ends the choosing.
fn line_ends(text: &str, x: impl Fn(usize) -> f32, whole: f32, width: f32) -> Vec<usize> {
    let bytes = text.as_bytes();
    let spaces = (1..bytes.len().saturating_sub(1))
        .filter(|&i| bytes[i] == b' ' && bytes[i - 1] != b' ' && bytes[i + 1] != b' ');
    let mut ends = Vec::new();
    let mut start = 0.0;
    let mut fits: Option<usize> = None;
    for space in spaces {
        if x(space) - start <= width {
            fits = Some(space);
            continue;
        }
        let Some(end) = fits.take() else {
            return ends;
        };
        ends.push(end);
        start = x(end + 1);
        if x(space) - start > width {
            return ends;
        }
        fits = Some(space);
    }
    if whole - start > width {
        ends.extend(fits);
    }
    ends
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character one unit wide.
    fn ends_at(text: &str, width: f32) -> Vec<String> {
        let x = |at: usize| text[..at].chars().count() as f32;
        let whole = text.chars().count() as f32;
        let mut lines = Vec::new();
        let mut from = 0;
        for end in line_ends(text, x, whole, width) {
            lines.push(text[from..end].to_owned());
            from = end + 1;
        }
        lines.push(text[from..].to_owned());
        lines
    }

    #[test]
    fn each_line_is_as_long_as_the_width_allows_and_ends_at_a_space() {
        assert_eq!(ends_at("aaa bbb ccc ddd", 7.0), ["aaa bbb", "ccc ddd"]);
        assert_eq!(ends_at("aaa bbb ccc", 6.0), ["aaa", "bbb", "ccc"]);
        assert_eq!(ends_at("aaa bbb", 20.0), ["aaa bbb"]);
    }

    #[test]
    fn a_prime_or_an_index_stays_with_its_letter() {
        assert_eq!(
            ends_at("Let M′ be the mid-point", 6.0),
            ["Let M′", "be the", "mid-point"]
        );
        assert_eq!(ends_at("so E₀ and c²", 6.0), ["so E₀", "and c²"]);
    }

    #[test]
    fn words_in_other_scripts_are_not_cut() {
        assert_eq!(
            ends_at("τὸ φῶς ἐκπέμπεται ταχέως", 12.0),
            ["τὸ φῶς", "ἐκπέμπεται", "ταχέως"]
        );
        assert_eq!(ends_at("नमस्ते दुनिया सब", 13.0), ["नमस्ते दुनिया", "सब"]);
    }

    #[test]
    fn a_word_wider_than_the_column_leaves_the_rest_to_gpui() {
        assert_eq!(
            ends_at("see https://example.org/a/long/path now", 10.0),
            ["see", "https://example.org/a/long/path now"]
        );
        assert_eq!(
            ends_at("nospacesatallinthisline", 5.0),
            ["nospacesatallinthisline"]
        );
    }

    #[test]
    fn a_line_set_left_to_right_measures_as_drawn() {
        // "ab c": glyphs 2, 3, 1 and 4 wide, in the text's order.
        let along = advanced(
            4,
            [(0, 0.0), (1, 2.0), (2, 5.0), (3, 6.0)].into_iter(),
            10.0,
        );
        assert_eq!(along, [0.0, 2.0, 5.0, 6.0, 10.0]);
    }

    #[test]
    fn a_line_set_right_to_left_measures_in_the_texts_order() {
        // The same widths, drawn the other way: the last glyph of the text leftmost.
        let along = advanced(
            4,
            [(3, 0.0), (2, 4.0), (1, 5.0), (0, 8.0)].into_iter(),
            10.0,
        );
        assert_eq!(along, [0.0, 2.0, 5.0, 6.0, 10.0]);
    }

    #[test]
    fn letters_drawn_out_of_order_inside_a_word_keep_the_space_after_it_in_place() {
        // A sign of the second byte drawn before the first, then a space and a letter.
        let along = advanced(4, [(1, 0.0), (0, 1.0), (2, 4.0), (3, 5.0)].into_iter(), 7.0);
        assert_eq!(along[2], 4.0);
        assert_eq!(along[4], 7.0);
    }

    #[test]
    fn a_run_of_spaces_is_not_broken_inside() {
        assert_eq!(ends_at("aa  bb cc", 6.0), ["aa  bb", "cc"]);
    }
}
