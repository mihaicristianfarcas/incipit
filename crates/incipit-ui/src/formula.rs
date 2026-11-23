//! Formulas in the window's text. The look has no typesetter of its own: the app gives it one
//! (`typeset_with`; it answers with incipit-math), so this crate does not carry Typst. A formula
//! is typeset on a background thread once for its TeX, its size, its ink and the screen's
//! density, and kept; until it is ready, and where it cannot be typeset, what was written stands
//! in its place, as the earlier app showed math as its TeX until KaTeX had loaded.
//!
//! In a line, a formula sits on the text's baseline. Its picture is cut to its ink, and its box
//! ends as far under its baseline as a word's line box ends under the word's, so in a row whose
//! items are aligned by their bottoms (`items_baseline`, as an item without a baseline of its own
//! is aligned by its bottom edge) the baselines meet; what hangs lower than the words do is kept
//! as room under the line, and what stands taller makes the line taller, as in a browser.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{
    canvas, div, img, prelude::*, px, AnyElement, App, Bounds, Corners, Element, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, RenderImage, Rgba, TextRun, Window,
};

/// A formula typeset into a picture.
#[derive(Clone, Debug)]
pub struct Typeset {
    /// Its size in points, and how far under its top edge its baseline lies.
    pub width: f32,
    pub height: f32,
    pub baseline: f32,
    /// Its pixels: RGBA, not premultiplied, `pixel_width` × `pixel_height`.
    pub pixels: Vec<u8>,
    pub pixel_width: u32,
    pub pixel_height: u32,
}

/// What typesets a formula: its TeX, whether it is a block of its own (else part of a line), the
/// size of its type in points, its ink, and the screen's pixels a point. An error says why the
/// TeX could not be typeset.
pub type Typesetter = fn(&str, bool, f32, (u8, u8, u8), f32) -> Result<Typeset, String>;

/// How much larger than its text a formula of its own is set, as display math is in print.
const DISPLAYED: f32 = 1.15;
/// The formulas kept before those kept are let go: enough for every formula on screen many
/// times over, as an answer or a card holds a few.
const KEPT: usize = 2000;

/// Gives the window its typesetter. Without one, formulas stay as they were written.
pub fn typeset_with(cx: &mut App, typesetter: Typesetter) {
    cx.default_global::<Formulas>().typesetter = Some(typesetter);
}

#[derive(Default)]
struct Formulas {
    typesetter: Option<Typesetter>,
    kept: HashMap<Key, Kept>,
}

impl gpui::Global for Formulas {}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    tex: String,
    display: bool,
    /// The type's size and the pixels a point, as their bits (they are only ever compared).
    size: u32,
    density: u32,
    ink: [u8; 3],
}

enum Kept {
    Typesetting,
    Ready(Arc<Picture>),
    Failed,
}

struct Picture {
    image: Arc<RenderImage>,
    width: f32,
    height: f32,
    baseline: f32,
}

/// A formula's picture when it is typeset. Otherwise it is asked for, once, and the window is
/// drawn again when it is ready.
fn picture(key: Key, window: &mut Window, cx: &mut App) -> Option<Arc<Picture>> {
    let formulas = cx.default_global::<Formulas>();
    let typesetter = formulas.typesetter?;
    match formulas.kept.get(&key) {
        Some(Kept::Ready(picture)) => return Some(picture.clone()),
        Some(_) => return None,
        None => {}
    }
    if formulas.kept.len() >= KEPT {
        formulas
            .kept
            .retain(|_, kept| matches!(kept, Kept::Typesetting));
    }
    formulas.kept.insert(key.clone(), Kept::Typesetting);
    let window = window.window_handle();
    cx.spawn(async move |cx| {
        let asked = key.clone();
        let made = cx
            .background_executor()
            .spawn(async move {
                let [r, g, b] = asked.ink;
                typesetter(
                    &asked.tex,
                    asked.display,
                    f32::from_bits(asked.size),
                    (r, g, b),
                    f32::from_bits(asked.density),
                )
                .ok()
                .and_then(|set| picture_of(set, f32::from_bits(asked.density)))
            })
            .await;
        cx.update(|cx| {
            let kept = made.map_or(Kept::Failed, |p| Kept::Ready(Arc::new(p)));
            cx.default_global::<Formulas>().kept.insert(key, kept);
            let _ = window.update(cx, |_, window, _| window.refresh());
        });
    })
    .detach();
    None
}

/// The rows of a picture (RGBA, `width` pixels wide) that hold ink: the first and one past the
/// last. `None` for a picture with none.
pub(crate) fn inked_rows(pixels: &[u8], width: usize) -> Option<(usize, usize)> {
    let inked = |row: &[u8]| row.as_chunks::<4>().0.iter().any(|p| p[3] > 0);
    let rows: Vec<&[u8]> = pixels.chunks_exact(width.max(1) * 4).collect();
    let first = rows.iter().position(|row| inked(row))?;
    let last = rows.iter().rposition(|row| inked(row))?;
    Some((first, last + 1))
}

