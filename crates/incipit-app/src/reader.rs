//! The reader page: a PDF drawn by pdfium and an EPUB laid out as native text, each with text you
//! can select by dragging and notes in the margin beside the passage they are tied to.
//! INCIPIT_BOOK=pdf|epub picks the book, INCIPIT_NOTE=1 starts with a passage noted and another
//! selected (for pictures taken without a mouse).
//!
//! Spike limits: the first pages and blocks only (no virtual list), a selection stays inside one
//! page or one paragraph, and a note's words are fixed (no text field yet).

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    canvas, div, fill, font, img, prelude::*, px, size, AnyElement, Bounds, Context, Corners,
    FontStyle, FontWeight, Hsla, Image, ImageFormat, IntoElement, MouseButton, Pixels, Point,
    Render, RenderImage, Rgba, ScrollHandle, StyledText, TextRun, Window,
};
use incipit_book::{Anchor, Block, Book, Page, Pdf, Rich};
use incipit_ui::{button, page, page_header, segmented, Theme, Type};

/// The width of a page or a column of text, and of the margin beside it.
const COLUMN: f32 = 560.0;
const MARGIN: f32 = 230.0;
const PAGES: usize = 3;
const BLOCKS: usize = 70;
const SERIF: &str = "Charter";
const BODY: f32 = 17.0;

fn root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// A stretch of one page (in characters) or one block (in bytes).
#[derive(Clone, Copy, PartialEq)]
struct Selection {
    part: usize,
    from: usize,
    to: usize,
}

impl Selection {
    fn range(&self) -> (usize, usize) {
        (self.from.min(self.to), self.from.max(self.to))
    }
}

struct Note {
    anchor: Anchor,
    words: String,
}

struct PdfPage {
    page: Page,
    image: Arc<RenderImage>,
    /// Where the page was last painted in the window.
    origin: Rc<Cell<Point<Pixels>>>,
}

struct Shelf<T> {
    book: Result<T, String>,
    took: Duration,
    selection: Option<Selection>,
    notes: Vec<Note>,
}

/// A book's blocks, each with its text as characters (what anchors count in).
type Blocks = Vec<(Block, Vec<char>)>;

pub struct ReaderView {
    epub: bool,
    pdf: Option<Shelf<(usize, Vec<PdfPage>)>>,
    book: Option<Shelf<(String, Blocks)>>,
    dragging: bool,
    demo: bool,
    scroll: ScrollHandle,
}

impl ReaderView {
    pub fn new(_: &mut Context<Self>) -> Self {
        ReaderView {
            epub: std::env::var("INCIPIT_BOOK").as_deref() == Ok("epub"),
            pdf: None,
            book: None,
            dragging: false,
            demo: std::env::var_os("INCIPIT_NOTE").is_some(),
            scroll: ScrollHandle::new(),
        }
    }

    fn open_pdf(&mut self, density: f32) {
        let started = Instant::now();
        let book = (|| -> anyhow::Result<(usize, Vec<PdfPage>)> {
            let pdf = Pdf::open(&root().join("lib/lib"), &root().join("samples/paper.pdf"))?;
            let mut pages = Vec::new();
            for index in 0..pdf.pages().min(PAGES) {
                let mut page = pdf.page(index, (COLUMN * density) as u32)?;
                let mut pixels = std::mem::take(&mut page.pixels);
                // GPUI's images are BGRA.
                for p in pixels.as_chunks_mut::<4>().0 {
                    p.swap(0, 2);
                }
                let buffer =
                    image::RgbaImage::from_raw(page.pixel_width, page.pixel_height, pixels)
                        .ok_or_else(|| anyhow::anyhow!("the page's picture has the wrong size"))?;
                let image = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
                pages.push(PdfPage {
                    page,
                    image,
                    origin: Rc::default(),
                });
            }
            Ok((pdf.pages(), pages))
        })()
        .map_err(|e| format!("{e:#}"));
        let mut shelf = Shelf {
            book,
            took: started.elapsed(),
            selection: None,
            notes: Vec::new(),
        };
        if let (true, Ok((_, pages))) = (self.demo, &shelf.book) {
            let text = pages[0].page.text();
            if let Some((start, end)) = find(&text, "based solely on attention mechanisms") {
                shelf.notes.push(Note {
                    anchor: Anchor::of(0, &text, start, end),
                    words: "The claim of the whole paper. What does recurrence buy that attention has to replace?".into(),
                });
            }
            if let Some((start, end)) =
                find(&text, "superior in quality while being more parallelizable")
            {
                shelf.selection = Some(Selection {
                    part: 0,
                    from: start,
                    to: end - 1,
                });
            }
        }
        self.pdf = Some(shelf);
    }

