//! Math, typeset natively. Books and models write LaTeX; `mitex` turns it into Typst's math, Typst
//! (as a library, with its own fonts) lays it out, and the result is a picture at the screen's
//! density with its size and baseline, so a window can set it in a line of text. No browser, no
//! KaTeX. The Typst definitions `mitex` leans on are compiled in from third_party/.

use std::sync::LazyLock;

use include_dir::{include_dir, Dir};
use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::layout::{Abs, Frame, FrameItem};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::{LazyHash, Scalar};
use typst::{Library, LibraryExt, World};
use typst_layout::PagedDocument;

static MITEX: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../third_party/mitex");

/// Typst's standard library and fonts, loaded once.
struct Shared {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
}

static SHARED: LazyLock<Shared> = LazyLock::new(|| {
    let fonts: Vec<Font> = typst_assets::fonts()
        .flat_map(|data| Font::iter(Bytes::new(data)))
        .collect();
    Shared {
        library: LazyHash::new(Library::default()),
        book: LazyHash::new(FontBook::from_fonts(&fonts)),
        fonts,
    }
});

/// One formula's world: its own source, and the definitions it imports.
struct Formula {
    main: Source,
}

impl Formula {
    /// The text of a file a formula may read: mitex's definitions, under /mitex.
    fn read(id: FileId) -> FileResult<&'static [u8]> {
        let path = id.get();
        let name = path.vpath().get_without_slash();
        let missing = || FileError::NotFound(name.into());
        match path.root() {
            VirtualRoot::Project => {
                let inside = name.strip_prefix("mitex/").ok_or_else(missing)?;
                MITEX
                    .get_file(inside)
                    .map(|f| f.contents())
                    .ok_or_else(missing)
            }
            VirtualRoot::Package(_) => Err(missing()),
        }
    }
}

impl World for Formula {
    fn library(&self) -> &LazyHash<Library> {
        &SHARED.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &SHARED.book
    }

    fn main(&self) -> FileId {
        self.main.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() {
            return Ok(self.main.clone());
        }
        let text =
            String::from_utf8(Self::read(id)?.to_vec()).map_err(|_| FileError::InvalidUtf8)?;
        Ok(Source::new(id, text))
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        Self::read(id).map(Bytes::new)
    }

    fn font(&self, index: usize) -> Option<Font> {
        SHARED.fonts.get(index).cloned()
    }

    fn today(&self, _: Option<Duration>) -> Option<Datetime> {
        None
    }
}

/// A typeset formula, as a picture.
#[derive(Clone, Debug)]
pub struct Rendered {
    /// Its size in points (the units text sizes are in).
    pub width: f32,
    pub height: f32,
    /// How far under its top edge the baseline lies, in points: where the text line's baseline
    /// goes when the formula sits in a sentence.
    pub baseline: f32,
    /// Its pixels: RGBA, not premultiplied, `pixel_width` × `pixel_height`.
    pub pixels: Vec<u8>,
    pub pixel_width: u32,
    pub pixel_height: u32,
}

/// LaTeX math as Typst math (in mitex's vocabulary).
pub fn to_typst(latex: &str) -> Result<String, String> {
    mitex::convert_math(latex, None).map(|math| modernize(&math))
}

/// Names mitex writes that this Typst no longer has, and the characters they stood for: a
/// character means the same in every Typst.
const RENAMED: &[(&str, &str)] = &[
    ("plus.circle", "⊕"),
    ("plus.circle.big", "⨁"),
    ("minus.circle", "⊖"),
    ("times.circle", "⊗"),
    ("times.circle.big", "⨂"),
    ("dot.circle", "⊙"),
    ("dot.circle.big", "⨀"),
    ("ast.circle", "⊛"),
    ("dash.circle", "⊝"),
    ("circle.nested", "⊚"),
    ("bracket.l.double", "⟦"),
    ("bracket.r.double", "⟧"),
    ("angle.l", "⟨"),
    ("angle.r", "⟩"),
    ("sect", "∩"),
    ("sect.big", "⋂"),
    ("sect.double", "⋒"),
    ("sect.sq", "⊓"),
    ("diff", "∂"),
    ("planck.reduce", "ℏ"),
    ("arrow.l.dash", "⇠"),
    ("arrow.r.dash", "⇢"),
    ("ohm.inv", "℧"),
    ("argmax", "op(\"arg max\", limits: #true)"),
    ("argmin", "op(\"arg min\", limits: #true)"),
];

