//! An EPUB as blocks a window lays out itself: headings, paragraphs of styled text and pictures, in
//! reading order. The book's HTML is material, never run or shown as HTML: only its text, a few
//! styles (italic, bold) and its pictures are kept.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use scraper::{ElementRef, Html, Node, Selector};

pub struct Book {
    pub title: String,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Heading {
        level: u8,
        text: Rich,
    },
    Paragraph(Rich),
    /// A picture on its own line: its file's name in the book and its bytes.
    Image {
        name: String,
        bytes: Vec<u8>,
    },
}

/// Text with the stretches of it that are italic or bold.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rich {
    pub text: String,
    pub spans: Vec<Span>,
}

/// A styled stretch, in bytes of the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub italic: bool,
    pub bold: bool,
}

impl Book {
    pub fn open(file: &Path) -> Result<Book> {
        let mut zip = zip::ZipArchive::new(
            std::fs::File::open(file).with_context(|| file.display().to_string())?,
        )?;
        let mut read = |name: &str| -> Result<Vec<u8>> {
            let mut bytes = Vec::new();
            zip.by_name(name)
                .with_context(|| format!("{name} is not in the book"))?
                .read_to_end(&mut bytes)?;
            Ok(bytes)
        };
        let text = |bytes: Vec<u8>| String::from_utf8_lossy(&bytes).into_owned();

        // container.xml names the package file; the package lists the files and their order.
        let container = Html::parse_document(&text(read("META-INF/container.xml")?));
        let rootfile = Selector::parse("rootfile").unwrap();
        let package_path = container
            .select(&rootfile)
            .find_map(|e| e.value().attr("full-path"))
            .ok_or_else(|| anyhow!("the book names no package file"))?
            .to_string();
        let folder = package_path
            .rsplit_once('/')
            .map(|(dir, _)| format!("{dir}/"))
            .unwrap_or_default();
        let package = Html::parse_document(&text(read(&package_path)?));
        let hrefs: HashMap<&str, &str> = package
            .select(&Selector::parse("item").unwrap())
            .filter_map(|e| Some((e.value().attr("id")?, e.value().attr("href")?)))
            .collect();
        let title = package
            .select(&Selector::parse("dc\\:title, title").unwrap())
            .next()
            .map(|e| e.text().collect::<String>().trim().to_string())
            .unwrap_or_default();
        let spine: Vec<String> = package
            .select(&Selector::parse("itemref").unwrap())
            .filter_map(|e| {
                hrefs
                    .get(e.value().attr("idref")?)
                    .map(|href| format!("{folder}{href}"))
            })
            .collect();

        let mut blocks = Vec::new();
        for path in spine {
            let chapter = Html::parse_document(&text(read(&path)?));
            let Some(body) = chapter.select(&Selector::parse("body").unwrap()).next() else {
                continue;
            };
            let base = path
                .rsplit_once('/')
                .map(|(dir, _)| format!("{dir}/"))
                .unwrap_or_default();
            let mut picture = |src: &str| read(&format!("{base}{src}")).ok();
            walk(body, &mut blocks, &mut picture);
        }
        Ok(Book { title, blocks })
    }
}

const SKIPPED: &[&str] = &["script", "style", "nav", "head", "svg", "math", "table"];
const INLINE: &[&str] = &[
    "a", "abbr", "b", "br", "cite", "code", "em", "i", "small", "span", "strong", "sub", "sup",
    "u", "img",
];

