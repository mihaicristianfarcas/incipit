//! What a text field needs to be a place to write Markdown: how each stretch of the text looks
//! (the source stays as written; its marks are dimmed and what they mean is shown), what Enter
//! does at the end of a list item, wrapping a selection in a mark, and the history Undo walks.
//! Nothing here draws or knows of a window, so all of it is tested as plain text.

use std::ops::Range;
use std::time::{Duration, Instant};

/// The colour a stretch of text takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tone {
    #[default]
    Plain,
    /// Markdown's own marks: `#`, `**`, list bullets, a link's address.
    Dim,
    Link,
    Quote,
}

/// How a stretch of text looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Look {
    pub bold: bool,
    pub italic: bool,
    pub mono: bool,
    pub tone: Tone,
}

/// The looks of `text` from start to end: each with how many bytes it runs for. The lengths add
/// up to the text's.
pub fn looks(text: &str) -> Vec<(usize, Look)> {
    let mut each = vec![Look::default(); text.len()];
    let mut fenced = false;
    let mut at = 0;
    for line in text.split('\n') {
        let range = at..at + line.len();
        at = range.end + 1;
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            paint(&mut each, range, |l| l.tone = Tone::Dim);
            continue;
        }
        if fenced {
            paint(&mut each, range, |l| l.mono = true);
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let rest = &line[indent..];
        let start = range.start + indent;
        if let Some(level) = heading(rest) {
            paint(&mut each, start..start + level, |l| l.tone = Tone::Dim);
            paint(&mut each, start + level..range.end, |l| l.bold = true);
            inline(rest, level, start, &mut each);
        } else if rest.starts_with('>') {
            paint(&mut each, start..start + 1, |l| l.tone = Tone::Dim);
            paint(&mut each, start + 1..range.end, |l| l.tone = Tone::Quote);
            inline(rest, 1, start, &mut each);
        } else {
            let marker = list_marker(rest).map(|m| m.len()).unwrap_or(0);
            paint(&mut each, start..start + marker, |l| l.tone = Tone::Dim);
            inline(rest, marker, start, &mut each);
        }
    }
    let mut runs: Vec<(usize, Look)> = Vec::new();
    for look in each {
        match runs.last_mut() {
            Some((len, last)) if *last == look => *len += 1,
            _ => runs.push((1, look)),
        }
    }
    runs
}

fn paint(each: &mut [Look], range: Range<usize>, change: impl Fn(&mut Look)) {
    for look in &mut each[range] {
        change(look);
    }
}

/// How many `#` a heading line starts with.
fn heading(line: &str) -> Option<usize> {
    let level = line.bytes().take_while(|b| *b == b'#').count();
    ((1..=6).contains(&level) && line[level..].starts_with(' ')).then_some(level)
}

/// A list item's marker with the space after it: `- `, `12. `, `- [ ] `.
fn list_marker(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    let bullet = match bytes.first()? {
        b'-' | b'*' | b'+' => 1,
        b'0'..=b'9' => {
            let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
            match bytes.get(digits) {
                Some(b'.') | Some(b')') if digits <= 9 => digits + 1,
                _ => return None,
            }
        }
        _ => return None,
    };
    if bytes.get(bullet) != Some(&b' ') {
        return None;
    }
    let after = &line[bullet + 1..];
    let task = ["[ ] ", "[x] ", "[X] "]
        .iter()
        .any(|t| after.starts_with(t));
    Some(&line[..bullet + 1 + if task { 4 } else { 0 }])
}