/// A typeset formula as a picture the window can paint, cut to the rows holding ink.
fn picture_of(set: Typeset, density: f32) -> Option<Picture> {
    let width = set.pixel_width as usize;
    let (first, end) = inked_rows(&set.pixels, width)?;
    let mut pixels = set.pixels[first * width * 4..end * width * 4].to_vec();
    // GPUI's images are BGRA.
    for p in pixels.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(set.pixel_width, (end - first) as u32, pixels)?;
    let cut = first as f32 / density;
    Some(Picture {
        image: Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])),
        width: set.width,
        height: (end - first) as f32 / density,
        baseline: set.baseline - cut,
    })
}

/// A formula: TeX `tex` in a line of text of type `size` points on lines `line` high, or a block
/// of its own (`display`), in `ink`. `written` stands in until it is typeset, and where it
/// cannot be.
pub(crate) fn formula(
    tex: String,
    display: bool,
    size: f32,
    line: f32,
    ink: Rgba,
    written: impl FnOnce() -> AnyElement + 'static,
) -> impl IntoElement {
    lazily(move |window, cx| {
        let channel = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
        let key = Key {
            tex,
            display,
            size: (if display { size * DISPLAYED } else { size }).to_bits(),
            density: window.scale_factor().to_bits(),
            ink: [channel(ink.r), channel(ink.g), channel(ink.b)],
        };
        let Some(picture) = picture(key, window, cx) else {
            return written();
        };
        if display {
            // Narrower room than the formula: it is drawn smaller to fit.
            return div()
                .w_full()
                .py(px(4.0))
                .flex()
                .justify_center()
                .child(
                    img(picture.image.clone())
                        .w(px(picture.width))
                        .max_w_full()
                        .aspect_ratio(picture.width / picture.height.max(1.0)),
                )
                .into_any_element();
        }
        // Where the words' baseline is, above the bottom of their line box.
        let run = TextRun {
            len: 1,
            font: window.text_style().font(),
            color: gpui::Hsla::default(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window
            .text_system()
            .shape_line("x".into(), px(size), &[run], None);
        let (ascent, descent) = (shaped.ascent.as_f32(), shaped.descent.as_f32());
        let below = (line - ascent - descent) / 2.0 + descent;
        let hangs = picture.height - picture.baseline;
        let image = picture.image.clone();
        div()
            .flex_none()
            .relative()
            .w(px(picture.width))
            .h(px(picture.baseline + below))
            .mb(px((hangs - below).max(0.0)))
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds: Bounds<Pixels>, _, window, _| {
                        let _ = window.paint_image(
                            bounds,
                            bounds,
                            Corners::default(),
                            image.clone(),
                            0,
                            false,
                        );
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .w(px(picture.width))
                .h(px(picture.height)),
            )
            .into_any_element()
    })
}

/// An element made when the window lays it out, from what the window knows then (its text
/// style, its density, what is kept in the app).
pub(crate) fn lazily<F>(make: F) -> Lazily<F>
where
    F: FnOnce(&mut Window, &mut App) -> AnyElement + 'static,
{
    Lazily(Some(make))
}

pub(crate) struct Lazily<F>(Option<F>);

impl<F> IntoElement for Lazily<F>
where
    F: FnOnce(&mut Window, &mut App) -> AnyElement + 'static,
{
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl<F> Element for Lazily<F>
where
    F: FnOnce(&mut Window, &mut App) -> AnyElement + 'static,
{
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, AnyElement) {
        let make = self.0.take().expect("an element is laid out once a frame");
        let mut element = make(window, cx);
        let layout = element.request_layout(window, cx);
        (layout, element)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        element: &mut AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) {
        element.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        element: &mut AnyElement,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        element.paint(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_formulas_picture_is_cut_to_its_ink() {
        // Three pixels wide, five high: ink on the second and third rows only.
        let mut pixels = vec![0u8; 3 * 5 * 4];
        pixels[(3 + 1) * 4 + 3] = 255;
        pixels[(2 * 3) * 4 + 3] = 40;
        assert_eq!(inked_rows(&pixels, 3), Some((1, 3)));
        assert_eq!(inked_rows(&[0u8; 3 * 5 * 4], 3), None);
    }

    #[test]
    fn a_cut_picture_keeps_its_baseline_where_it_was() {
        let mut pixels = vec![0u8; 2 * 10 * 4];
        // Ink on rows 4 to 7, at two pixels a point.
        for row in 4..8 {
            pixels[row * 2 * 4 + 3] = 255;
        }
        let set = Typeset {
            width: 1.0,
            height: 5.0,
            baseline: 3.0,
            pixels,
            pixel_width: 2,
            pixel_height: 10,
        };
        let picture = picture_of(set, 2.0).unwrap();
        assert_eq!(picture.height, 2.0);
        // Two points were cut above it.
        assert_eq!(picture.baseline, 1.0);
    }
}