/// The blocks under `element`, in order.
fn walk(
    element: ElementRef,
    blocks: &mut Vec<Block>,
    picture: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
) {
    let name = element.value().name();
    if SKIPPED.contains(&name) {
        return;
    }
    let level = match name {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" | "h5" | "h6" => Some(4),
        _ => None,
    };
    let holds_blocks = element
        .children()
        .filter_map(ElementRef::wrap)
        .any(|c| !INLINE.contains(&c.value().name()));
    if level.is_some() || name == "p" || !holds_blocks {
        // A picture alone on its line is a block of its own.
        let mut inside = element
            .children()
            .filter(|n| !matches!(n.value(), Node::Text(t) if t.trim().is_empty()));
        if let (Some(only), None) = (inside.next(), inside.next()) {
            if let Some(src) = ElementRef::wrap(only)
                .filter(|e| e.value().name() == "img")
                .and_then(|e| e.value().attr("src"))
            {
                if let Some(bytes) = picture(src) {
                    blocks.push(Block::Image {
                        name: src.to_string(),
                        bytes,
                    });
                }
                return;
            }
        }
        let mut rich = Rich::default();
        inline(element, false, false, &mut rich);
        let trimmed = rich.text.trim_end().len();
        rich.text.truncate(trimmed);
        rich.spans.retain_mut(|s| {
            s.end = s.end.min(trimmed);
            s.start < s.end
        });
        if !rich.text.is_empty() {
            blocks.push(match level {
                Some(level) => Block::Heading { level, text: rich },
                None => Block::Paragraph(rich),
            });
        }
        return;
    }
    for child in element.children().filter_map(ElementRef::wrap) {
        walk(child, blocks, picture);
    }
}

/// The text under `element` added to `rich`, with white space runs as one space.
fn inline(element: ElementRef, italic: bool, bold: bool, rich: &mut Rich) {
    for node in element.children() {
        match node.value() {
            Node::Text(text) => {
                let start = rich.text.len();
                for word in text.split_inclusive(char::is_whitespace) {
                    let (word, spaced) = match word.trim_end() {
                        w if w.len() < word.len() => (w, true),
                        w => (w, false),
                    };
                    if word.is_empty() && rich.text.is_empty() {
                        continue;
                    }
                    rich.text.push_str(word);
                    if spaced && !rich.text.ends_with(' ') {
                        rich.text.push(' ');
                    }
                }
                if (italic || bold) && rich.text.len() > start {
                    rich.spans.push(Span {
                        start,
                        end: rich.text.len(),
                        italic,
                        bold,
                    });
                }
            }
            Node::Element(e) => {
                let Some(child) = ElementRef::wrap(node) else {
                    continue;
                };
                match e.name() {
                    name if SKIPPED.contains(&name) => {}
                    "br" => {
                        if !rich.text.is_empty() && !rich.text.ends_with(' ') {
                            rich.text.push(' ');
                        }
                    }
                    // A picture in a line (a formula, mostly): what it says it shows.
                    "img" => rich
                        .text
                        .push_str(e.attr("alt").or(e.attr("title")).unwrap_or("▢")),
                    "i" | "em" | "cite" => inline(child, true, bold, rich),
                    "b" | "strong" => inline(child, italic, true, rich),
                    _ => inline(child, italic, bold, rich),
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocks(html: &str) -> Vec<Block> {
        let page = Html::parse_document(html);
        let body = page
            .select(&Selector::parse("body").unwrap())
            .next()
            .unwrap();
        let mut blocks = Vec::new();
        walk(body, &mut blocks, &mut |_| None);
        blocks
    }

    #[test]
    fn markup_becomes_blocks_of_styled_text() {
        let out = blocks("<body><div><h2>II<br/><span>Space</span></h2><p>It is <i>not</i>\n clear.</p></div></body>");
        let Block::Heading { level: 2, text } = &out[0] else {
            panic!("{out:?}")
        };
        assert_eq!(text.text, "II Space");
        let Block::Paragraph(p) = &out[1] else {
            panic!("{out:?}")
        };
        assert_eq!(p.text, "It is not clear.");
        assert_eq!(
            p.spans,
            vec![Span {
                start: 6,
                end: 9,
                italic: true,
                bold: false
            }]
        );
    }

    #[test]
    fn scripts_are_dropped() {
        let out = blocks(
            "<body><p>Read me.</p><script>alert(1)</script><p onclick=\"x()\">And me.</p></body>",
        );
        assert_eq!(out.len(), 2);
        assert!(out
            .iter()
            .all(|b| matches!(b, Block::Paragraph(p) if !p.text.contains("alert"))));
    }
}
