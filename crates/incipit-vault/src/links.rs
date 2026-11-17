//! Links written in a document, the two ways Obsidian writes them:
//!
//! ```text
//! [[Note]]  [[Note|shown text]]  [[Note#Heading]]  [[folder/Note]]  ![[picture.png]]
//! [shown text](Note.md)  [text](../folder/My%20note.md#Heading)  [text](<My note.md>)
//! ```
//!
//! Links inside fenced code blocks and inline code are not links. Markdown links with a scheme
//! (`https:`, `mailto:`) or only a `#heading` point outside the vault and are left out. Each link
//! remembers where its target is written, so a rename changes exactly those characters and
//! nothing else in the file.

use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkStyle {
    /// `[[target#heading|label]]`
    Wiki,
    /// `[label](target#heading)`
    Markdown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    /// What it points at, as a name or a path (Markdown links decoded: `My note.md`).
    pub target: String,
    /// After `#`: a heading, a `^block`, or a book's `page=12`.
    pub heading: Option<String>,
    /// The text shown instead of the target.
    pub label: Option<String>,
    /// `![[…]]` or `![…](…)`: shown in place.
    pub embed: bool,
    pub style: LinkStyle,
    /// The whole link, in bytes of the text it was read from.
    pub span: Range<usize>,
    /// The target as written (before any `#`), in bytes of the text.
    pub(crate) target_span: Range<usize>,
    /// Whether a Markdown target was written in `<…>` (spaces as they are, not `%20`).
    pub(crate) angled: bool,
    /// The line, from 1.
    pub line: usize,
}

/// Byte ranges of fenced code blocks and inline code, which hold no links.
pub(crate) fn code_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut fence: Option<(char, usize, usize)> = None; // marker, length, start
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let indent = line.len() - line.trim_start_matches(' ').len();
        let bare = line.trim_start_matches(' ');
        let run = |marker: char| bare.chars().take_while(|&c| c == marker).count();
        match fence {
            Some((marker, length, start)) => {
                if indent <= 3
                    && run(marker) >= length
                    && bare.trim_start_matches(marker).trim().is_empty()
                {
                    ranges.push(start..offset + line.len());
                    fence = None;
                }
            }
            None => {
                let opened = ['`', '~']
                    .into_iter()
                    .map(|m| (m, run(m)))
                    .find(|&(_, n)| n >= 3)
                    .filter(|_| indent <= 3);
                if let Some((marker, length)) = opened {
                    fence = Some((marker, length, offset));
                } else {
                    inline_code(line, offset, &mut ranges);
                }
            }
        }
        offset += line.len();
    }
    if let Some((_, _, start)) = fence {
        ranges.push(start..text.len());
    }
    ranges
}

/// Inline code on one line: a run of backticks up to the next run of the same length.
fn inline_code(line: &str, offset: usize, ranges: &mut Vec<Range<usize>>) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i] == b'`' {
            i += 1;
        }
        let length = i - start;
        let mut j = i;
        let mut closed = None;
        while j < bytes.len() {
            if bytes[j] == b'`' {
                let run_start = j;
                while j < bytes.len() && bytes[j] == b'`' {
                    j += 1;
                }
                if j - run_start == length {
                    closed = Some(j);
                    break;
                }
            } else {
                j += 1;
            }
        }
        if let Some(end) = closed {
            ranges.push(offset + start..offset + end);
            i = end;
        }
    }
}

fn inside(ranges: &[Range<usize>], at: usize) -> bool {
    ranges.iter().any(|r| r.contains(&at))
}

/// Every link in a text, in order.
pub fn parse(text: &str) -> Vec<Link> {
    let code = code_ranges(text);
    let mut links = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut line = 1;
    let mut counted = 0;
    while i < bytes.len() {
        let found = match bytes[i] {
            b'[' if !inside(&code, i) => {
                if bytes.get(i + 1) == Some(&b'[') {
                    wiki_link(text, i)
                } else {
                    markdown_link(text, i)
                }
            }
            _ => None,
        };
        match found {
            Some(mut link) => {
                let embed = link.span.start > 0 && bytes[link.span.start - 1] == b'!';
                if embed {
                    link.span.start -= 1;
                    link.embed = true;
                }
                line += text[counted..link.span.start].matches('\n').count();
                counted = link.span.start;
                link.line = line;
                i = link.span.end;
                links.push(link);
            }
            None => i += 1,
        }
    }
    links
}

