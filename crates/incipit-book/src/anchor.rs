//! Where a note is tied to a text: a position, and the passage's own words as a fingerprint. When
//! the text has moved (another edition, another extraction), the words find the place again.

/// A passage in a run of text (a PDF page's characters, an EPUB block's).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchor {
    /// The page or block the passage is in.
    pub part: usize,
    /// Where it starts and ends there, in characters.
    pub start: usize,
    pub end: usize,
    /// The passage itself.
    pub quote: String,
}

impl Anchor {
    /// The passage `start..end` of `text` (in characters), in part `part`.
    pub fn of(part: usize, text: &[char], start: usize, end: usize) -> Anchor {
        let (start, end) = (start.min(end), start.max(end).min(text.len()));
        Anchor {
            part,
            start,
            end,
            quote: text[start..end].iter().collect(),
        }
    }

    /// Where the passage is in `text` now: where it was if its words are still there, else the
    /// nearest place they occur. None when they are gone.
    pub fn locate(&self, text: &[char]) -> Option<(usize, usize)> {
        let quote: Vec<char> = self.quote.chars().collect();
        if quote.is_empty() || quote.len() > text.len() {
            return None;
        }
        if text.get(self.start..self.start + quote.len()) == Some(&quote[..]) {
            return Some((self.start, self.start + quote.len()));
        }
        (0..=text.len() - quote.len())
            .filter(|i| text[*i..*i + quote.len()] == quote[..])
            .min_by_key(|i| i.abs_diff(self.start))
            .map(|i| (i, i + quote.len()))
    }
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
    fn a_passage_that_is_gone_is_not_found() {
        let anchor = Anchor::of(0, &chars("a clock"), 2, 7);
        assert_eq!(anchor.locate(&chars("a watch")), None);
    }
}
