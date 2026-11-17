//! Tags, the two ways Obsidian has them: the `tags` property, and `#tag` written in the text. A
//! tag is letters, digits, `_`, `-` and `/` (`#study/plan` is a tag in a tag), is not only
//! digits, and stands after a space, a line break, an opening bracket or the start of the text;
//! `# a heading`, `#123`, a web address's `#fragment` and anything in code are not tags. Case
//! does not tell two tags apart, and the first way a tag was written is the one kept.

use crate::links::code_ranges;

/// Whether a character may be inside a tag, after its first.
fn in_tag(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '/')
}

/// Whether a `#` after this character begins a tag: after white space or an opening bracket.
fn may_begin(before: Option<char>) -> bool {
    before.is_none_or(|c| c.is_whitespace() || c == '(')
}

/// The tags written in a text, once each, in the order they first appear.
pub(crate) fn inline(text: &str) -> Vec<String> {
    let code = code_ranges(text);
    let mut tags = Vec::new();
    let mut before: Option<char> = None;
    let mut before_that: Option<char> = None;
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if c == '#'
            && may_begin(before)
            // `[shown](#Heading)` is a link inside the note, not a tag.
            && !(before == Some('(') && before_that == Some(']'))
            && !code.iter().any(|r| r.contains(&at))
            && chars
                .peek()
                .is_some_and(|&(_, next)| next.is_alphanumeric() || next == '_')
        {
            let start = at + 1;
            let mut end = start;
            while let Some(&(i, next)) = chars.peek() {
                if !in_tag(next) {
                    break;
                }
                end = i + next.len_utf8();
                chars.next();
            }
            let tag = &text[start..end];
            if !tag.chars().all(|c| c.is_ascii_digit()) {
                add(&mut tags, tag);
            }
            before_that = None;
            before = text[..end].chars().next_back();
            continue;
        }
        before_that = before;
        before = Some(c);
    }
    tags
}

/// Adds a tag unless the list has it already, in any case.
pub(crate) fn add(tags: &mut Vec<String>, tag: &str) {
    let tag = tag.trim().trim_start_matches('#');
    if !tag.is_empty() && !tags.iter().any(|t| t.to_lowercase() == tag.to_lowercase()) {
        tags.push(tag.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_in_the_text_are_read_once_each_in_the_order_they_come() {
        assert_eq!(
            inline("A #learning note about #study/plan-b, #learning again and #Learning.\n#last"),
            ["learning", "study/plan-b", "last"]
        );
        assert_eq!(
            inline("(#in-brackets) and #fizică, #día and #_under"),
            ["in-brackets", "fizică", "día", "_under"]
        );
    }

    #[test]
    fn what_only_looks_like_a_tag_is_not_one() {
        let text = "# A heading\n## Another\nA number #123 and #2026, a page https://a.b/#frag, \
                    a note [[Note#Heading]], a link [here](#Section), mid#word, and a lone # sign.\n\
                    ```\n#in-code\n```\nInline `#code` too.\n";
        assert_eq!(inline(text), Vec::<String>::new());
        // A number with a letter in it is a tag, as in Obsidian.
        assert_eq!(inline("#1a and #2026-plan"), ["1a", "2026-plan"]);
    }

    #[test]
    fn broken_input_does_not_panic() {
        for text in ["#", "(#", "é#é", "#/", "#é", "##a", "x\u{0}#a\n#"] {
            let _ = inline(text);
        }
        assert_eq!(inline("##a"), Vec::<String>::new());
    }

    #[test]
    fn a_tag_is_added_once_whatever_its_case_and_without_its_hash() {
        let mut tags = vec!["Physics".to_owned()];
        add(&mut tags, "#physics");
        add(&mut tags, " waves ");
        add(&mut tags, "#");
        assert_eq!(tags, ["Physics", "waves"]);
    }
}