/// `[[…]]` starting at `at`, on one line, with no brackets inside.
fn wiki_link(text: &str, at: usize) -> Option<Link> {
    let rest = &text[at + 2..];
    let end = rest.find("]]")?;
    let inner = &rest[..end];
    if inner.contains(['[', ']', '\n']) {
        return None;
    }
    let inner_start = at + 2;
    // In a table a `|` is written `\|`; the backslash belongs to neither side.
    let (before_label, label) = match inner.find('|') {
        Some(bar) => (&inner[..bar], Some(&inner[bar + 1..])),
        None => (inner, None),
    };
    let before_label = before_label.strip_suffix('\\').unwrap_or(before_label);
    let (target, heading) = match before_label.find('#') {
        Some(hash) => (&before_label[..hash], Some(&before_label[hash + 1..])),
        None => (before_label, None),
    };
    let target_trimmed = target.trim();
    if target_trimmed.is_empty() && heading.is_none() {
        return None;
    }
    let lead = target.len() - target.trim_start().len();
    let target_start = inner_start + lead;
    Some(Link {
        target: target_trimmed.to_owned(),
        heading: heading
            .map(|h| h.trim().to_owned())
            .filter(|h| !h.is_empty()),
        label: label.map(|l| l.trim().to_owned()).filter(|l| !l.is_empty()),
        embed: false,
        style: LinkStyle::Wiki,
        span: at..inner_start + end + 2,
        target_span: target_start..target_start + target_trimmed.len(),
        angled: false,
        line: 0,
    })
}

/// `[label](destination)` starting at `at`, for destinations inside the vault.
fn markdown_link(text: &str, at: usize) -> Option<Link> {
    let rest = &text[at + 1..];
    let close = rest.find(']')?;
    let label = &rest[..close];
    if label.contains(['[', '\n']) {
        return None;
    }
    let after = at + 1 + close + 1;
    if text.as_bytes().get(after) != Some(&b'(') {
        return None;
    }
    let dest_start = after + 1;
    let tail = &text[dest_start..];
    let (raw, raw_start, angled, end) = if let Some(angled) = tail.strip_prefix('<') {
        let close = angled.find('>')?;
        let raw = &angled[..close];
        let paren = angled[close + 1..].find(')')?;
        (
            raw,
            dest_start + 1,
            true,
            dest_start + 1 + close + 1 + paren + 1,
        )
    } else {
        let stop = tail.find([' ', ')', '\n'])?;
        let raw = &tail[..stop];
        let paren = tail[stop..].find(')')?;
        if tail[stop..stop + paren].contains('\n') {
            return None;
        }
        (raw, dest_start, false, dest_start + stop + paren + 1)
    };
    if raw.is_empty() || raw.starts_with('#') || has_scheme(raw) {
        return None;
    }
    let (path, heading) = match raw.find('#') {
        Some(hash) => (&raw[..hash], Some(&raw[hash + 1..])),
        None => (raw, None),
    };
    let target = if angled {
        path.to_owned()
    } else {
        decode(path)
    };
    Some(Link {
        target,
        heading: heading.map(decode).filter(|h| !h.is_empty()),
        label: (!label.trim().is_empty()).then(|| label.trim().to_owned()),
        embed: false,
        style: LinkStyle::Markdown,
        span: at..end,
        target_span: raw_start..raw_start + path.len(),
        angled,
        line: 0,
    })
}

/// Whether a destination starts with a URL scheme (`https:`, `mailto:`, `obsidian:`).
fn has_scheme(dest: &str) -> bool {
    match dest.find(':') {
        Some(colon) if colon > 0 => {
            let scheme = &dest[..colon];
            scheme.starts_with(|c: char| c.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        }
        _ => false,
    }
}

/// `%XX` escapes decoded; anything malformed is kept as written.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let hex = |b: u8| (b as char).to_digit(16);
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((high * 16 + low) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_owned())
}

