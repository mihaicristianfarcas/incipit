//! The math page: LaTeX typeset by `incipit-math` and drawn in the window, on its own lines and
//! inside sentences, sitting on the text's baseline. Each formula is typeset once for the theme and
//! the screen's density and kept.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    canvas, div, prelude::*, px, AnyElement, Bounds, Context, Corners, IntoElement, Render,
    RenderImage, ScrollHandle, Window,
};
use incipit_ui::{card, page, page_header, Theme, Type};

/// The size of the text the formulas sit in.
const TEXT: f32 = 16.0;
/// The height of a line of it.
const LINE: f32 = TEXT * 1.7;

const DISPLAYED: &[(&str, &str)] = &[
    ("Euler's identity", r"e^{i\pi} + 1 = 0"),
    (
        "The quadratic formula",
        r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}",
    ),
    (
        "A Gaussian integral",
        r"\int_0^\infty e^{-x^2}\,dx = \frac{\sqrt{\pi}}{2}",
    ),
    (
        "Attention",
        r"\mathrm{Attention}(Q, K, V) = \mathrm{softmax}\left(\frac{QK^T}{\sqrt{d_k}}\right)V",
    ),
    (
        "Faraday's law",
        r"\nabla \times \mathbf{E} = -\frac{\partial \mathbf{B}}{\partial t}",
    ),
    (
        "A matrix acting on a vector",
        r"\begin{pmatrix} a & b \\ c & d \end{pmatrix} \begin{pmatrix} x \\ y \end{pmatrix} = \begin{pmatrix} ax + by \\ cx + dy \end{pmatrix}",
    ),
    (
        "A function by cases",
        r"f(x) = \begin{cases} x^2 & x \ge 0 \\ -x & x < 0 \end{cases}",
    ),
    (
        "The Schrödinger equation",
        r"i\hbar\frac{\partial}{\partial t}\Psi(\mathbf{r}, t) = \hat{H}\,\Psi(\mathbf{r}, t)",
    ),
    (
        "Bayes' rule",
        r"P(A \mid B) = \frac{P(B \mid A)\,P(A)}{P(B)}",
    ),
    (
        "An expectation",
        r"\mathbb{E}[X] = \sum_{x \in \mathcal{X}} x\,p(x) \xrightarrow{n \to \infty} \mu",
    ),
];

/// Sentences with `$…$` in them.
const SENTENCES: &[&str] = &[
    r"A quadratic $ax^2 + bx + c = 0$ with $a \neq 0$ has two roots when its discriminant $\Delta = b^2 - 4ac$ is positive, one when $\Delta = 0$, and none on the real line otherwise.",
    r"The sample mean $\bar{x} = \frac{1}{n}\sum_{i=1}^{n} x_i$ tends to $\mu$ as $n \to \infty$, at a rate of about $\sigma / \sqrt{n}$.",
];

/// A formula ready to paint.
#[derive(Clone)]
struct Typeset {
    image: Arc<RenderImage>,
    width: f32,
    height: f32,
    baseline: f32,
}

pub struct MathView {
    /// By LaTeX, whether it is displayed, and whether the theme is dark.
    cache: HashMap<(String, bool, bool), Result<Typeset, String>>,
    /// How long typesetting everything on the page took, the first time.
    took: Option<Duration>,
    scroll: ScrollHandle,
}

impl MathView {
    pub fn new(_: &mut Context<Self>) -> Self {
        MathView {
            cache: HashMap::new(),
            took: None,
            scroll: ScrollHandle::new(),
        }
    }

    fn typeset(
        &mut self,
        latex: &str,
        display: bool,
        t: Theme,
        density: f32,
    ) -> Result<Typeset, String> {
        self.cache
            .entry((latex.to_string(), display, t.dark))
            .or_insert_with(|| {
                let ink = t.primary;
                let rgb = (
                    (ink.r * 255.0) as u8,
                    (ink.g * 255.0) as u8,
                    (ink.b * 255.0) as u8,
                );
                let size = if display { TEXT * 1.15 } else { TEXT };
                let out = incipit_math::render(latex, display, size, rgb, density)?;
                let mut pixels = out.pixels;
                // GPUI's images are BGRA.
                for p in pixels.as_chunks_mut::<4>().0 {
                    p.swap(0, 2);
                }
                let buffer = image::RgbaImage::from_raw(out.pixel_width, out.pixel_height, pixels)
                    .ok_or("the picture has the wrong size")?;
                let image = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
                Ok(Typeset {
                    image,
                    width: out.width,
                    height: out.height,
                    baseline: out.baseline,
                })
            })
            .clone()
    }