/// Marks inside a line, from byte `from` of it; `base` is where the line starts in the text.
fn inline(line: &str, from: usize, base: usize, each: &mut [Look]) {
    let bytes = line.as_bytes();
    let mut i = from;
    while i < bytes.len() {
        let rest = &line[i..];
        // A pair of marks with something between them, closed on this line.
        let pair = |open: &str, close: &str| -> Option<usize> {
            let inner = rest.strip_prefix(open)?;
            let end = inner.find(close)?;
            (end > 0).then_some(end)
        };
        let dim =
            |each: &mut [Look], range: Range<usize>| paint(each, range, |l| l.tone = Tone::Dim);
        if let Some(end) = pair("`", "`") {
            dim(each, base + i..base + i + 1);
            paint(each, base + i + 1..base + i + 1 + end, |l| l.mono = true);
            dim(each, base + i + 1 + end..base + i + 2 + end);
            i += end + 2;
        } else if let Some(end) = pair("$", "$").filter(|_| !rest.starts_with("$ ")) {
            paint(each, base + i..base + i + end + 2, |l| l.mono = true);
            i += end + 2;
        } else if let Some(end) = pair("[[", "]]") {
            dim(each, base + i..base + i + 2);
            paint(each, base + i + 2..base + i + 2 + end, |l| {
                l.tone = Tone::Link
            });
            dim(each, base + i + 2 + end..base + i + 4 + end);
            i += end + 4;
        } else if let Some((label, address)) = link(rest) {
            dim(each, base + i..base + i + 1);
            paint(each, base + i + 1..base + i + 1 + label, |l| {
                l.tone = Tone::Link
            });
            dim(each, base + i + 1 + label..base + i + 4 + label + address);
            i += label + address + 4;
        } else if let Some((mark, end)) = ["**", "__"].iter().find_map(|m| Some((*m, pair(m, m)?)))
        {
            let _ = mark;
            dim(each, base + i..base + i + 2);
            paint(each, base + i + 2..base + i + 2 + end, |l| l.bold = true);
            dim(each, base + i + 2 + end..base + i + 4 + end);
            // Italic and code may sit inside bold.
            inline(&line[..i + 2 + end], i + 2, base, each);
            i += end + 4;
        } else if let Some(end) = emphasis(line, i) {
            dim(each, base + i..base + i + 1);
            paint(each, base + i + 1..base + i + 1 + end, |l| l.italic = true);
            dim(each, base + i + 1 + end..base + i + 2 + end);
            i += end + 2;
        } else {
            i += rest.chars().next().map(char::len_utf8).unwrap_or(1);
        }
    }
}

/// `[label](address)` at the start of `rest`: the label's and the address's lengths.
fn link(rest: &str) -> Option<(usize, usize)> {
    let inner = rest.strip_prefix('[')?;
    let label = inner.find(']')?;
    let after = inner[label + 1..].strip_prefix('(')?;
    let address = after.find(')')?;
    (label > 0).then_some((label, address))
}

/// `*words*` or `_words_` starting at byte `i`: the length between the marks. An underscore
/// inside a word (snake_case) is not a mark, and a mark is not followed by a space.
fn emphasis(line: &str, i: usize) -> Option<usize> {
    let mark = *line.as_bytes().get(i)?;
    if mark != b'*' && mark != b'_' {
        return None;
    }
    let inner = &line[i + 1..];
    if inner.starts_with(' ') || inner.as_bytes().first() == Some(&mark) {
        return None;
    }
    let word_before = line[..i]
        .chars()
        .next_back()
        .is_some_and(char::is_alphanumeric);
    if mark == b'_' && word_before {
        return None;
    }
    let end = inner.find(mark as char)?;
    (end > 0 && !inner[..end].ends_with(' ')).then_some(end)
}

/// What Enter does with the caret at `caret`: the stretch to replace and what goes there. In a
/// list or a quote the next line starts as one too (a numbered item counts on, a task starts
/// unticked); on an item with nothing in it, Enter ends the list instead.
pub fn enter(text: &str, caret: usize) -> (Range<usize>, String) {
    let start = text[..caret].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line = &text[start..caret];
    let indent = &line[..line.len() - line.trim_start().len()];
    let rest = &line[indent.len()..];
    let marker = match list_marker(rest) {
        Some(marker) => marker.to_string(),
        None if rest.starts_with("> ") => "> ".to_string(),
        None => return (caret..caret, format!("\n{indent}")),
    };
    if rest.len() == marker.len() {
        return (start..caret, String::new());
    }
    let digits = marker.bytes().take_while(u8::is_ascii_digit).count();
    let next = if digits > 0 {
        let number: u64 = marker[..digits].parse().unwrap_or(0);
        format!("{}{}", number + 1, &marker[digits..])
    } else {
        marker.replace("[x]", "[ ]").replace("[X]", "[ ]")
    };
    (caret..caret, format!("\n{indent}{next}"))
}