/// A path as a Markdown link destination: spaces and the few characters that would end or
/// confuse a link escaped.
fn encode(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            '#' => out.push_str("%23"),
            '%' => out.push_str("%25"),
            _ => out.push(c),
        }
    }
    out
}

/// The text with some links pointed elsewhere: `change` gives a link's new target (a name or a
/// path, as a Wiki link would write it), or `None` to leave it as it is.
pub(crate) fn retarget(text: &str, mut change: impl FnMut(&Link) -> Option<String>) -> String {
    let links = parse(text);
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for link in &links {
        let Some(new) = change(link) else { continue };
        let written = match (link.style, link.angled) {
            (LinkStyle::Wiki, _) | (LinkStyle::Markdown, true) => new,
            (LinkStyle::Markdown, false) => encode(&new),
        };
        out.push_str(&text[last..link.target_span.start]);
        out.push_str(&written);
        last = link.target_span.end;
    }
    out.push_str(&text[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wiki_links_are_read_with_their_heading_and_label() {
        let links = parse("See [[Derivative#Rules|the rules]] and ![[graph.png]].\n[[Notes/Plan]]");
        assert_eq!(links.len(), 3);
        assert_eq!(links[0].target, "Derivative");
        assert_eq!(links[0].heading.as_deref(), Some("Rules"));
        assert_eq!(links[0].label.as_deref(), Some("the rules"));
        assert!(!links[0].embed);
        assert!(links[1].embed);
        assert_eq!(links[1].target, "graph.png");
        assert_eq!(links[2].target, "Notes/Plan");
        assert_eq!(links[2].line, 2);
    }

    #[test]
    fn markdown_links_to_files_in_the_vault_are_read_and_web_links_are_not() {
        let links = parse(
            "[one](My%20note.md) [two](<Other note.md#Part 2>) [web](https://example.com) \
             [mail](mailto:a@b.c) [here](#Heading) [titled](../x.md \"A title\")",
        );
        let targets: Vec<&str> = links.iter().map(|l| l.target.as_str()).collect();
        assert_eq!(targets, ["My note.md", "Other note.md", "../x.md"]);
        assert_eq!(links[1].heading.as_deref(), Some("Part 2"));
        assert_eq!(links[0].label.as_deref(), Some("one"));
    }

    #[test]
    fn links_in_code_are_not_links() {
        let text = "Real [[A]].\n```\n[[Not a link]]\n```\nInline `[[B]]` and ``[[C]]``.\n~~~md\n[x](y.md)\n~~~\n[[D]]";
        let targets: Vec<String> = parse(text).into_iter().map(|l| l.target).collect();
        assert_eq!(targets, ["A", "D"]);
    }

    #[test]
    fn retargeting_changes_only_the_target() {
        let text = "[[Old#H|shown]] [[old]] [x](Old.md#H) [y](<Old.md>) [[Other]]";
        let out = retarget(text, |link| {
            link.target
                .trim_end_matches(".md")
                .eq_ignore_ascii_case("old")
                .then(|| match link.style {
                    LinkStyle::Wiki => "New name".to_owned(),
                    LinkStyle::Markdown => "New name.md".to_owned(),
                })
        });
        assert_eq!(
            out,
            "[[New name#H|shown]] [[New name]] [x](New%20name.md#H) [y](<New name.md>) [[Other]]"
        );
    }

    #[test]
    fn a_table_escaped_bar_still_separates_the_label() {
        let links = parse("| [[Note\\|label]] |");
        assert_eq!(links[0].target, "Note");
        assert_eq!(links[0].label.as_deref(), Some("label"));
    }

    #[test]
    fn broken_brackets_are_not_links_and_do_not_panic() {
        for text in [
            "[[",
            "[[]]",
            "[a](",
            "[a](<b",
            "]]",
            "[[a\nb]]",
            "[x](%zz.md)",
            "é[[ü",
        ] {
            let _ = parse(text);
        }
        assert!(parse("[[]]").is_empty());
        assert_eq!(parse("[x](%zz.md)")[0].target, "%zz.md");
    }
}
