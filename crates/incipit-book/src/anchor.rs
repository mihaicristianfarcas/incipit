//! Where a note is tied to a text: a position, the passage's own words as a fingerprint, and a
//! few words either side of it. When the text has moved (another edition, another extraction),
//! the words find the place again, and the words around them tell one place the passage's words
//! occur from another (as a web annotation's text quote does: its `prefix` and `suffix`).
//!
//! A passage can run over the end of its page or block into the next ones. It is kept as one
//! quote, its parts joined by a line break, and found again a part at a time (`head_in`): the
//! words that end the part are its start, and what is left is looked for at the start of the
//! next.

/// How many characters either side of a passage are kept with it.
pub const CONTEXT: usize = 32;
/// The fewest characters of a passage that are taken to be its start at the end of a part:
/// fewer would match by chance.
const SHORTEST_HEAD: usize = 4;

/// A passage in a run of text (a PDF page's characters, an EPUB block's).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Anchor {
    /// The page or block the passage is in.
    pub part: usize,
    /// Where it starts and ends there, in characters.
    pub start: usize,
    pub end: usize,
    /// The passage itself.
    pub quote: String,
    /// The characters before and after it in its part, `CONTEXT` at most each.
    pub prefix: String,
    pub suffix: String,
}

impl Anchor {
    /// The passage `start..end` of `text` (in characters), in part `part`.
    pub fn of(part: usize, text: &[char], start: usize, end: usize) -> Anchor {
        let (start, end) = (
            start.min(end).min(text.len()),
            start.max(end).min(text.len()),
        );
        Anchor {
            part,
            start,
            end,
            quote: text[start..end].iter().collect(),
            prefix: text[start.saturating_sub(CONTEXT)..start].iter().collect(),
            suffix: text[end..(end + CONTEXT).min(text.len())].iter().collect(),
        }
    }

    /// A passage kept elsewhere (in the vault), to be found again in part `part`: its words and
    /// the words around them, its place in the part not known.
    pub fn kept(part: usize, quote: &str, prefix: &str, suffix: &str) -> Anchor {
        Anchor {
            part,
            start: 0,
            end: quote.chars().count(),
            quote: quote.to_owned(),
            prefix: prefix.to_owned(),
            suffix: suffix.to_owned(),
        }
    }

    /// Where the passage is in `text` now: where it was if its words are still there, else the
    /// place they occur whose surroundings are most like the passage's (its prefix and suffix),
    /// the nearest its old place among equals. None when they are gone.
    pub fn locate(&self, text: &[char]) -> Option<(usize, usize)> {
        let quote: Vec<char> = self.quote.chars().collect();
        if quote.is_empty() || quote.len() > text.len() {
            return None;
        }
        let prefix: Vec<char> = self.prefix.chars().collect();
        let suffix: Vec<char> = self.suffix.chars().collect();
        // Where it was, with the words around it as they were: there.
        let end = self.start + quote.len();
        if text.get(self.start..end) == Some(&quote[..])
            && matched_back(&text[..self.start], &prefix) == prefix.len()
            && matched_forward(&text[end..], &suffix) == suffix.len()
        {
            return Some((self.start, end));
        }
        (0..=text.len() - quote.len())
            .filter(|i| text[*i..*i + quote.len()] == quote[..])
            .max_by_key(|&i| {
                let before = matched_back(&text[..i], &prefix);
                let after = matched_forward(&text[i + quote.len()..], &suffix);
                // More surroundings matched first; then the nearest (the larger key).
                (before + after, std::cmp::Reverse(i.abs_diff(self.start)))
            })
            .map(|i| (i, i + quote.len()))
    }

    /// How much of the passage is in `text`, when it may run on into the parts after: where it
    /// is, if it is there whole, else the start of it that ends the text, and what of it is left
    /// for the next part (from its first character that is not white space). None when neither
    /// is there.
    pub fn head_in(&self, text: &[char]) -> Option<((usize, usize), Option<String>)> {
        if let Some(found) = self.locate(text) {
            return Some((found, None));
        }
        let quote: Vec<char> = self.quote.chars().collect();
        // The text without the white space at its end.
        let end = text
            .iter()
            .rposition(|c| !c.is_whitespace())
            .map_or(0, |at| at + 1);
        let earliest = end.saturating_sub(quote.len());
        for start in earliest..end.saturating_sub(SHORTEST_HEAD - 1) {
            let head = &text[start..end];
            let at_word =
                start == 0 || !text[start - 1].is_alphanumeric() || !head[0].is_alphanumeric();
            if !at_word || quote.get(..head.len()) != Some(head) {
                continue;
            }
            let rest = &quote[head.len()..];
            // The passage goes on past the part at a break between words.
            if rest.first().is_some_and(|c| !c.is_whitespace()) {
                continue;
            }
            let rest: String = rest.iter().collect::<String>().trim_start().to_owned();
            return Some(((start, end), (!rest.is_empty()).then_some(rest)));
        }
        None
    }
}