    fn open_epub(&mut self) {
        let started = Instant::now();
        let book = Book::open(&root().join("samples/book.epub")).map_err(|e| format!("{e:#}")).map(|book| {
            // Start at the first chapter, past the title pages and notes.
            let first = book
                .blocks
                .iter()
                .position(|b| matches!(b, Block::Heading { text, .. } if text.text.contains("Physical Meaning")))
                .unwrap_or(0);
            let blocks = book
                .blocks
                .into_iter()
                .skip(first)
                .take(BLOCKS)
                .map(|b| {
                    let chars: Vec<char> = rich(&b).map(|r| r.text.chars().collect()).unwrap_or_default();
                    (b, chars)
                })
                .collect::<Vec<_>>();
            (book.title, blocks)
        });
        let mut shelf = Shelf {
            book,
            took: started.elapsed(),
            selection: None,
            notes: Vec::new(),
        };
        if let (true, Ok((_, blocks))) = (self.demo, &shelf.book) {
            for (part, (_, chars)) in blocks.iter().enumerate() {
                if let Some((start, end)) = find(
                    chars,
                    "What, then, do you mean by the assertion that these propositions are true?",
                ) {
                    shelf.notes.push(Note {
                        anchor: Anchor::of(part, chars, start, end),
                        words: "The question the chapter turns on: following from axioms is not the same as being true of the world.".into(),
                    });
                }
                if let Some((start, end)) = find(chars, "with more respect than love") {
                    let text: String = chars.iter().collect();
                    shelf.selection = Some(Selection {
                        part,
                        from: bytes(&text, start),
                        to: bytes(&text, end),
                    });
                }
            }
        }
        self.book = Some(shelf);
    }

    fn note(&mut self, cx: &mut Context<Self>) {
        let words = "A note on this passage.".to_string();
        if self.epub {
            let Some(shelf) = &mut self.book else { return };
            let (Some(s), Ok((_, blocks))) = (shelf.selection.take(), &shelf.book) else {
                return;
            };
            let (block, chars) = &blocks[s.part];
            let Some(r) = rich(block) else { return };
            let (from, to) = s.range();
            let anchor = Anchor::of(
                s.part,
                chars,
                r.text[..from].chars().count(),
                r.text[..to].chars().count(),
            );
            shelf.notes.push(Note { anchor, words });
        } else {
            let Some(shelf) = &mut self.pdf else { return };
            let (Some(s), Ok((_, pages))) = (shelf.selection.take(), &shelf.book) else {
                return;
            };
            let (from, to) = s.range();
            shelf.notes.push(Note {
                anchor: Anchor::of(s.part, &pages[s.part].page.text(), from, to + 1),
                words,
            });
        }
        cx.notify();
    }

