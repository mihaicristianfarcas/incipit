//! One row of the reader's list: a part of the book (a PDF's page, an EPUB's block) with the
//! margin's cards for it beside it. A PDF's page is drawn dark when the app is (`incipit_book::
//! dark`), as the earlier app's reader drew it. A row turns the mouse into places in the book; what a press,
//! a drag and a release mean is the view's.

use std::sync::Arc;

use gpui::{
    canvas, div, fill, font, img, prelude::*, px, size, AnyElement, Bounds, Context, Corners,
    FontStyle, FontWeight, Hsla, ImageFormat, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, RenderImage, Rgba, StyledText, TextRun, Window,
};
use incipit_book::dark::{self, Dark};
use incipit_book::{Block, Page, Rich};
use incipit_ui::Theme;

use super::model::{byte_of, Doc, Note, Opened, Place};
use super::{ReaderView, State, ANSWER, COLUMN, GAP, MARGIN, PAGE_GAP};

const SERIF: &str = "Charter";
/// Room kept for a card when placing the next one under it: closed, open (the most it can
/// take: its quote, what it says, its buttons), and being written.
const CARD: (f32, f32, f32) = (44.0, ANSWER + 160.0, 150.0);
/// How strongly the place the find bar is on is marked: more than a note, as the earlier app
/// marked it, so it stands out among them.
const FOUND: f32 = 0.55;

/// Turns a drawn page's RGBA pixels dark (on the PDF's thread), leaving its pictures as they are.
fn darken(pixels: &mut [u8], page: &Page) {
    let scale = page.pixel_width as f32 / page.width.max(1.0);
    let boxes: Vec<[usize; 4]> = page
        .pictures
        .iter()
        .map(|&(x, y, w, h)| {
            let at = |v: f32| (v * scale).max(0.0) as usize;
            [
                at(x),
                at(y),
                at(x + w).saturating_add(1),
                at(y + h).saturating_add(1),
            ]
        })
        .collect();
    DARK.recolour(pixels, page.pixel_width as usize, &boxes);
}

/// The dark page's ramp, worked out once.
static DARK: std::sync::LazyLock<Dark> = std::sync::LazyLock::new(Dark::new);

pub fn format_of(name: &str) -> Option<ImageFormat> {
    match name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => Some(ImageFormat::Png),
        "jpg" | "jpeg" => Some(ImageFormat::Jpeg),
        "gif" => Some(ImageFormat::Gif),
        "svg" => Some(ImageFormat::Svg),
        "webp" => Some(ImageFormat::Webp),
        _ => None,
    }
}

fn tint(colour: Rgba, alpha: f32) -> Hsla {
    Rgba { a: alpha, ..colour }.into()
}

/// The colour a note's passage is marked in, at `alpha`: a concept's passage in the accent, more
/// lightly.
fn note_ink(note: &Note, t: Theme, alpha: f32) -> Hsla {
    if note.concept.is_some() {
        tint(t.accent, alpha * 0.7)
    } else {
        tint(t.attention, alpha)
    }
}

impl ReaderView {
    pub(super) fn row(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = crate::theme(window);
        let pdf = matches!(
            &self.state,
            State::Open(Opened {
                doc: Doc::Pdf(_),
                ..
            })
        );
        let (part, margin) = if pdf {
            self.page(index, t, window, cx)
        } else {
            self.block(index, t, window, cx)
        };
        div()
            .w_full()
            .flex()
            .justify_center()
            .child(
                div()
                    .flex()
                    .gap(px(GAP))
                    .child(part)
                    .when(self.margin, |row| row.child(margin)),
            )
            .into_any_element()
    }

    /// The character of page `index` at a point of the window.
    fn page_place(&self, index: usize, position: Point<Pixels>) -> Option<Place> {
        let State::Open(Opened {
            doc: Doc::Pdf(pdf), ..
        }) = &self.state
        else {
            return None;
        };
        let held = pdf.pages.get(&index)?;
        let scale = self.scale.min(self.room) / held.page.width;
        let p = position - held.origin.get();
        let at = held
            .page
            .glyph_at(p.x.as_f32() / scale, p.y.as_f32() / scale)?;
        Some(Place { part: index, at })
    }