/// A passage that runs over several parts as one quote: the parts' quotes, each part's joined
/// to the next by a line break unless it ends in white space already.
pub fn joined(anchors: &[Anchor]) -> String {
    let mut out = String::new();
    for anchor in anchors {
        if !out.is_empty() && !out.ends_with(char::is_whitespace) {
            out.push('\n');
        }
        out.push_str(&anchor.quote);
    }
    out
}

/// How many of `prefix`'s last characters `text` ends with.
fn matched_back(text: &[char], prefix: &[char]) -> usize {
    text.iter()
        .rev()
        .zip(prefix.iter().rev())
        .take_while(|(a, b)| a == b)
        .count()
}

/// How many of `suffix`'s first characters `text` starts with.
fn matched_forward(text: &[char], suffix: &[char]) -> usize {
    text.iter()
        .zip(suffix.iter())
        .take_while(|(a, b)| a == b)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn a_passage_is_found_where_it_was() {
        let text = chars("time is what a clock reads, and a clock is what reads time");
        let anchor = Anchor::of(0, &text, 15, 20);
        assert_eq!(anchor.quote, "clock");
        assert_eq!(anchor.prefix, "time is what a ");
        assert_eq!(anchor.suffix, " reads, and a clock is what read");
        assert_eq!(anchor.locate(&text), Some((15, 20)));
    }

    #[test]
    fn a_passage_that_moved_is_found_by_its_words_nearest_its_old_place() {
        let old = chars("time is what a clock reads, and a clock is what reads time");
        let anchor = Anchor::of(0, &old, 34, 39);
        let new = chars("So: time is what a clock reads, and a clock is what reads time");
        assert_eq!(anchor.locate(&new), Some((38, 43)));
    }

    #[test]
    fn a_passage_kept_without_its_place_is_found_by_the_words_around_it() {
        let text = chars("time is what a clock reads, and a clock is what reads time");
        // The second "clock", kept with the words around it but not where it was.
        let kept = Anchor::kept(0, "clock", "and a ", " is what");
        assert_eq!(kept.locate(&text), Some((34, 39)));
        // Without them, the first.
        assert_eq!(
            Anchor::kept(0, "clock", "", "").locate(&text),
            Some((15, 20))
        );
    }

    #[test]
    fn a_passage_that_is_gone_is_not_found() {
        let anchor = Anchor::of(0, &chars("a clock"), 2, 7);
        assert_eq!(anchor.locate(&chars("a watch")), None);
        assert_eq!(anchor.head_in(&chars("a watch")), None);
    }

    #[test]
    fn a_passage_over_two_parts_is_found_a_part_at_a_time() {
        let one = chars("The first page ends with the start of it\n");
        let two = chars("and the second page goes on with the end. More.");
        let parts = [
            Anchor::of(4, &one, 20, one.len()),
            Anchor::of(5, &two, 0, 41),
        ];
        let quote = joined(&parts);
        assert_eq!(
            quote,
            "with the start of it\nand the second page goes on with the end."
        );
        let kept = Anchor::kept(4, &quote, &parts[0].prefix, &parts[1].suffix);
        let ((start, end), rest) = kept.head_in(&one).unwrap();
        assert_eq!((start, end), (20, 40));
        let rest = rest.unwrap();
        assert_eq!(rest, "and the second page goes on with the end.");
        let next = Anchor::kept(5, &rest, "", &parts[1].suffix);
        assert_eq!(next.head_in(&two), Some(((0, 41), None)));
    }

    #[test]
    fn the_start_of_a_passage_ends_its_part_only_at_a_break_between_words() {
        // "it" ends the part, but the passage goes on with "itself": not a break between words.
        let kept = Anchor::kept(0, "it itself goes on", "", "");
        assert_eq!(kept.head_in(&chars("a page that ends with it")), None);
        // And a few letters that happen to end a part are not taken for a passage's start.
        let kept = Anchor::kept(0, "is\nmore", "", "");
        assert_eq!(kept.head_in(&chars("this")), None);
    }
}
