//! YAML frontmatter: the `---` block at the top of a Markdown file that holds its properties, the
//! way Obsidian reads and writes it. A file without the block, or with an opening `---` and no
//! closing one, has no properties and is all body, as in Obsidian.
//!
//! Parsing is serde-saphyr's (YAML 1.2, with limits on size and aliases, and errors instead of
//! panics on any input). A name written twice keeps its last value, as Obsidian does. Writing
//! puts the properties back in their order; only a document whose properties changed is written
//! anew, so the comments and quoting of a block nobody touched survive.

use crate::value::{Properties, Value};

/// A file's text cut into its frontmatter (without the `---` lines) and its body.
#[derive(Debug, PartialEq)]
pub(crate) struct Split<'a> {
    pub front: Option<&'a str>,
    pub body: &'a str,
}

/// The fence line, without its line ending.
const FENCE: &str = "---";

/// The line that starts at `at`, without its ending, and where the next one starts.
fn line_at(text: &str, at: usize) -> (&str, usize) {
    match text[at..].find('\n') {
        Some(end) => (text[at..at + end].trim_end_matches('\r'), at + end + 1),
        None => (&text[at..], text.len()),
    }
}

pub(crate) fn split(text: &str) -> Split<'_> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let all_body = Split {
        front: None,
        body: text,
    };
    let (first, mut at) = line_at(text, 0);
    if first.trim_end() != FENCE || at == text.len() && !text.ends_with('\n') {
        return all_body;
    }
    let start = at;
    while at < text.len() {
        let (line, next) = line_at(text, at);
        if line.trim_end() == FENCE {
            return Split {
                front: Some(&text[start..at]),
                body: &text[next..],
            };
        }
        at = next;
    }
    all_body
}

/// The properties in a frontmatter block, or what is wrong with it, in words.
pub(crate) fn parse(front: &str) -> Result<Properties, String> {
    if front.trim().is_empty() {
        return Ok(Properties::new());
    }
    let mut options = serde_saphyr::Options::default();
    options.duplicate_keys = serde_saphyr::DuplicateKeyPolicy::LastWins;
    options.with_snippet = false;
    match serde_saphyr::from_str_with_options::<Value>(front, options) {
        Ok(Value::Map(properties)) => Ok(properties),
        Ok(Value::Null) => Ok(Properties::new()),
        Ok(_) => Err("they are not a list of names and values".to_owned()),
        Err(error) => Err(error.to_string()),
    }
}

/// The YAML for a set of properties, ending with a line break; empty when there are none.
pub(crate) fn write(properties: &Properties) -> String {
    if properties.is_empty() {
        return String::new();
    }
    // List items indented under their key, as Obsidian writes them.
    let mut options = serde_saphyr::SerializerOptions::default();
    options.compact_list_indent = false;
    // Properties hold only texts, numbers, booleans, lists and maps with text names, which every
    // YAML writer can write; a failure here would be a bug in the writer.
    let mut yaml = serde_saphyr::to_string_with_options(properties, options)
        .expect("properties are plain YAML values with text names");
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml
}

/// A whole file: the frontmatter block (when there is one) and the body.
pub(crate) fn compose(front: &str, body: &str) -> String {
    let mut text = String::with_capacity(front.len() + body.len() + 8);
    if !front.is_empty() {
        text.push_str("---\n");
        text.push_str(front);
        if !front.ends_with('\n') {
            text.push('\n');
        }
        text.push_str("---\n");
    }
    text.push_str(body);
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_with_a_block_at_the_top_has_properties_and_a_body() {
        let text = "---\ntitle: Waves\ntags:\n  - physics\n---\nThe body.\n";
        let split = split(text);
        assert_eq!(split.front, Some("title: Waves\ntags:\n  - physics\n"));
        assert_eq!(split.body, "The body.\n");
        let properties = parse(split.front.unwrap()).unwrap();
        assert_eq!(properties.text("title").as_deref(), Some("Waves"));
        assert_eq!(properties.texts("tags"), vec!["physics"]);
    }

    #[test]
    fn a_block_that_is_never_closed_is_part_of_the_body() {
        let text = "---\ntitle: Waves\nno closing fence\n";
        assert_eq!(split(text).front, None);
        assert_eq!(split(text).body, text);
    }

    #[test]
    fn windows_line_endings_and_a_byte_order_mark_are_read() {
        let split = split("\u{feff}---\r\ntitle: Waves\r\n---\r\nBody\r\n");
        assert_eq!(split.front, Some("title: Waves\r\n"));
        assert_eq!(split.body, "Body\r\n");
    }

    #[test]
    fn malformed_frontmatter_gives_an_error_not_a_panic() {
        for bad in [
            "title: [unclosed\n",
            "key: value\n  bad indent: here\n",
            "- a list\n- not a map\n",
            "&a [*a]\n",
            "\"unterminated\n",
            ": : :\n",
        ] {
            assert!(parse(bad).is_err(), "{bad:?} was read");
        }
    }

    #[test]
    fn written_properties_read_back_in_the_same_order() {
        let mut properties = Properties::new();
        properties.set("zeta", "last letter");
        properties.set("tags", vec!["a".to_owned(), "b: c".to_owned()]);
        properties.set("count", 12_i64);
        properties.set("done", true);
        properties.set("when", "2026-10-14");
        properties.set("yes", "yes");
        let yaml = write(&properties);
        let back = parse(&yaml).unwrap();
        assert_eq!(back, properties, "the YAML was:\n{yaml}");
        assert_eq!(
            back.keys().collect::<Vec<_>>(),
            ["zeta", "tags", "count", "done", "when", "yes"]
        );
    }

    #[test]
    fn a_name_written_twice_keeps_its_last_value() {
        let properties = parse("a: 1\na: 2\n").unwrap();
        assert_eq!(properties.get("a"), Some(&Value::Int(2)));
    }
}
