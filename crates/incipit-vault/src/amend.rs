//! Changing some of the properties in a file's frontmatter block and keeping the rest of its text
//! as it was: a comment, a key Incipit does not know, the way a list or a string was quoted, a
//! blank line, the line endings. A learner's own vault (an Obsidian one) is full of such things,
//! and the first time Incipit writes a note it must not tidy them.
//!
//! It works on the block's lines. Each top-level key and the lines under it are an entry;
//! an entry whose value changed is written again (its key as it was written, a list that was on
//! one line kept on one line, a comment after a one-line value kept), an entry that is gone is
//! cut out, and a new one goes before the first later key that was there, or at the end. The
//! lines between entries (comments, blank lines) are never touched. A block this cannot read
//! line by line (a flow map, anchors, a key it cannot tell, keys moved) is written whole by the
//! YAML writer instead, and so is any result that does not read back as exactly the properties
//! asked for, so the file never says something else than the vault does.

use crate::frontmatter::{parse, write};
use crate::value::{Properties, Value};

/// The properties' text after they have become `new`. `front` is the text as it was read, and
/// `eol` the line ending to write new lines with.
pub(crate) fn amend(front: &str, new: &Properties, eol: &str) -> String {
    let Ok(old) = parse(front) else {
        return whole(new, eol);
    };
    if old == *new {
        return front.to_owned();
    }
    edit(front, &old, new, eol)
        .filter(|text| parse(text).is_ok_and(|read| read == *new))
        .unwrap_or_else(|| whole(new, eol))
}

/// All the properties written afresh.
fn whole(properties: &Properties, eol: &str) -> String {
    ending(write(properties), eol)
}

/// Text made of `\n` lines, with the line ending the file uses.
fn ending(text: String, eol: &str) -> String {
    if eol == "\n" {
        text
    } else {
        text.replace('\n', eol)
    }
}

/// A key at the top of the block and the lines that belong to it.
struct Entry<'a> {
    /// The key as it reads.
    key: String,
    /// What stands before the colon, as written (`"my key"`).
    token: &'a str,
    /// Its first line and the one after its last.
    start: usize,
    end: usize,
}

/// A line with nothing in it but a comment, or nothing.
fn is_gap(line: &str) -> bool {
    let line = line.trim();
    line.is_empty() || line.starts_with('#')
}

/// The key a line starts with: what stands before its colon, where the colon is, and the key as
/// it reads. `None` for a line that is not `key:` or `key: value`.
fn key_of(line: &str) -> Option<(&str, usize, String)> {
    let bytes = line.as_bytes();
    let (key, after_key) = match *bytes.first()? {
        quote @ (b'"' | b'\'') => {
            let mut at = 1;
            loop {
                match *bytes.get(at)? {
                    b'\\' if quote == b'"' => at += 2,
                    b'\'' if quote == b'\'' && bytes.get(at + 1) == Some(&b'\'') => at += 2,
                    b if b == quote => break,
                    _ => at += 1,
                }
            }
            let inside = &line[1..at];
            let key = if quote == b'"' {
                inside.replace("\\\"", "\"").replace("\\\\", "\\")
            } else {
                inside.replace("''", "'")
            };
            (key, at + 1)
        }
        b'[' | b'{' | b'!' | b'&' | b'*' | b'|' | b'>' | b'%' | b'@' | b'`' | b'?' | b'-' => {
            return None
        }
        _ => {
            let mut colon = None;
            for (at, c) in line.char_indices() {
                if c == ':'
                    && line[at + 1..]
                        .chars()
                        .next()
                        .is_none_or(char::is_whitespace)
                {
                    colon = Some(at);
                    break;
                }
            }
            let at = colon?;
            (line[..at].trim_end().to_owned(), at)
        }
    };
    let colon = after_key + line[after_key..].find(|c: char| c != ' ' && c != '\t')?;
    (line.as_bytes()[colon] == b':').then(|| (&line[..colon], colon, key))
}

