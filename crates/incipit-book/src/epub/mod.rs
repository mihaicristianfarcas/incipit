//! An EPUB as blocks a window lays out itself: headings, paragraphs of styled text, text set
//! apart (quotations, items of lists, code, captions, rows of tables) and pictures, in reading
//! order, with the links between them and the book's own table of contents. The book's HTML is
//! material, never run or shown as HTML: only its text, a few styles (italic, bold), its
//! pictures and where its links lead are kept (`walk`).
//!
//! A book is read as much as it can be: a chapter that is missing or cannot be read is left out
//! and the rest is read, so one broken file does not lose the book; a book with no text at all
//! is refused, and so is one locked with DRM, whose text is not there to read. The contents are
//! the book's own (EPUB 3's navigation document, else EPUB 2's NCX), each line led to the block
//! it names; a book whose contents lead nowhere is given its headings instead.

mod math;
mod walk;
mod xhtml;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use scraper::{ElementRef, Html, Selector};

use crate::pdf::Entry;
pub use walk::CELLS;
use xhtml::{open_closed, resolve, Href};

pub struct Book {
    pub title: String,
    /// Who the book says wrote it (its first `dc:creator`), when it says.
    pub author: Option<String>,
    pub blocks: Vec<Block>,
    /// The book's table of contents, each line leading to a block.
    pub contents: Vec<Entry>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Heading {
        level: u8,
        text: Rich,
    },
    Paragraph(Rich),
    /// Text set apart from the paragraphs.
    Text {
        kind: Kind,
        text: Rich,
    },
    /// A picture on its own line: its file's path in the book and its bytes.
    Image {
        name: String,
        bytes: Vec<u8>,
    },
}

/// How text set apart from the paragraphs is set apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Quoted from elsewhere (a `blockquote`).
    Quote,
    /// An item of a list, or what is said of a term, `depth` lists deep from 1. An item's mark
    /// ("•", "3. ") begins its first block's text.
    Item { depth: u8 },
    /// Text whose spaces and line breaks are as written: code, a poem set as written.
    Pre,
    /// What a picture or a table shows.
    Caption,
    /// A row of a table, its cells apart by `CELLS`; a row of headers is bold.
    Row,
}

/// Text with the stretches of it that are italic or bold, and its links.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rich {
    pub text: String,
    pub spans: Vec<Span>,
    /// The links in the text, in order.
    pub links: Vec<Link>,
}

/// A styled stretch, in bytes of the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub italic: bool,
    pub bold: bool,
}

/// A stretch of text, in bytes, that leads somewhere.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub start: usize,
    pub end: usize,
    pub to: Target,
}

/// Where a link leads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A block of the book (a note, a figure, a chapter).
    Block(usize),
    /// A page on the web, by its `http` or `https` address.
    Web(String),
}

impl Block {
    /// The block's text, unless it is a picture.
    pub fn rich(&self) -> Option<&Rich> {
        match self {
            Block::Heading { text, .. } | Block::Paragraph(text) | Block::Text { text, .. } => {
                Some(text)
            }
            Block::Image { .. } => None,
        }
    }
}

/// The encryption EPUBs use to hide their fonts, which leaves the text readable.
const FONT_HIDING: &[&str] = &[
    "http://www.idpf.org/2008/embedding",
    "http://ns.adobe.com/pdf/enc#RC",
];

/// A file of the book, as its package lists it.
struct Item {
    path: String,
    media: String,
    properties: String,
}