    fn pdf_rows(&self, t: Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(Shelf {
            book: Ok((_, pages)),
            selection,
            notes,
            ..
        }) = &self.pdf
        else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        for (index, held) in pages.iter().enumerate() {
            let scale = COLUMN / held.page.width;
            let height = held.page.height * scale;
            // What to mark on this page: the notes' passages, then the selection.
            let mut marks: Vec<((f32, f32, f32, f32), Hsla)> = Vec::new();
            let mut cards = Vec::new();
            for note in notes.iter().filter(|n| n.anchor.part == index) {
                let Some((start, end)) = note.anchor.locate(&held.page.text()) else {
                    continue;
                };
                let boxes = held.page.boxes(start, end);
                let top = boxes.first().map(|b| b.1 * scale).unwrap_or(0.0);
                marks.extend(boxes.into_iter().map(|b| (b, tint(t.attention, 0.32))));
                cards.push(
                    card(note, t)
                        .absolute()
                        .top(px(top - 6.0))
                        .left_0()
                        .into_any_element(),
                );
            }
            if let Some(s) = selection.filter(|s| s.part == index) {
                let (from, to) = s.range();
                marks.extend(
                    held.page
                        .boxes(from, to + 1)
                        .into_iter()
                        .map(|b| (b, tint(t.accent, 0.3))),
                );
            }
            let image = held.image.clone();
            let origin = held.origin.clone();
            let picture = canvas(
                move |bounds: Bounds<Pixels>, _, _| origin.set(bounds.origin),
                move |bounds: Bounds<Pixels>, _, window, _| {
                    let _ = window.paint_image(
                        bounds,
                        bounds,
                        Corners::default(),
                        image.clone(),
                        0,
                        false,
                    );
                    for ((x, y, w, h), colour) in &marks {
                        let at = bounds.origin + Point::new(px(x * scale), px(y * scale));
                        window.paint_quad(fill(
                            Bounds::new(at, size(px(w * scale), px(h * scale))),
                            *colour,
                        ));
                    }
                },
            )
            .size_full();
            let origin = held.origin.clone();
            let at = move |this: &Self, position: Point<Pixels>| -> Option<usize> {
                let Some(Shelf {
                    book: Ok((_, pages)),
                    ..
                }) = &this.pdf
                else {
                    return None;
                };
                let p = position - origin.get();
                pages[index]
                    .page
                    .glyph_at(p.x.as_f32() / scale, p.y.as_f32() / scale)
            };
            let (down, moved) = (at.clone(), at);
            let sheet = div()
                .w(px(COLUMN))
                .h(px(height))
                .flex_none()
                .rounded(px(4.0))
                .overflow_hidden()
                .border_1()
                .border_color(t.hairline)
                .cursor_text()
                .child(picture)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, e: &gpui::MouseDownEvent, _, cx| {
                        let glyph = down(this, e.position);
                        if let (Some(g), Some(shelf)) = (glyph, &mut this.pdf) {
                            shelf.selection = Some(Selection {
                                part: index,
                                from: g,
                                to: g,
                            });
                            this.dragging = true;
                            cx.notify();
                        }
                    }),
                )
                .on_mouse_move(cx.listener(move |this, e: &gpui::MouseMoveEvent, _, cx| {
                    if !this.dragging || e.pressed_button != Some(MouseButton::Left) {
                        return;
                    }
                    let glyph = moved(this, e.position);
                    if let (
                        Some(g),
                        Some(Shelf {
                            selection: Some(s), ..
                        }),
                    ) = (glyph, &mut this.pdf)
                    {
                        if s.part == index && s.to != g {
                            s.to = g;
                            cx.notify();
                        }
                    }
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _, _, _| this.dragging = false),
                );
            rows.push(
                div()
                    .flex()
                    .gap(px(18.0))
                    .child(sheet)
                    .child(div().relative().w(px(MARGIN)).flex_none().children(cards))
                    .into_any_element(),
            );
        }
        rows
    }

    fn epub_rows(&self, t: Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(Shelf {
            book: Ok((_, blocks)),
            selection,
            notes,
            ..
        }) = &self.book
        else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        for (index, (block, chars)) in blocks.iter().enumerate() {
            let content: AnyElement = match block {
                Block::Image { name, bytes } => {
                    let format = match name.rsplit('.').next().unwrap_or("") {
                        "png" => Some(ImageFormat::Png),
                        "jpg" | "jpeg" => Some(ImageFormat::Jpeg),
                        "gif" => Some(ImageFormat::Gif),
                        "svg" => Some(ImageFormat::Svg),
                        _ => None,
                    };
                    match format {
                        Some(format) => div()
                            .py(px(10.0))
                            .flex()
                            .justify_center()
                            .child(
                                img(Arc::new(Image::from_bytes(format, bytes.clone())))
                                    .max_w(px(COLUMN * 0.8)),
                            )
                            .into_any_element(),
                        None => div()
                            .text_color(t.tertiary)
                            .child(format!("[{name}]"))
                            .into_any_element(),
                    }
                }
                Block::Heading { level, text } => {
                    let scale = if *level <= 2 { 1.5 } else { 1.2 };
                    self.passage(
                        index,
                        text,
                        chars,
                        BODY * scale,
                        true,
                        *selection,
                        notes,
                        t,
                        cx,
                    )
                    .pt(px(14.0))
                    .into_any_element()
                }
                Block::Paragraph(text) => self
                    .passage(index, text, chars, BODY, false, *selection, notes, t, cx)
                    .into_any_element(),
            };
            let cards = notes
                .iter()
                .filter(|n| n.anchor.part == index)
                .map(|n| card(n, t));
            rows.push(
                div()
                    .flex()
                    .gap(px(18.0))
                    .child(div().w(px(COLUMN)).flex_none().child(content))
                    .child(
                        div()
                            .w(px(MARGIN))
                            .flex_none()
                            .flex()
                            .flex_col()
                            .gap(px(8.0))
                            .children(cards),
                    )
                    .into_any_element(),
            );
        }
        rows
    }

    /// One block of text: its styles, the notes' passages and the selection as runs, and the mouse
    /// turned into places in the text by the text's own layout.
    #[allow(clippy::too_many_arguments)]
    fn passage(
        &self,
        index: usize,
        text: &Rich,
        chars: &[char],
        size: f32,
        heading: bool,
        selection: Option<Selection>,
        notes: &[Note],
        t: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        // Each stretch to mark, in bytes, with its colour: notes first, the selection over them.
        let mut marks: Vec<(usize, usize, Hsla)> = notes
            .iter()
            .filter(|n| n.anchor.part == index)
            .filter_map(|n| n.anchor.locate(chars))
            .map(|(s, e)| {
                (
                    bytes(&text.text, s),
                    bytes(&text.text, e),
                    tint(t.attention, 0.3),
                )
            })
            .collect();
        if let Some(s) = selection.filter(|s| s.part == index) {
            let (from, to) = s.range();
            marks.push((from, to, tint(t.accent, 0.35)));
        }
        let mut cuts: Vec<usize> = vec![0, text.text.len()];
        cuts.extend(text.spans.iter().flat_map(|s| [s.start, s.end]));
        cuts.extend(marks.iter().flat_map(|m| [m.0, m.1]));
        cuts.retain(|c| *c <= text.text.len() && text.text.is_char_boundary(*c));
        cuts.sort_unstable();
        cuts.dedup();
        let runs: Vec<TextRun> = cuts
            .windows(2)
            .map(|w| {
                let inside = |s: usize, e: usize| s <= w[0] && w[1] <= e;
                let mut face = font(SERIF);
                if heading || text.spans.iter().any(|s| s.bold && inside(s.start, s.end)) {
                    face.weight = FontWeight::SEMIBOLD;
                }
                if text
                    .spans
                    .iter()
                    .any(|s| s.italic && inside(s.start, s.end))
                {
                    face.style = FontStyle::Italic;
                }
                TextRun {
                    len: w[1] - w[0],
                    font: face,
                    color: t.primary.into(),
                    background_color: marks.iter().rev().find(|m| inside(m.0, m.1)).map(|m| m.2),
                    underline: None,
                    strikethrough: None,
                }
            })
            .collect();
        let styled = StyledText::new(text.text.clone()).with_runs(runs);
        let layout = styled.layout().clone();
        let place = move |position: Point<Pixels>| match layout.index_for_position(position) {
            Ok(i) | Err(i) => i,
        };
        let (down, moved) = (place.clone(), place);
        div()
            .text_size(px(size))
            .line_height(px(size * 1.6))
            .pb(px(if heading { 4.0 } else { 12.0 }))
            .cursor_text()
            .child(styled)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &gpui::MouseDownEvent, _, cx| {
                    let at = down(e.position);
                    if let Some(shelf) = &mut this.book {
                        shelf.selection = Some(Selection {
                            part: index,
                            from: at,
                            to: at,
                        });
                        this.dragging = true;
                        cx.notify();
                    }
                }),
            )
            .on_mouse_move(cx.listener(move |this, e: &gpui::MouseMoveEvent, _, cx| {
                if !this.dragging || e.pressed_button != Some(MouseButton::Left) {
                    return;
                }
                let at = moved(e.position);
                if let Some(Shelf {
                    selection: Some(s), ..
                }) = &mut this.book
                {
                    if s.part == index && s.to != at {
                        s.to = at;
                        cx.notify();
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.dragging = false),
            )
    }
}

impl Render for ReaderView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        if self.epub && self.book.is_none() {
            self.open_epub();
        }
        if !self.epub && self.pdf.is_none() {
            self.open_pdf(window.scale_factor());
        }
        let (title, about, failed, selected, notes) = if self.epub {
            let shelf = self.book.as_ref().expect("opened above");
            match &shelf.book {
                Ok((title, blocks)) => (
                    title.clone(),
                    format!(
                        "EPUB as native text: {} blocks read in {} ms.",
                        blocks.len(),
                        shelf.took.as_millis()
                    ),
                    None,
                    shelf.selection.is_some_and(|s| s.from != s.to),
                    shelf.notes.len(),
                ),
                Err(e) => ("The book".into(), String::new(), Some(e.clone()), false, 0),
            }
        } else {
            let shelf = self.pdf.as_ref().expect("opened above");
            match &shelf.book {
                Ok((count, pages)) => (
                    "Attention Is All You Need".to_string(),
                    format!(
                        "PDF drawn by pdfium: {} of {count} pages, with every character's place, in {} ms.",
                        pages.len(),
                        shelf.took.as_millis()
                    ),
                    None,
                    shelf.selection.is_some(),
                    shelf.notes.len(),
                ),
                Err(e) => ("The paper".into(), String::new(), Some(e.clone()), false, 0),
            }
        };
        let view = cx.entity().downgrade();
        let tools = div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(
                segmented("reader-book", ["PDF", "EPUB"], self.epub as usize, t).on_select(
                    move |which, _, cx| {
                        let _ = view.update(cx, |this, cx| {
                            this.epub = which == 1;
                            cx.notify();
                        });
                    },
                ),
            )
            .child(
                div().when(!selected, |d| d.opacity(0.45)).child(
                    button("reader-note", "Note on selection", t)
                        .on_click(cx.listener(|this, _, _, cx| this.note(cx))),
                ),
            )
            .child(
                div()
                    .text_size(px(Type::META))
                    .text_color(t.tertiary)
                    .child(match notes {
                        0 => "Drag over the text to select it.".to_string(),
                        1 => "1 note, tied to its passage's words.".to_string(),
                        n => format!("{n} notes, tied to their passages' words."),
                    }),
            );
        let rows = match (&failed, self.epub) {
            (Some(e), _) => vec![div()
                .text_color(t.danger)
                .child(e.clone())
                .into_any_element()],
            (None, true) => self.epub_rows(t, cx),
            (None, false) => self.pdf_rows(t, cx),
        };
        let body = div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(page_header(title, Some(about.into()), None, t))
            .child(tools)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(if self.epub { 0.0 } else { 16.0 }))
                    .children(rows),
            );
        page(
            if self.epub {
                "reader-epub"
            } else {
                "reader-pdf"
            },
            &self.scroll,
            COLUMN + MARGIN + 18.0,
            t,
            body,
        )
    }
}

/// A note beside its passage.
fn card(note: &Note, t: Theme) -> gpui::Div {
    let quote: String = note.anchor.quote.chars().take(60).collect();
    incipit_ui::card(t)
        .w(px(MARGIN))
        .p(px(10.0))
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(
            div()
                .text_size(px(Type::META))
                .text_color(t.tertiary)
                .child(format!("“{quote}”")),
        )
        .child(
            div()
                .text_size(px(Type::BODY))
                .text_color(t.primary)
                .child(note.words.clone()),
        )
}

fn rich(block: &Block) -> Option<&Rich> {
    match block {
        Block::Heading { text, .. } | Block::Paragraph(text) => Some(text),
        Block::Image { .. } => None,
    }
}

fn tint(colour: Rgba, alpha: f32) -> Hsla {
    Rgba { a: alpha, ..colour }.into()
}

/// Where `words` first are in `text`, in characters.
fn find(text: &[char], words: &str) -> Option<(usize, usize)> {
    let words: Vec<char> = words.chars().collect();
    text.windows(words.len())
        .position(|w| w == words)
        .map(|at| (at, at + words.len()))
}

/// The byte a character index is at.
fn bytes(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map(|(b, _)| b)
        .unwrap_or(text.len())
}