    /// A formula as an element of its own size.
    fn formula(&mut self, latex: &str, below: Option<f32>, t: Theme, density: f32) -> AnyElement {
        let display = below.is_none();
        match self.typeset(latex, display, t, density) {
            Ok(set) => {
                let image = set.image.clone();
                let picture = canvas(
                    |_, _, _| (),
                    move |bounds: Bounds<gpui::Pixels>, _, window, _| {
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
                .w(px(set.width))
                .h(px(set.height));
                match below {
                    None => picture.into_any_element(),
                    // In a line everything stands on the line's bottom edge: lift the formula so
                    // its baseline is as far above that edge as the words' is.
                    Some(below) => div()
                        .flex_none()
                        .mb(px(below - (set.height - set.baseline)))
                        .child(picture)
                        .into_any_element(),
                }
            }
            Err(e) => div()
                .font_family(incipit_ui::mono_font())
                .text_size(px(Type::META))
                .text_color(t.danger)
                .child(format!("{latex} ({e})"))
                .into_any_element(),
        }
    }

    /// A sentence with its `$…$` typeset, wrapping word by word.
    fn sentence(&mut self, text: &str, t: Theme, window: &Window) -> AnyElement {
        let density = window.scale_factor();
        // How far a word's baseline is above the bottom of its line box.
        let run = gpui::TextRun {
            len: 1,
            font: gpui::font(incipit_ui::ui_font()),
            color: gpui::Hsla::default(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window
            .text_system()
            .shape_line("x".into(), px(TEXT), &[run], None);
        let (ascent, descent) = (shaped.ascent.as_f32(), shaped.descent.as_f32());
        let below = (LINE - ascent - descent) / 2.0 + descent;
        let mut line = div()
            .flex()
            .flex_wrap()
            .items_end()
            .gap_x(px(TEXT * 0.28))
            .text_size(px(TEXT));
        for (i, part) in text.split('$').enumerate() {
            if i % 2 == 1 {
                line = line.child(self.formula(part, Some(below), t, density));
            } else {
                for word in part.split_whitespace() {
                    line = line.child(
                        div()
                            .flex_none()
                            .line_height(px(LINE))
                            .child(word.to_string()),
                    );
                }
            }
        }
        line.into_any_element()
    }
}

impl Render for MathView {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        let density = window.scale_factor();
        let started = Instant::now();
        let sentences: Vec<AnyElement> = SENTENCES
            .iter()
            .map(|s| self.sentence(s, t, window))
            .collect();
        let displayed: Vec<AnyElement> = DISPLAYED
            .iter()
            .map(|(name, latex)| {
                div()
                    .py(px(14.0))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.0))
                    .child(self.formula(latex, None, t, density))
                    .child(
                        div()
                            .text_size(px(Type::META))
                            .text_color(t.tertiary)
                            .child(*name),
                    )
                    .into_any_element()
            })
            .collect();
        let took = *self.took.get_or_insert_with(|| started.elapsed());
        let count = DISPLAYED.len()
            + SENTENCES
                .iter()
                .map(|s| s.matches('$').count() / 2)
                .sum::<usize>();
        let body = div()
            .flex()
            .flex_col()
            .gap(px(18.0))
            .child(page_header(
                "Math",
                Some(
                    format!(
                        "{count} formulas typeset from LaTeX in {} ms, with no browser.",
                        took.as_millis()
                    )
                    .into(),
                ),
                None,
                t,
            ))
            .child(div().flex().flex_col().gap(px(10.0)).children(sentences))
            .child(card(t).px(px(16.0)).flex().flex_col().children(displayed));
        page("math", &self.scroll, 760.0, t, body)
    }
}