impl Book {
    pub fn open(file: &Path) -> Result<Book> {
        let opened = std::fs::File::open(file).with_context(|| file.display().to_string())?;
        let mut zip = zip::ZipArchive::new(opened)
            .map_err(|e| anyhow!("{} is not an EPUB that can be read ({e})", file.display()))?;
        let mut read = |name: &str| -> Result<Vec<u8>> {
            let mut bytes = Vec::new();
            zip.by_name(name)
                .with_context(|| format!("{name} is not in the book"))?
                .read_to_end(&mut bytes)?;
            Ok(bytes)
        };
        let text = |bytes: Vec<u8>| String::from_utf8_lossy(&bytes).into_owned();

        if let Ok(encryption) = read("META-INF/encryption.xml") {
            if locked(&text(encryption)) {
                bail!(
                    "This book is locked with DRM, so its text cannot be read. A copy without \
                     DRM, from the shop or the publisher, opens."
                );
            }
        }

        // container.xml names the package file; the package lists the files and their order.
        let container = Html::parse_document(&text(read("META-INF/container.xml")?));
        let package_path = container
            .select(&selector("rootfile"))
            .find_map(|e| e.value().attr("full-path"))
            .ok_or_else(|| anyhow!("the book names no package file"))?
            .to_string();
        let package = Html::parse_document(&open_closed(&text(read(&package_path)?)));
        let items: HashMap<String, Item> = package
            .select(&selector("manifest item"))
            .filter_map(|e| {
                let e = e.value();
                let Some(Href::Inside(path, _)) = resolve(&package_path, e.attr("href")?) else {
                    return None;
                };
                let item = Item {
                    path,
                    media: e
                        .attr("media-type")
                        .unwrap_or_default()
                        .to_ascii_lowercase(),
                    properties: e.attr("properties").unwrap_or_default().to_owned(),
                };
                Some((e.attr("id")?.to_owned(), item))
            })
            .collect();
        let said = |query: &str| {
            package
                .select(&selector(query))
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())
                .filter(|t| !t.is_empty())
        };
        let title = said("dc\\:title, title").unwrap_or_default();
        let author = said("dc\\:creator, creator");
        let spine: Vec<&Item> = package
            .select(&selector("spine itemref"))
            .filter_map(|e| items.get(e.value().attr("idref")?))
            .collect();
        let ncx = package
            .select(&selector("spine"))
            .next()
            .and_then(|s| items.get(s.value().attr("toc")?))
            .or_else(|| {
                items
                    .values()
                    .find(|i| i.media == "application/x-dtbncx+xml")
            })
            .map(|i| i.path.clone());
        let nav = items
            .values()
            .find(|i| i.properties.split_whitespace().any(|p| p == "nav"))
            .map(|i| i.path.clone());

        let mut pictures = |path: &str| read(path).ok();
        let mut chapters: Vec<(String, Option<String>)> = Vec::new();
        for item in &spine {
            if item.media.starts_with("image/") {
                chapters.push((item.path.clone(), None));
                continue;
            }
            // A chapter that cannot be read is left out; the rest of the book is still read.
            if let Some(bytes) = pictures(&item.path) {
                chapters.push((item.path.clone(), Some(text(bytes))));
            }
        }
        let navigation = nav
            .as_deref()
            .and_then(|path| Some((path.to_owned(), text(pictures(path)?))));
        let ncx = ncx
            .as_deref()
            .and_then(|path| Some((path.to_owned(), text(pictures(path)?))));

        let mut walker = walk::Walker::new(&mut pictures);
        for (path, chapter) in &chapters {
            match chapter {
                None => walker.picture_file(path),
                Some(chapter) => {
                    let page = Html::parse_document(&open_closed(chapter));
                    if let Some(body) = page.select(&selector("body")).next() {
                        walker.chapter(path, body);
                    }
                }
            }
        }
        let (blocks, starts) = walker.finish();
        if !blocks.iter().any(|b| b.rich().is_some()) {
            bail!("{} has no text that could be read", file.display());
        }
        let lines = navigation
            .map(|(path, nav)| nav_lines(&path, &nav))
            .filter(|lines| !lines.is_empty())
            .or_else(|| ncx.map(|(path, ncx)| ncx_lines(&path, &ncx)))
            .unwrap_or_default();
        let mut contents = led(&lines, &starts);
        if contents.is_empty() {
            contents = headings(&blocks);
        }
        Ok(Book {
            title,
            author,
            blocks,
            contents,
        })
    }
}

fn selector(query: &str) -> Selector {
    Selector::parse(query).expect("the selectors written here are valid")
}

/// Whether a book's `encryption.xml` hides the text itself, not only its fonts.
fn locked(encryption: &str) -> bool {
    let page = Html::parse_document(encryption);
    page.select(&selector("encrypteddata")).any(|data| {
        let method = data
            .select(&selector("encryptionmethod"))
            .next()
            .and_then(|m| m.value().attr("algorithm"))
            .unwrap_or_default();
        !FONT_HIDING.contains(&method)
    })
}

/// A line of the contents as the book writes it: its title, how deep, and where it leads (a
/// file, or `file#id`).
type Line = (String, usize, String);