/// Whether the caret's line is a list item (so Tab indents it rather than leaving the field).
pub fn in_list(text: &str, caret: usize) -> bool {
    let start = text[..caret].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = text[caret..]
        .find('\n')
        .map(|i| caret + i)
        .unwrap_or(text.len());
    list_marker(text[start..end].trim_start()).is_some()
}

/// Wrap the selection in `mark` (`**`, `*`), or take the mark away when it is already there:
/// the stretch to replace, what goes there, and the selection afterwards.
pub fn toggle(
    text: &str,
    selected: Range<usize>,
    mark: &str,
) -> (Range<usize>, String, Range<usize>) {
    let inside = &text[selected.clone()];
    let before = text[..selected.start].ends_with(mark);
    let after = text[selected.end..].starts_with(mark);
    if before && after {
        let range = selected.start - mark.len()..selected.end + mark.len();
        let kept = range.start..range.start + inside.len();
        return (range, inside.to_string(), kept);
    }
    let kept = selected.start + mark.len()..selected.end + mark.len();
    (selected, format!("{mark}{inside}{mark}"), kept)
}

/// A moment of a field: its text and where the selection was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Moment {
    pub text: String,
    pub selected: Range<usize>,
}

/// What Undo and Redo walk. Typing in one go is one step: a moment is kept only when the last
/// was kept more than `PAUSE` ago.
#[derive(Default)]
pub struct History {
    past: Vec<Moment>,
    future: Vec<Moment>,
    kept: Option<Instant>,
}

const PAUSE: Duration = Duration::from_millis(700);
/// Steps kept; a note's whole text is in each.
const STEPS: usize = 200;

impl History {
    /// Before a change: keep the moment it changes, unless typing goes on.
    pub fn before_change(&mut self, moment: Moment, now: Instant) {
        self.future.clear();
        if self
            .kept
            .is_some_and(|kept| now.duration_since(kept) < PAUSE)
            && !self.past.is_empty()
        {
            self.kept = Some(now);
            return;
        }
        self.kept = Some(now);
        self.past.push(moment);
        if self.past.len() > STEPS {
            self.past.remove(0);
        }
    }

    /// The moment to go back to, given the one shown now.
    pub fn undo(&mut self, now: Moment) -> Option<Moment> {
        let back = self.past.pop()?;
        self.future.push(now);
        self.kept = None;
        Some(back)
    }