/// `math` with each name in [`RENAMED`] replaced, wherever it stands as a whole name outside a
/// string.
fn modernize(math: &str) -> String {
    let mut out = String::with_capacity(math.len());
    let mut chars = math.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        if c == '"' {
            // A string: copied as it is, to its closing quote.
            out.push(c);
            while let Some((_, c)) = chars.next() {
                out.push(c);
                match c {
                    '\\' => out.extend(chars.next().map(|(_, c)| c)),
                    '"' => break,
                    _ => {}
                }
            }
        } else if c.is_ascii_alphabetic() {
            let mut end = start + 1;
            while let Some((at, c)) = chars.peek().copied() {
                if !(c.is_ascii_alphanumeric() || c == '.') {
                    break;
                }
                end = at + 1;
                chars.next();
            }
            let name = math[start..end].trim_end_matches('.');
            let dots = &math[start + name.len()..end];
            match RENAMED.iter().find(|(old, _)| *old == name) {
                Some((_, new)) => out.push_str(new),
                None => out.push_str(name),
            }
            out.push_str(dots);
        } else {
            out.push(c);
        }
    }
    out
}

/// Typeset `latex` at `size` points in the colour `ink` (r, g, b), drawn at `density` pixels a
/// point. `display` sets it as a block of its own; otherwise as part of a line.
pub fn render(
    latex: &str,
    display: bool,
    size: f32,
    ink: (u8, u8, u8),
    density: f32,
) -> Result<Rendered, String> {
    let math = to_typst(latex)?;
    let quoted = math.replace('\\', "\\\\").replace('"', "\\\"");
    let (r, g, b) = ink;
    // In a line, Typst gives a formula the height of the text around it, whatever it holds: room
    // above and under keeps a fraction or a sum's limits in the picture.
    let margin = if display { 0.0 } else { size * 0.9 };
    let text = format!(
        "#import \"/mitex/specs/mod.typ\": mitex-scope\n\
         #set page(width: auto, height: auto, margin: (x: 0pt, y: {margin}pt), fill: none)\n\
         #set text(size: {size}pt, fill: rgb({r}, {g}, {b}), top-edge: \"ascender\", bottom-edge: \"descender\")\n\
         #math.equation(block: {display}, eval(\"$\" + \"{quoted}\" + \"$\", scope: mitex-scope))\n"
    );
    let main = RootedPath::new(
        VirtualRoot::Project,
        VirtualPath::new("main.typ").map_err(|e| e.to_string())?,
    );
    let world = Formula {
        main: Source::new(main.intern(), text),
    };
    let document = typst::compile::<PagedDocument>(&world)
        .output
        .map_err(|errors| {
            errors
                .iter()
                .map(|e| e.message.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        })?;
    let page = document.pages().first().ok_or("the formula is empty")?;
    let options = typst_render::RenderOptions {
        pixel_per_pt: Scalar::new(density as f64),
        render_bleed: false,
    };
    let pixmap = typst_render::render(page, &options);
    let mut pixels = pixmap.data().to_vec();
    for p in pixels.as_chunks_mut::<4>().0 {
        let a = p[3] as u32;
        if a != 0 && a != 255 {
            for c in &mut p[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    let size = page.frame.size();
    let height = size.y.to_pt() as f32;
    Ok(Rendered {
        width: size.x.to_pt() as f32,
        height,
        baseline: baseline(&page.frame, Abs::zero())
            .map(|b| b.to_pt() as f32)
            .unwrap_or(height * 0.75),
        pixels,
        pixel_width: pixmap.width(),
        pixel_height: pixmap.height(),
    })
}

/// The baseline of the first line in `frame`, from the page's top: that of the first frame that
/// says where its baseline is, or else of the first run of text (a plain formula is runs of text
/// laid straight on the line, each placed by its baseline).
fn baseline(frame: &Frame, top: Abs) -> Option<Abs> {
    fn framed(frame: &Frame, top: Abs) -> Option<Abs> {
        if frame.has_baseline() {
            return Some(top + frame.baseline());
        }
        frame.items().find_map(|(at, item)| match item {
            FrameItem::Group(group) => framed(&group.frame, top + at.y),
            _ => None,
        })
    }
    fn first_text(frame: &Frame, top: Abs) -> Option<Abs> {
        frame.items().find_map(|(at, item)| match item {
            FrameItem::Text(_) => Some(top + at.y),
            FrameItem::Group(group) => first_text(&group.frame, top + at.y),
            _ => None,
        })
    }
    framed(frame, top).or_else(|| first_text(frame, top))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORMULAS: &[&str] = &[
        r"e^{i\pi} + 1 = 0",
        r"\frac{-b \pm \sqrt{b^2 - 4ac}}{2a}",
        r"\int_0^\infty e^{-x^2}\,dx = \frac{\sqrt{\pi}}{2}",
        r"\sum_{k=1}^{n} k = \frac{n(n+1)}{2}",
        r"\begin{pmatrix} a & b \\ c & d \end{pmatrix} \begin{pmatrix} x \\ y \end{pmatrix}",
        r"\nabla \times \mathbf{E} = -\frac{\partial \mathbf{B}}{\partial t}",
        r"\mathrm{Attention}(Q, K, V) = \mathrm{softmax}\left(\frac{QK^T}{\sqrt{d_k}}\right)V",
        r"\lim_{x \to 0} \frac{\sin x}{x} = 1",
        r"f(x) = \begin{cases} x^2 & x \ge 0 \\ -x & x < 0 \end{cases}",
        r"\hat{H}\psi = i\hbar\frac{\partial \psi}{\partial t}",
        r"P(A \mid B) = \frac{P(B \mid A)\,P(A)}{P(B)}",
        r"\mathbb{E}[X] = \sum_{x \in \mathcal{X}} x\,p(x)",
        r"x \xrightarrow{f} y",
    ];

    #[test]
    fn common_formulas_typeset() {
        let started = std::time::Instant::now();
        for latex in FORMULAS {
            let out = render(latex, true, 16.0, (0, 0, 0), 2.0)
                .unwrap_or_else(|e| panic!("{latex}: {e}"));
            assert!(
                out.width > 8.0 && out.height > 8.0,
                "{latex}: {}x{}",
                out.width,
                out.height
            );
            assert!(
                out.pixels.as_chunks::<4>().0.iter().any(|p| p[3] > 0),
                "{latex}: nothing drawn"
            );
        }
        eprintln!("{} formulas in {:?}", FORMULAS.len(), started.elapsed());
    }

    #[test]
    fn inline_math_has_a_baseline_inside_it() {
        let out = render(r"\frac{a}{b} + x_i", false, 16.0, (0, 0, 0), 2.0).unwrap();
        assert!(
            out.baseline > 0.0 && out.baseline < out.height,
            "{} of {}",
            out.baseline,
            out.height
        );
        // A plain formula and one with a fraction stand on the same line.
        let plain = render(r"a \neq 0", false, 16.0, (0, 0, 0), 2.0).unwrap();
        let tall = render(r"x^2 + \frac{1}{n}", false, 16.0, (0, 0, 0), 2.0).unwrap();
        assert!(
            (plain.baseline - tall.baseline).abs() < 0.5,
            "{} and {}",
            plain.baseline,
            tall.baseline
        );
    }

    #[test]
    fn broken_latex_is_an_error_not_a_panic() {
        assert!(render(r"\notacommand{x}", true, 16.0, (0, 0, 0), 2.0).is_err());
        // Unbalanced braces are forgiven, not fatal.
        let _ = render(r"\frac{a}{b", true, 16.0, (0, 0, 0), 2.0);
    }
}
