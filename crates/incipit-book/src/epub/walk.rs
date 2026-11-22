//! A chapter's body turned into blocks. Headings and paragraphs are the book's own; what is set
//! apart from them keeps a kind of its own, so the window can set it apart too: a quotation,
//! an item of a list (with its mark, "•" or "3.", at the start of its text, where it is seen and
//! read), text whose lines are as written (code), a caption and a row of a table (its cells
//! apart by " | "). Nothing in the book is dropped for its markup: text loose in a division
//! beside its paragraphs is a paragraph of its own, a table is its rows, and a formula is
//! written as Unicode math (`math`): from its MathML, else the TeX or spoken words the book gives
//! for it.
//!
//! A picture alone in its block, in a figure or among the text but larger than a line is a
//! block of its own; one the size of a word (a symbol, a formula drawn as a picture) is its
//! alternative text in the line, as math when that is TeX or spoken math. Links are kept with the
//! text they cover: one to a place in the book is resolved to the block it leads to once the whole
//! book is read (`Walker::finish`), one to the web is kept as its address, which the window may
//! open and never fetches.

use std::collections::HashMap;

use scraper::{ElementRef, Node};

use super::math;
use super::xhtml::{picture_size, resolve, Href};
use super::{Block, Kind, Link, Rich, Span, Target};

/// Elements whose content is not part of the text read.
const SKIPPED: &[&str] = &[
    "script", "style", "nav", "head", "noscript", "template", "button", "input", "select",
    "textarea", "iframe", "object", "audio", "video",
];
/// Elements that stay in a line of text.
const INLINE: &[&str] = &[
    "a", "abbr", "acronym", "b", "bdi", "bdo", "big", "br", "cite", "code", "data", "del", "dfn",
    "em", "font", "i", "img", "ins", "kbd", "label", "mark", "math", "q", "rb", "rp", "rt", "ruby",
    "s", "samp", "small", "span", "strike", "strong", "sub", "sup", "time", "tt", "u", "var",
    "wbr",
];
/// A picture no taller than this, in pixels, among the text is a symbol in the line, not a
/// picture of its own: about two lines of a book's text.
const IN_LINE: u32 = 48;
/// A picture smaller than this either way is a spacer or a rule, left out.
const SPECK: u32 = 4;
/// What stands between the cells of a table's row.
pub const CELLS: &str = " | ";

/// A link found in a block's text that leads to a place in the book, waiting until the whole
/// book is read to know which block that is.
struct Pending {
    block: usize,
    start: usize,
    end: usize,
    key: String,
}

/// Text being gathered for a block: its words and styles, and its links.
#[derive(Default)]
struct Run {
    rich: Rich,
    /// Links to places in the book: where they are in the text, and the place's key (`file` or
    /// `file#id`).
    inside: Vec<(usize, usize, String)>,
}

/// What the blocks being made are inside of.
#[derive(Clone, Copy, Default)]
struct Within {
    quote: bool,
    /// How many lists deep, from 1 in a list's item.
    item: u8,
}

/// Turns the chapters of a book into its blocks, one chapter after another, and keeps where each
/// file and each element with an `id` starts, so that links can be resolved once all are read.
pub struct Walker<'a> {
    pub blocks: Vec<Block>,
    /// Where each place a link can lead starts: a file's path, or `path#id`.
    pub starts: HashMap<String, usize>,
    pending: Vec<Pending>,
    /// The file being read, by its path from the book's root.
    file: String,
    /// The mark of the list item being read, until its first block takes it.
    mark: Option<String>,
    picture: &'a mut dyn FnMut(&str) -> Option<Vec<u8>>,
}