/// The entries of a block, or `None` when a line is not one this can place.
fn entries<'a>(lines: &[&'a str]) -> Option<Vec<Entry<'a>>> {
    let mut entries: Vec<Entry<'a>> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if is_gap(line) {
            continue;
        }
        let indented = line.starts_with([' ', '\t']);
        let item = line.starts_with("- ") || line.trim_end() == "-";
        if indented || item {
            // The value of the key above, in the lines under it.
            entries.last()?;
            continue;
        }
        let (token, _, key) = key_of(line)?;
        entries.push(Entry {
            key,
            token,
            start: index,
            end: lines.len(),
        });
    }
    // Each entry ends where the next begins, less the comments and blank lines between them,
    // which belong to neither.
    for at in 0..entries.len() {
        let mut end = entries.get(at + 1).map_or(lines.len(), |next| next.start);
        while end > entries[at].start + 1 && is_gap(lines[end - 1]) {
            end -= 1;
        }
        entries[at].end = end;
    }
    Some(entries)
}

/// What follows `key:` when a value is written: ` text` and a line break, or a line break and
/// the lines under the key.
fn after_colon(value: &Value) -> Option<String> {
    let mut one = Properties::new();
    one.set("k", value.clone());
    write(&one).strip_prefix("k:").map(str::to_owned)
}

/// A value that is no list or map, written on one line.
fn one_line(value: &Value) -> Option<String> {
    if matches!(value, Value::List(_) | Value::Map(_)) {
        return None;
    }
    let text = after_colon(value)?;
    let item = text.strip_prefix(' ')?.strip_suffix('\n')?;
    (!item.contains('\n')).then(|| item.to_owned())
}

/// A comment that ends a line after a value written on one line, with the space before it, and
/// nothing when there is none (a `#` inside quotes is not one).
fn comment_after(rest_of_line: &str) -> &str {
    let line = rest_of_line.trim_end_matches(['\n', '\r']);
    let mut quote: Option<char> = None;
    let mut escaped = false;
    // A quote opens a quoted text only where a value or a list item begins.
    let mut opens = true;
    let mut spaces_from = None;
    for (at, c) in line.char_indices() {
        if let Some(open) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' && open == '"' {
                escaped = true;
            } else if c == open {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' if opens => quote = Some(c),
            '#' if spaces_from.is_some() => return &line[spaces_from.unwrap_or(at)..],
            _ => {}
        }
        if c.is_whitespace() {
            spaces_from.get_or_insert(at);
        } else {
            spaces_from = None;
        }
        opens = c.is_whitespace() || matches!(c, '[' | ',' | '{');
    }
    ""
}

/// The text of an entry whose value changed, written in the way the old one was where it can be.
fn changed(entry: &Entry<'_>, lines: &[&str], value: &Value, eol: &str) -> Option<String> {
    let first = lines[entry.start];
    let rest_of_first = &first[entry.token.len() + 1..];
    let value_text = rest_of_first.trim_start();
    // A list written on one line stays on one line.
    let flow_items = match value {
        Value::List(items) if value_text.starts_with('[') => {
            items.iter().map(one_line).collect::<Option<Vec<String>>>()
        }
        _ => None,
    };
    let mut rest = match flow_items {
        Some(items) => format!(" [{}]\n", items.join(", ")),
        None => after_colon(value)?,
    };
    // A comment after a value that was on one line stays after one that still is.
    let one_line_before = entry.end - entry.start == 1;
    if one_line_before && rest.starts_with(' ') && rest.matches('\n').count() == 1 {
        let comment = comment_after(rest_of_first);
        if !comment.is_empty() {
            rest = format!("{}{comment}\n", rest.trim_end_matches('\n'));
        }
    }
    Some(ending(format!("{}:{rest}", entry.token), eol))
}

/// The text of an entry that is new.
fn added(key: &str, value: &Value, eol: &str) -> String {
    let mut one = Properties::new();
    one.set(key, value.clone());
    whole(&one, eol)
}