    pub fn redo(&mut self, now: Moment) -> Option<Moment> {
        let forward = self.future.pop()?;
        self.past.push(now);
        self.kept = None;
        Some(forward)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The text with each stretch's look named: `b`old, `i`talic, `m`ono, `d`im, `l`ink, `q`uote.
    fn named(text: &str) -> Vec<(String, String)> {
        let mut at = 0;
        looks(text)
            .into_iter()
            .map(|(len, look)| {
                let piece = text[at..at + len].to_string();
                at += len;
                let mut name = String::new();
                for (on, letter) in [(look.bold, 'b'), (look.italic, 'i'), (look.mono, 'm')] {
                    if on {
                        name.push(letter);
                    }
                }
                name.push_str(match look.tone {
                    Tone::Plain => "",
                    Tone::Dim => "d",
                    Tone::Link => "l",
                    Tone::Quote => "q",
                });
                (piece, name)
            })
            .collect()
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn the_looks_cover_the_whole_text() {
        for text in [
            "",
            "plain",
            "# A *b* `c`\n\n- [[d]] é\n```\nx\n```\n> q **w**",
        ] {
            assert_eq!(looks(text).iter().map(|l| l.0).sum::<usize>(), text.len());
        }
    }

    #[test]
    fn a_heading_is_bold_after_its_dimmed_marks() {
        assert_eq!(named("## Title"), pairs(&[("##", "d"), (" Title", "b")]));
    }

    #[test]
    fn bold_and_italic_show_with_their_marks_dimmed() {
        assert_eq!(
            named("a **b** *c*"),
            pairs(&[
                ("a ", ""),
                ("**", "d"),
                ("b", "b"),
                ("**", "d"),
                (" ", ""),
                ("*", "d"),
                ("c", "i"),
                ("*", "d")
            ])
        );
    }

    #[test]
    fn an_underscore_inside_a_word_is_not_emphasis() {
        assert_eq!(named("snake_case_name"), pairs(&[("snake_case_name", "")]));
    }

    #[test]
    fn a_wikilink_and_a_link_show_their_label() {
        assert_eq!(
            named("[[Entropy]]"),
            pairs(&[("[[", "d"), ("Entropy", "l"), ("]]", "d")])
        );
        assert_eq!(
            named("[a](b.md)"),
            pairs(&[("[", "d"), ("a", "l"), ("](b.md)", "d")])
        );
    }

    #[test]
    fn marks_inside_a_code_block_stay_as_written() {
        assert_eq!(
            named("```\n**x**\n```"),
            pairs(&[
                ("```", "d"),
                ("\n", ""),
                ("**x**", "m"),
                ("\n", ""),
                ("```", "d")
            ])
        );
    }

    #[test]
    fn a_mark_left_open_is_plain_text() {
        assert_eq!(
            named("2 * 3 and **open"),
            pairs(&[("2 * 3 and **open", "")])
        );
    }

    #[test]
    fn enter_in_a_list_starts_the_next_item() {
        assert_eq!(enter("- milk", 6), (6..6, "\n- ".to_string()));
        assert_eq!(enter("  3. three", 10), (10..10, "\n  4. ".to_string()));
        assert_eq!(enter("- [x] done", 10), (10..10, "\n- [ ] ".to_string()));
        assert_eq!(enter("> said", 6), (6..6, "\n> ".to_string()));
    }

    #[test]
    fn enter_on_an_empty_item_ends_the_list() {
        assert_eq!(enter("- milk\n- ", 9), (7..9, String::new()));
    }

    #[test]
    fn enter_in_a_paragraph_keeps_its_indent() {
        assert_eq!(enter("  text", 6), (6..6, "\n  ".to_string()));
    }

    #[test]
    fn toggling_a_mark_twice_gives_the_text_back() {
        let (range, new, kept) = toggle("a word here", 2..6, "**");
        let mut text = "a word here".to_string();
        text.replace_range(range, &new);
        assert_eq!((text.as_str(), kept.clone()), ("a **word** here", 4..8));
        let (range, new, kept) = toggle(&text, kept, "**");
        text.replace_range(range, &new);
        assert_eq!((text.as_str(), kept), ("a word here", 2..6));
    }

    #[test]
    fn typing_in_one_go_is_one_step_to_undo() {
        let mut history = History::default();
        let start = Instant::now();
        let moment = |text: &str| Moment {
            text: text.into(),
            selected: 0..0,
        };
        history.before_change(moment(""), start);
        history.before_change(moment("h"), start + Duration::from_millis(100));
        history.before_change(moment("hi"), start + Duration::from_millis(200));
        history.before_change(moment("hi!"), start + Duration::from_secs(5));
        assert_eq!(history.undo(moment("hi! there")), Some(moment("hi!")));
        assert_eq!(history.undo(moment("hi!")), Some(moment("")));
        assert_eq!(history.undo(moment("")), None);
        assert_eq!(history.redo(moment("")), Some(moment("hi!")));
    }
}