    /// A PDF's page, and the margin beside it with each card at its passage's height.
    fn page(
        &mut self,
        index: usize,
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (AnyElement, AnyElement) {
        let width = self.scale.min(self.room);
        let wanted = (width * window.scale_factor()) as u32;
        let drawn = self.drawn.clone();
        let State::Open(Opened {
            doc: Doc::Pdf(pdf), ..
        }) = &mut self.state
        else {
            return (div().into_any_element(), div().into_any_element());
        };
        let (page_width, page_height) = pdf.sizes[index];
        let scale = width / page_width;
        let height = page_height * scale;
        // A page not drawn yet, or drawn for another width or the other theme, is asked for once.
        let dark = t.dark;
        let fresh = pdf
            .pages
            .get(&index)
            .is_some_and(|h| h.page.pixel_width.abs_diff(wanted) <= 1 && h.dark == dark);
        if !fresh && pdf.asked.insert(index) {
            pdf.worker.page(index, wanted, move |result| {
                let result = result.map_err(|e| format!("{e:#}")).and_then(|mut page| {
                    let mut pixels = std::mem::take(&mut page.pixels);
                    if dark {
                        darken(&mut pixels, &page);
                    }
                    // GPUI's images are BGRA.
                    for p in pixels.as_chunks_mut::<4>().0 {
                        p.swap(0, 2);
                    }
                    let buffer =
                        image::RgbaImage::from_raw(page.pixel_width, page.pixel_height, pixels)
                            .ok_or("the page's picture has the wrong size")?;
                    Ok((
                        page,
                        Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])),
                        dark,
                    ))
                });
                let _ = drawn.unbounded_send((index, result));
            });
        }

        let State::Open(Opened {
            doc: Doc::Pdf(pdf), ..
        }) = &self.state
        else {
            unreachable!("matched above")
        };
        let held = pdf.pages.get(&index);
        // What to mark on the page, and the cards beside it with the height of their passage.
        let mut marks: Vec<((f32, f32, f32, f32), Hsla)> = Vec::new();
        let mut cards: Vec<(f32, f32, AnyElement)> = Vec::new();
        if let Some(held) = held {
            for note in &self.notes {
                let mut top = None;
                for anchor in note.anchors.iter().filter(|a| a.part == index) {
                    let Some((start, end)) = anchor.locate(&held.text) else {
                        continue;
                    };
                    let boxes = held.page.boxes(start, end);
                    top = top.or(boxes.first().map(|b| b.1 * scale));
                    let ink = note_ink(note, t, 0.32);
                    marks.extend(boxes.into_iter().map(|b| (b, ink)));
                }
                let first = note.anchors.first().is_some_and(|a| a.part == index);
                if let (Some(top), true, true) = (top, first, note.in_margin()) {
                    let room = if self.open == Some(note.id) {
                        CARD.1
                    } else {
                        CARD.0
                    };
                    cards.push((
                        top - 8.0,
                        room,
                        self.card(note, t, window, cx).into_any_element(),
                    ));
                }
            }
            if let Some(anchor) = self
                .composing
                .as_ref()
                .and_then(|a| a.first())
                .filter(|a| a.part == index)
            {
                let boxes = anchor
                    .locate(&held.text)
                    .map(|(s, e)| held.page.boxes(s, e))
                    .unwrap_or_default();
                let top = boxes.first().map(|b| b.1 * scale).unwrap_or(0.0);
                marks.extend(boxes.into_iter().map(|b| (b, tint(t.attention, 0.32))));
                cards.push((top - 8.0, CARD.2, self.composer(t, cx).into_any_element()));
            }
            if let Some((start, end)) = self
                .selection
                .and_then(|s| s.within(index, held.text.len(), true))
            {
                marks.extend(
                    held.page
                        .boxes(start, end)
                        .into_iter()
                        .map(|b| (b, tint(t.accent, 0.3))),
                );
            }
            if let Some((start, end)) = self.found_in(index) {
                marks.extend(
                    held.page
                        .boxes(start, end)
                        .into_iter()
                        .map(|b| (b, tint(t.attention, FOUND))),
                );
            }
        }
        // Cards never overlap: one whose passage is too close under another's moves down.
        cards.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut floor = f32::MIN;
        let cards = cards.into_iter().map(|(top, room, card)| {
            // A card near the foot of its page moves up to stay on it, where there is room.
            let top = top.min(height - room).max(0.0).max(floor);
            floor = top + room + 8.0;
            div().absolute().top(px(top)).left_0().child(card)
        });

        let picture = held.map(|held| {
            let image = held.image.clone();
            let origin = held.origin.clone();
            canvas(
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
            .size_full()
        });
        let sheet = div()
            .w(px(width))
            .h(px(height))
            .flex_none()
            .mb(px(PAGE_GAP))
            // The paper, until the page is drawn on it: dark on a dark page.
            .bg(if dark {
                let [r, g, b] = dark::GROUND;
                gpui::Rgba {
                    r: f32::from(r) / 255.0,
                    g: f32::from(g) / 255.0,
                    b: f32::from(b) / 255.0,
                    a: 1.0,
                }
            } else {
                gpui::white().into()
            })
            .rounded(px(3.0))
            .overflow_hidden()
            .cursor_text()
            .children(picture)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                    this.focus.focus(window, cx);
                    if let Some(place) = this.page_place(index, e.position) {
                        this.pressed(place, cx);
                    }
                }),
            )
            .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, _, cx| {
                if !this.dragging {
                    return;
                }
                if let Some(place) = this.page_place(index, e.position) {
                    this.dragged(place, e.pressed_button == Some(MouseButton::Left), cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, e: &MouseUpEvent, _, cx| this.released(e.position, cx)),
            );
        let margin = div().relative().w(px(MARGIN)).flex_none().children(cards);
        (sheet.into_any_element(), margin.into_any_element())
    }

    /// An EPUB's block, and the margin's cards for the passages that start in it.
    fn block(
        &self,
        index: usize,
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (AnyElement, AnyElement) {
        let State::Open(Opened {
            doc: Doc::Epub(book),
            ..
        }) = &self.state
        else {
            return (div().into_any_element(), div().into_any_element());
        };
        let Some((block, chars)) = book.blocks.get(index) else {
            return (div().into_any_element(), div().into_any_element());
        };
        let content: AnyElement = match block {
            Block::Image { name, .. } => match book.pictures.get(&index) {
                Some(picture) => div()
                    .py(px(10.0))
                    .flex()
                    .justify_center()
                    .child(img(picture.clone()).max_w(px(COLUMN * 0.8)))
                    .into_any_element(),
                None => div()
                    .text_color(t.tertiary)
                    .child(format!("[{name}]"))
                    .into_any_element(),
            },
            Block::Heading { level, text } => {
                let scale = match level {
                    1 | 2 => 1.55,
                    3 => 1.25,
                    _ => 1.1,
                };
                self.passage(index, text, chars, self.scale * scale, true, t, cx)
                    .pt(px(18.0))
                    .into_any_element()
            }
            Block::Paragraph(text) => self
                .passage(index, text, chars, self.scale, false, t, cx)
                .into_any_element(),
        };
        let starts_here = |note: &&Note| note.anchors.first().is_some_and(|a| a.part == index);
        let mut cards = Vec::new();
        for note in self
            .notes
            .iter()
            .filter(starts_here)
            .filter(|n| n.in_margin())
        {
            cards.push(self.card(note, t, window, cx).into_any_element());
        }
        let composer = self
            .composing
            .as_ref()
            .and_then(|a| a.first())
            .filter(|a| a.part == index)
            .map(|_| self.composer(t, cx).into_any_element());
        let margin = div()
            .w(px(MARGIN))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .pb(px(8.0))
            .children(cards)
            .children(composer);
        (
            div()
                .w(px(COLUMN.min(self.room)))
                .flex_none()
                .child(content)
                .into_any_element(),
            margin.into_any_element(),
        )
    }

    /// One block of text: its styles, the noted passages and the selection as runs, and the mouse
    /// turned into places in the text by the text's own layout.
    #[allow(clippy::too_many_arguments)]
    fn passage(
        &self,
        index: usize,
        text: &Rich,
        chars: &[char],
        size: f32,
        heading: bool,
        t: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        // Each stretch to mark, in bytes, with its colour: notes first, the selection over them.
        let noted = self
            .notes
            .iter()
            .flat_map(|n| n.anchors.iter().map(move |a| (a, note_ink(n, t, 0.3))));
        let composing = self
            .composing
            .iter()
            .flatten()
            .map(|a| (a, tint(t.attention, 0.3)));
        let mut marks: Vec<(usize, usize, Hsla)> = noted
            .chain(composing)
            .filter(|(a, _)| a.part == index)
            .filter_map(|(a, ink)| a.locate(chars).map(|found| (found, ink)))
            .map(|((s, e), ink)| (byte_of(&text.text, s), byte_of(&text.text, e), ink))
            .collect();
        if let Some((from, to)) = self
            .selection
            .and_then(|s| s.within(index, text.text.len(), false))
        {
            marks.push((from, to, tint(t.accent, 0.35)));
        }
        if let Some((start, end)) = self.found_in(index) {
            let found = (byte_of(&text.text, start), byte_of(&text.text, end));
            marks.push((found.0, found.1, tint(t.attention, FOUND)));
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
            Ok(at) | Err(at) => Place { part: index, at },
        };
        let (down, moved) = (place.clone(), place);
        div()
            .text_size(px(size))
            .line_height(px(size * 1.62))
            .pb(px(if heading { 6.0 } else { size * 0.8 }))
            .cursor_text()
            .child(styled)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                    this.focus.focus(window, cx);
                    this.pressed(down(e.position), cx)
                }),
            )
            .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, _, cx| {
                if this.dragging {
                    this.dragged(
                        moved(e.position),
                        e.pressed_button == Some(MouseButton::Left),
                        cx,
                    );
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, e: &MouseUpEvent, _, cx| this.released(e.position, cx)),
            )
    }
}