/// The block with `new` in place of `old`, line by line; `None` when that cannot be done.
fn edit(front: &str, old: &Properties, new: &Properties, eol: &str) -> Option<String> {
    let lines: Vec<&str> = front.split_inclusive('\n').collect();
    let entries = entries(&lines)?;
    if !entries.iter().map(|e| e.key.as_str()).eq(old.keys()) {
        return None;
    }
    // Keys that stay stay in their order: moving one is not something to do line by line.
    if !new
        .keys()
        .filter(|k| old.contains(k))
        .eq(old.keys().filter(|k| new.contains(k)))
    {
        return None;
    }
    // What each entry becomes: nothing for one that is gone, text for one that changed.
    let mut becomes: Vec<Option<String>> = Vec::with_capacity(entries.len());
    for entry in &entries {
        becomes.push(match new.get(&entry.key) {
            None => Some(String::new()),
            Some(value) if old.get(&entry.key) == Some(value) => None,
            Some(value) => Some(changed(entry, &lines, value, eol)?),
        });
    }
    // Where each new key goes: before the first later key there was, else at the end.
    let keys: Vec<&str> = new.keys().collect();
    let mut inserts: Vec<(usize, String)> = Vec::new();
    for (at, key) in keys.iter().enumerate() {
        if old.contains(key) {
            continue;
        }
        let before = keys[at + 1..]
            .iter()
            .find_map(|k| entries.iter().position(|e| e.key == *k));
        let line = match before {
            Some(0) => 0,
            Some(entry) => entries[entry - 1].end,
            None => lines.len(),
        };
        inserts.push((line, added(key, new.get(key)?, eol)));
    }
    let mut out = String::with_capacity(front.len() + 64);
    for index in 0..=lines.len() {
        if index == lines.len() && !out.is_empty() && !out.ends_with('\n') {
            out.push_str(eol);
        }
        for (_, text) in inserts.iter().filter(|(line, _)| *line == index) {
            out.push_str(text);
        }
        let Some(line) = lines.get(index) else { break };
        match entries
            .iter()
            .position(|e| (e.start..e.end).contains(&index))
        {
            Some(at) => match &becomes[at] {
                Some(text) if index == entries[at].start => out.push_str(text),
                Some(_) => {}
                None => out.push_str(line),
            },
            None => out.push_str(line),
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(yaml: &str) -> Properties {
        parse(yaml).unwrap()
    }

    /// The block after `change` is made to its properties.
    fn after(front: &str, change: impl FnOnce(&mut Properties)) -> String {
        let mut new = props(front);
        change(&mut new);
        amend(front, &new, "\n")
    }

    const BLOCK: &str = "# a note about waves\n\
        title:   'Spaced'   # keep this\n\
        tags: [physics, waves]   # two\n\
        \n\
        # who wrote it\n\
        author: \"Ada: Lovelace\"\n\
        nested:\n  \
          inner: [1, 2]\n  \
          # inside\n  \
          text: |\n    one\n    two\n\
        related:\n  - \"[[A]]\"\n  - \"[[B]]\"\n\
        # the end\n";

    #[test]
    fn what_is_not_changed_is_written_back_exactly() {
        assert_eq!(after(BLOCK, |_| {}), BLOCK);
    }

    #[test]
    fn a_changed_property_is_written_again_and_the_others_stay_as_they_were() {
        let text = after(BLOCK, |p| {
            p.set(
                "related",
                vec!["[[A]]".to_owned(), "[[B]]".to_owned(), "[[C]]".to_owned()],
            );
        });
        assert_eq!(
            text,
            BLOCK.replace("  - \"[[B]]\"\n", "  - \"[[B]]\"\n  - \"[[C]]\"\n")
        );
    }

    #[test]
    fn a_list_on_one_line_stays_on_one_line_with_its_comment() {
        let text = after(BLOCK, |p| {
            p.set(
                "tags",
                vec!["physics".to_owned(), "waves".to_owned(), "x: y".to_owned()],
            );
        });
        assert_eq!(
            text,
            BLOCK.replace(
                "tags: [physics, waves]   # two\n",
                "tags: [physics, waves, \"x: y\"]   # two\n"
            )
        );
    }

    #[test]
    fn a_key_keeps_the_quotes_it_was_written_with() {
        let front = "\"my key\" : old\nother: 1\n";
        let text = after(front, |p| p.set("my key", "new"));
        assert_eq!(text, "\"my key\" : new\nother: 1\n");
    }

    #[test]
    fn a_removed_property_takes_its_lines_and_leaves_the_comments_around_it() {
        let text = after(BLOCK, |p| {
            p.remove("nested");
        });
        assert_eq!(
            text,
            BLOCK.replace(
                "nested:\n  inner: [1, 2]\n  # inside\n  text: |\n    one\n    two\n",
                ""
            )
        );
    }

    #[test]
    fn a_new_property_goes_before_the_first_later_one_there_and_after_its_comments() {
        let front = "# header\nid: x\n\n# when\ncreated: 2026-11-10\nupdated: 2026-11-10\n";
        let text = after(front, |p| {
            p.set_before(
                "related",
                Value::from(vec!["[[A]]".to_owned()]),
                &["created"],
            );
        });
        assert_eq!(
            text,
            "# header\nid: x\nrelated:\n  - \"[[A]]\"\n\n# when\ncreated: 2026-11-10\nupdated: 2026-11-10\n"
        );
        let at_the_end = after("a: 1\n# last words\n", |p| p.set("b", 2_i64));
        assert_eq!(at_the_end, "a: 1\n# last words\nb: 2\n");
        let first = after("# header\na: 1\n", |p| {
            p.set_before("z", Value::from(0_i64), &["a"]);
        });
        assert_eq!(first, "z: 0\n# header\na: 1\n");
    }

    #[test]
    fn a_first_property_goes_in_an_empty_block() {
        assert_eq!(after("", |p| p.set("a", "b")), "a: b\n");
        assert_eq!(
            after("# nothing yet\n", |p| p.set("a", "b")),
            "# nothing yet\na: b\n"
        );
    }

    #[test]
    fn windows_line_endings_are_kept_in_what_is_written() {
        let front = "title: x\r\ntags: [a]\r\n";
        let mut new = props(front);
        new.set("tags", vec!["a".to_owned(), "b".to_owned()]);
        new.set("count", 1_i64);
        assert_eq!(
            amend(front, &new, "\r\n"),
            "title: x\r\ntags: [a, b]\r\ncount: 1\r\n"
        );
    }

    #[test]
    fn a_block_that_cannot_be_edited_line_by_line_is_written_whole() {
        // A flow map, a repeated key and moved keys are all written afresh, and read back the same.
        for (front, change) in [
            ("{a: 1, b: 2}\n", ("a", 5_i64)),
            ("a: 1\na: 2\nb: 3\n", ("b", 9)),
        ] {
            let mut new = props(front);
            new.set(change.0, change.1);
            let text = amend(front, &new, "\n");
            assert_eq!(props(&text), new, "{front:?} became {text:?}");
        }
        let front = "a: 1\nb: 2\n";
        let mut new = Properties::new();
        new.set("b", 2_i64);
        new.set("a", 1_i64);
        assert_eq!(props(&amend(front, &new, "\n")), new);
    }

    #[test]
    fn a_comment_after_a_value_that_is_not_one_is_not_taken_for_one() {
        assert_eq!(comment_after(" 'a # b'  # c\n"), "  # c");
        assert_eq!(comment_after(" \"it's # not\" # c"), " # c");
        assert_eq!(comment_after(" don't # c"), " # c");
        assert_eq!(comment_after(" a#b"), "");
        assert_eq!(comment_after(" [a, 'b # c']"), "");
    }

    #[test]
    fn a_key_is_told_from_the_text_before_its_colon() {
        assert_eq!(key_of("tags: [a]").map(|k| k.2).as_deref(), Some("tags"));
        assert_eq!(key_of("a:b: c").map(|k| k.2).as_deref(), Some("a:b"));
        assert_eq!(key_of("'it''s': 1").map(|k| k.2).as_deref(), Some("it's"));
        assert_eq!(
            key_of("\"q\\\"x\": 1").map(|k| k.2).as_deref(),
            Some("q\"x")
        );
        assert_eq!(key_of("empty:").map(|k| k.2).as_deref(), Some("empty"));
        for not_a_key in ["- item", "[a]: 1", "plain text", "? complex", "\"open: 1"] {
            assert!(key_of(not_a_key).is_none(), "{not_a_key}");
        }
    }
}