/// The lines of EPUB 3's navigation document (the file at `path`): its `toc` navigation, or its
/// first when none says it is the table of contents.
fn nav_lines(path: &str, nav: &str) -> Vec<Line> {
    let page = Html::parse_document(&open_closed(nav));
    let navs: Vec<ElementRef> = page.select(&selector("nav")).collect();
    let toc = navs
        .iter()
        .find(|n| {
            n.value()
                .attr("epub:type")
                .is_some_and(|t| t.split_whitespace().any(|t| t == "toc"))
        })
        .or(navs.first());
    let mut lines = Vec::new();
    if let Some(list) = toc.and_then(|toc| {
        toc.select(&selector("ol"))
            .next()
            .or(toc.select(&selector("ul")).next())
    }) {
        nav_list(path, list, 0, &mut lines);
    }
    lines
}

fn nav_list(path: &str, list: ElementRef, depth: usize, lines: &mut Vec<Line>) {
    if depth > 8 || lines.len() >= 5000 {
        return;
    }
    for item in list
        .children()
        .filter_map(ElementRef::wrap)
        .filter(|e| e.value().name() == "li")
    {
        let label = item
            .children()
            .filter_map(ElementRef::wrap)
            .find(|e| matches!(e.value().name(), "a" | "span"));
        if let Some(label) = label {
            let title = squash(&label.text().collect::<String>());
            if let Some(Href::Inside(file, id)) =
                label.value().attr("href").and_then(|h| resolve(path, h))
            {
                lines.push((title, depth, key(file, id)));
            }
        }
        for sub in item
            .children()
            .filter_map(ElementRef::wrap)
            .filter(|e| matches!(e.value().name(), "ol" | "ul"))
        {
            nav_list(path, sub, depth + 1, lines);
        }
    }
}

/// The lines of EPUB 2's NCX (the file at `path`).
fn ncx_lines(path: &str, ncx: &str) -> Vec<Line> {
    fn points(path: &str, parent: ElementRef, depth: usize, lines: &mut Vec<Line>) {
        if depth > 8 || lines.len() >= 5000 {
            return;
        }
        for point in parent
            .children()
            .filter_map(ElementRef::wrap)
            .filter(|e| e.value().name() == "navpoint")
        {
            let title = point
                .select(&selector("navlabel text"))
                .next()
                .map(|t| squash(&t.text().collect::<String>()))
                .unwrap_or_default();
            let src = point
                .children()
                .filter_map(ElementRef::wrap)
                .find(|e| e.value().name() == "content")
                .and_then(|c| c.value().attr("src"));
            if let Some(Href::Inside(file, id)) = src.and_then(|s| resolve(path, s)) {
                lines.push((title, depth, key(file, id)));
            }
            points(path, point, depth + 1, lines);
        }
    }
    let page = Html::parse_document(&open_closed(ncx));
    let mut lines = Vec::new();
    if let Some(map) = page.select(&selector("navmap")).next() {
        points(path, map, 0, &mut lines);
    }
    lines
}

fn key(file: String, id: Option<String>) -> String {
    match id {
        Some(id) => format!("{file}#{id}"),
        None => file,
    }
}

fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The lines of the contents that lead to a block, each with that block. A line that names a
/// place the reading of the book did not keep (an `id` on nothing that was read) leads to the
/// start of its file.
fn led(lines: &[Line], starts: &HashMap<String, usize>) -> Vec<Entry> {
    lines
        .iter()
        .filter(|(title, _, _)| !title.is_empty())
        .filter_map(|(title, depth, to)| {
            let file = to.split('#').next().unwrap_or_default();
            let block = starts.get(to).or_else(|| starts.get(file))?;
            Some(Entry {
                title: title.clone(),
                depth: *depth,
                page: *block,
                y: None,
            })
        })
        .collect()
}

/// The contents a book without its own has: its headings of the first three levels, each at
/// its block.
pub fn headings(blocks: &[Block]) -> Vec<Entry> {
    blocks
        .iter()
        .enumerate()
        .filter_map(|(index, block)| match block {
            Block::Heading { level, text } if *level <= 3 => Some(Entry {
                title: text.text.clone(),
                depth: (*level as usize).saturating_sub(1),
                page: index,
                y: None,
            }),
            _ => None,
        })
        .collect()
}