impl<'a> Walker<'a> {
    pub fn new(picture: &'a mut dyn FnMut(&str) -> Option<Vec<u8>>) -> Walker<'a> {
        Walker {
            blocks: Vec::new(),
            starts: HashMap::new(),
            pending: Vec::new(),
            file: String::new(),
            mark: None,
            picture,
        }
    }

    /// Reads one chapter's body, the file at `file`.
    pub fn chapter(&mut self, file: &str, body: ElementRef) {
        self.file = file.to_owned();
        self.starts
            .entry(file.to_owned())
            .or_insert(self.blocks.len());
        self.walk(body, Within::default());
    }

    /// A picture that is a chapter of its own (a cover, a plate).
    pub fn picture_file(&mut self, file: &str) {
        self.starts
            .entry(file.to_owned())
            .or_insert(self.blocks.len());
        if let Some(bytes) = (self.picture)(file) {
            self.blocks.push(Block::Image {
                name: file.to_owned(),
                bytes,
            });
        }
    }

    /// The blocks, with each link to a place in the book led to its block; a link to a place
    /// the book does not have is left as text.
    pub fn finish(mut self) -> (Vec<Block>, HashMap<String, usize>) {
        for pending in std::mem::take(&mut self.pending) {
            let Some(&to) = self.starts.get(&pending.key) else {
                continue;
            };
            if let Some(rich) = self.blocks.get_mut(pending.block).and_then(rich_mut) {
                rich.links.push(Link {
                    start: pending.start,
                    end: pending.end,
                    to: Target::Block(to),
                });
            }
        }
        for rich in self.blocks.iter_mut().filter_map(rich_mut) {
            rich.links.sort_by_key(|l| l.start);
        }
        (self.blocks, self.starts)
    }

    fn walk(&mut self, element: ElementRef, within: Within) {
        let name = element.value().name();
        if SKIPPED.contains(&name) || hidden(element) {
            return;
        }
        self.note_start(element);
        let level = match name {
            "h1" => Some(1),
            "h2" => Some(2),
            "h3" => Some(3),
            "h4" | "h5" | "h6" => Some(4),
            _ => None,
        };
        match name {
            "img" | "image" => return self.picture(element),
            "svg" => {
                // A picture drawn in SVG is left out, but one it only holds is a picture.
                for image in element
                    .descendants()
                    .filter_map(ElementRef::wrap)
                    .filter(|e| e.value().name() == "image")
                {
                    self.picture(image);
                }
                return;
            }
            "table" => return self.table(element, within),
            "pre" => return self.preformatted(element),
            "ul" | "ol" => return self.list(element, name == "ol", within),
            "blockquote" => {
                let quote = Within {
                    quote: true,
                    ..within
                };
                return self.children(element, quote, None);
            }
            "figcaption" | "caption" => {
                let mut run = Run::default();
                self.inline(element, Style::default(), &mut run, within);
                return self.push_text(run, None, Some(Kind::Caption), within);
            }
            "dt" => {
                let mut run = Run::default();
                self.inline(element, Style::BOLD, &mut run, within);
                return self.push_text(run, None, None, within);
            }
            "dd" => {
                let indented = Within {
                    item: within.item + 1,
                    ..within
                };
                return self.children(element, indented, None);
            }
            "hr" => return,
            _ => {}
        }
        // A picture alone in its block is a picture, whatever its size.
        if let Some(picture) = alone_picture(element) {
            return self.picture(picture);
        }
        if level.is_some() || name == "p" || !holds_blocks(element) {
            return self.children(element, within, level);
        }
        self.children(element, within, None)
    }

    /// The content of `element`: its runs of text, each a block (a heading of `level`, when it
    /// has one), and the blocks among them.
    fn children(&mut self, element: ElementRef, within: Within, level: Option<u8>) {
        let mut run = Run::default();
        for node in element.children() {
            match node.value() {
                Node::Text(text) => push_words(&mut run.rich, text, Style::default()),
                Node::Element(e) => {
                    let Some(child) = ElementRef::wrap(node) else {
                        continue;
                    };
                    if INLINE.contains(&e.name()) {
                        self.inline_element(child, Style::default(), &mut run, within, level);
                    } else {
                        self.push_text(std::mem::take(&mut run), level, None, within);
                        self.walk(child, within);
                    }
                }
                _ => {}
            }
        }
        self.push_text(run, level, None, within);
    }

    /// The text under `element`, added to `run`.
    fn inline(&mut self, element: ElementRef, style: Style, run: &mut Run, within: Within) {
        for node in element.children() {
            match node.value() {
                Node::Text(text) => push_words(&mut run.rich, text, style),
                Node::Element(_) => {
                    if let Some(child) = ElementRef::wrap(node) {
                        self.inline_element(child, style, run, within, None);
                    }
                }
                _ => {}
            }
        }
    }

    /// One element in a line of text, added to `run`: its words in its style, a link with the
    /// text it covers, a formula as words, a picture as words or as a block of its own (the text
    /// before it then a block, of `level` when it is a heading's).
    fn inline_element(
        &mut self,
        element: ElementRef,
        style: Style,
        run: &mut Run,
        within: Within,
        level: Option<u8>,
    ) {
        let e = element.value();
        if SKIPPED.contains(&e.name()) || hidden(element) {
            return;
        }
        self.note_start(element);
        match e.name() {
            "br" => {
                let text = &mut run.rich.text;
                if !text.is_empty() {
                    if text.ends_with(' ') {
                        text.pop();
                    }
                    text.push('\n');
                }
            }
            "img" => {
                let src = e.attr("src").unwrap_or_default();
                let bytes = resolve(&self.file, src).and_then(|href| match href {
                    Href::Inside(path, _) => (self.picture)(&path).map(|b| (path, b)),
                    Href::Outside(_) => None,
                });
                let alt = [e.attr("alt"), e.attr("title")]
                    .into_iter()
                    .flatten()
                    .map(str::trim)
                    .find(|a| !a.is_empty());
                match bytes {
                    Some((path, bytes)) if !in_line(&bytes, alt) => {
                        self.push_text(std::mem::take(run), level, None, within);
                        push_picture(&mut self.blocks, path, bytes);
                    }
                    _ => {
                        if let Some(alt) = alt {
                            push_words(&mut run.rich, &formula(alt), style.with(Style::ITALIC));
                        }
                    }
                }
            }
            "math" => {
                let said = Some(math::mathml(element))
                    .filter(|said| !said.is_empty())
                    .or_else(|| e.attr("alttext").map(formula))
                    .or_else(|| tex_of(element).map(|tex| math::tex(&tex)))
                    .unwrap_or_else(|| element.text().collect::<Vec<_>>().join(""));
                push_words(&mut run.rich, &said, style.with(Style::ITALIC));
            }
            "a" => {
                let start = run.rich.text.len();
                self.inline(element, style, run, within);
                let end = run.rich.text.trim_end().len();
                if end > start {
                    match e.attr("href").and_then(|href| resolve(&self.file, href)) {
                        Some(Href::Inside(path, fragment)) => {
                            let key = match fragment {
                                Some(id) => format!("{path}#{id}"),
                                None => path,
                            };
                            run.inside.push((start, end, key));
                        }
                        Some(Href::Outside(address)) if is_web(&address) => {
                            run.rich.links.push(Link {
                                start,
                                end,
                                to: Target::Web(address),
                            });
                        }
                        _ => {}
                    }
                }
            }
            "i" | "em" | "cite" | "dfn" | "var" => {
                self.inline(element, style.with(Style::ITALIC), run, within)
            }
            "b" | "strong" => self.inline(element, style.with(Style::BOLD), run, within),
            _ => self.inline(element, style, run, within),
        }
    }

    /// A list's items, each a block of its own with its mark; an item's further paragraphs and
    /// the lists inside it are set in with it.
    fn list(&mut self, list: ElementRef, numbered: bool, within: Within) {
        let deeper = Within {
            item: within.item.saturating_add(1),
            ..within
        };
        let mut number: u32 = list
            .value()
            .attr("start")
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(1);
        for child in list.children().filter_map(ElementRef::wrap) {
            if child.value().name() != "li" {
                self.walk(child, deeper);
                continue;
            }
            if hidden(child) {
                continue;
            }
            self.note_start(child);
            if let Some(value) = child
                .value()
                .attr("value")
                .and_then(|v| v.trim().parse().ok())
            {
                number = value;
            }
            self.mark = Some(if numbered {
                format!("{number}. ")
            } else {
                "• ".to_owned()
            });
            number += 1;
            self.children(child, deeper, None);
            self.mark = None;
        }
    }

    /// A table as its caption and its rows, a row's cells apart by `CELLS`; a row of header
    /// cells is in bold.
    fn table(&mut self, table: ElementRef, within: Within) {
        for caption in table
            .children()
            .filter_map(ElementRef::wrap)
            .filter(|e| e.value().name() == "caption")
        {
            self.walk(caption, within);
        }
        let rows = table
            .descendants()
            .filter_map(ElementRef::wrap)
            .filter(|e| e.value().name() == "tr");
        for row in rows {
            self.note_start(row);
            let cells: Vec<ElementRef> = row
                .children()
                .filter_map(ElementRef::wrap)
                .filter(|c| matches!(c.value().name(), "td" | "th"))
                .collect();
            let header = !cells.is_empty() && cells.iter().all(|c| c.value().name() == "th");
            let style = if header {
                Style::BOLD
            } else {
                Style::default()
            };
            let mut run = Run::default();
            // Empty cells at a row's ends say nothing; those between others keep their place.
            let first = cells.iter().position(|c| !empty(*c)).unwrap_or(cells.len());
            let last = cells
                .iter()
                .rposition(|c| !empty(*c))
                .map_or(0, |at| at + 1);
            for (i, cell) in cells.into_iter().enumerate().take(last).skip(first) {
                if i > first {
                    let text = &mut run.rich.text;
                    text.truncate(text.trim_end().len());
                    text.push_str(CELLS);
                }
                self.inline(cell, style, &mut run, within);
            }
            self.push_text(run, None, Some(Kind::Row), within);
        }
    }

    /// Text set as written: its spaces and line breaks kept.
    fn preformatted(&mut self, pre: ElementRef) {
        self.note_start(pre);
        let text: String = pre.text().collect::<String>().replace("\r\n", "\n");
        let text = text.replace('\t', "    ");
        let text = text.trim_matches('\n').trim_end();
        if text.trim().is_empty() {
            return;
        }
        let rich = Rich {
            text: text.to_owned(),
            ..Rich::default()
        };
        self.blocks.push(Block::Text {
            kind: Kind::Pre,
            text: rich,
        });
    }

    /// A picture as a block of its own, from an `img` or an SVG `image`.
    fn picture(&mut self, element: ElementRef) {
        let e = element.value();
        let src = e.attr("src").or(e.attr("href")).unwrap_or_default();
        let Some(Href::Inside(path, _)) = resolve(&self.file, src) else {
            return;
        };
        if let Some(bytes) = (self.picture)(&path) {
            push_picture(&mut self.blocks, path, bytes);
        }
    }

    /// The text gathered, as a block: a heading of `level`, or text of `kind`, else what the
    /// blocks being made are inside of says (an item of a list, a quotation), else a paragraph.
    /// Space at its ends is trimmed, and nothing is made of a run with no words.
    fn push_text(&mut self, run: Run, level: Option<u8>, kind: Option<Kind>, within: Within) {
        let Run { mut rich, inside } = run;
        let start = rich.text.len() - rich.text.trim_start().len();
        let end = rich.text.trim_end().len();
        if start >= end {
            return;
        }
        let shift = |at: usize| at.clamp(start, end) - start;
        rich.text = rich.text[start..end].to_owned();
        for span in &mut rich.spans {
            (span.start, span.end) = (shift(span.start), shift(span.end));
        }
        rich.spans.retain(|s| s.start < s.end);
        for link in &mut rich.links {
            (link.start, link.end) = (shift(link.start), shift(link.end));
        }
        rich.links.retain(|l| l.start < l.end);
        let mut inside: Vec<(usize, usize, String)> = inside
            .into_iter()
            .map(|(s, e, key)| (shift(s), shift(e), key))
            .filter(|(s, e, _)| s < e)
            .collect();
        let kind = kind.or(match (level, within) {
            (Some(_), _) => None,
            (None, Within { item: 1.., .. }) => Some(Kind::Item { depth: within.item }),
            (None, Within { quote: true, .. }) => Some(Kind::Quote),
            _ => None,
        });
        // The item's mark goes before its first block's words.
        if let (Some(Kind::Item { .. }), Some(mark)) = (kind, self.mark.take()) {
            let by = mark.len();
            rich.text.insert_str(0, &mark);
            for span in &mut rich.spans {
                (span.start, span.end) = (span.start + by, span.end + by);
            }
            for link in &mut rich.links {
                (link.start, link.end) = (link.start + by, link.end + by);
            }
            for (s, e, _) in &mut inside {
                (*s, *e) = (*s + by, *e + by);
            }
        }
        let block = self.blocks.len();
        self.pending
            .extend(inside.into_iter().map(|(start, end, key)| Pending {
                block,
                start,
                end,
                key,
            }));
        self.blocks.push(match (level, kind) {
            (Some(level), _) => Block::Heading { level, text: rich },
            (None, Some(kind)) => Block::Text { kind, text: rich },
            (None, None) => Block::Paragraph(rich),
        });
    }

    /// Keeps where an element with an `id` starts: at the block made next.
    fn note_start(&mut self, element: ElementRef) {
        if let Some(id) = element.value().attr("id").filter(|id| !id.is_empty()) {
            let key = format!("{}#{id}", self.file);
            self.starts.entry(key).or_insert(self.blocks.len());
        }
    }
}

/// A picture as a block, unless it is a speck (a spacer).
fn push_picture(blocks: &mut Vec<Block>, name: String, bytes: Vec<u8>) {
    if let Some((w, h)) = picture_size(&bytes) {
        if w < SPECK || h < SPECK {
            return;
        }
    }
    blocks.push(Block::Image { name, bytes });
}

/// Whether a picture among text is the size of a word: no taller than `IN_LINE`, or, when its
/// size cannot be read, one that says what it is in a few words.
fn in_line(bytes: &[u8], alt: Option<&str>) -> bool {
    match picture_size(bytes) {
        Some((_, h)) => h <= IN_LINE,
        None => alt.is_some_and(|a| !a.is_empty() && a.chars().count() <= 40),
    }
}

/// A link that may be opened in a browser.
fn is_web(address: &str) -> bool {
    let lower = address.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// A formula's words as a reader reads them: TeX and spoken math as Unicode math, other words
/// as they are.
fn formula(said: &str) -> String {
    if math::looks_like_tex(said) || said.contains('^') || said.contains('_') {
        math::tex(said)
    } else {
        math::spoken(said).unwrap_or_else(|| said.to_owned())
    }
}

/// The TeX a MathML formula carries as an annotation, if it does.
fn tex_of(math: ElementRef) -> Option<String> {
    math.descendants()
        .filter_map(ElementRef::wrap)
        .find(|e| {
            e.value().name() == "annotation"
                && e.value()
                    .attr("encoding")
                    .is_some_and(|enc| enc.to_ascii_lowercase().contains("tex"))
        })
        .map(|e| e.text().collect::<String>().trim().to_owned())
        .filter(|tex| !tex.is_empty())
}

/// Whether an element is not shown: marked hidden, styled out of sight, or a page break's
/// marker (whose number would land in the middle of a sentence).
fn hidden(element: ElementRef) -> bool {
    let e = element.value();
    if e.attr("hidden").is_some() || e.attr("aria-hidden") == Some("true") {
        return true;
    }
    if e.attr("style").is_some_and(|style| {
        let style: String = style.chars().filter(|c| !c.is_whitespace()).collect();
        style.to_ascii_lowercase().contains("display:none")
    }) {
        return true;
    }
    let types = e.attr("epub:type").unwrap_or_default();
    types.split_whitespace().any(|t| t == "pagebreak") || e.attr("role") == Some("doc-pagebreak")
}

/// Whether an element holds no words and no picture.
fn empty(element: ElementRef) -> bool {
    element.text().all(|t| t.trim().is_empty())
        && !element
            .descendants()
            .filter_map(ElementRef::wrap)
            .any(|e| matches!(e.value().name(), "img" | "image" | "math"))
}

/// The picture an element holds and nothing else (but space), if it does.
fn alone_picture(element: ElementRef) -> Option<ElementRef> {
    let mut inside = element
        .children()
        .filter(|n| !matches!(n.value(), Node::Text(t) if t.trim().is_empty()));
    let only = ElementRef::wrap(inside.next()?)?;
    (inside.next().is_none() && only.value().name() == "img").then_some(only)
}

/// Whether an element holds blocks, not only a line of text.
fn holds_blocks(element: ElementRef) -> bool {
    element
        .children()
        .filter_map(ElementRef::wrap)
        .any(|c| !INLINE.contains(&c.value().name()))
}

/// How a stretch of text is set.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Style {
    italic: bool,
    bold: bool,
}

impl Style {
    const ITALIC: Style = Style {
        italic: true,
        bold: false,
    };
    const BOLD: Style = Style {
        italic: false,
        bold: true,
    };

    fn with(self, other: Style) -> Style {
        Style {
            italic: self.italic || other.italic,
            bold: self.bold || other.bold,
        }
    }
}

/// `text` added to `rich` in `style`, with each run of white space as one space, and none at
/// the start of a line.
fn push_words(rich: &mut Rich, text: &str, style: Style) {
    let start = rich.text.len();
    for word in text.split_inclusive(char::is_whitespace) {
        let (word, spaced) = match word.trim_end() {
            w if w.len() < word.len() => (w, true),
            w => (w, false),
        };
        rich.text.push_str(word);
        let at_line_start = rich.text.is_empty() || rich.text.ends_with(['\n', ' ']);
        if spaced && !at_line_start {
            rich.text.push(' ');
        }
    }
    if (style.italic || style.bold) && rich.text.len() > start {
        rich.spans.push(Span {
            start,
            end: rich.text.len(),
            italic: style.italic,
            bold: style.bold,
        });
    }
}

fn rich_mut(block: &mut Block) -> Option<&mut Rich> {
    match block {
        Block::Heading { text, .. } | Block::Paragraph(text) | Block::Text { text, .. } => {
            Some(text)
        }
        Block::Image { .. } => None,
    }
}
