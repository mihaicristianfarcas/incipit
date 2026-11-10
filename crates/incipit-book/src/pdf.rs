//! A PDF through pdfium: each page as a picture at the width asked for, with where every character
//! is on it, so a window can let text be selected over the picture.

use std::path::Path;
use std::sync::OnceLock;

use anyhow::{anyhow, Result};
use pdfium_render::prelude::*;

static PDFIUM: OnceLock<Pdfium> = OnceLock::new();

pub struct Pdf {
    document: PdfDocument<'static>,
}

/// One character on a page, in points from the page's top left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    pub c: char,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub struct Page {
    /// The page's size in points.
    pub width: f32,
    pub height: f32,
    /// Its picture: RGBA, `pixel_width` × `pixel_height`.
    pub pixels: Vec<u8>,
    pub pixel_width: u32,
    pub pixel_height: u32,
    /// Its characters in reading order. Spaces and line ends pdfium adds have no size.
    pub glyphs: Vec<Glyph>,
}

impl Pdf {
    /// Open `file`, with the pdfium library found in `library` (a folder).
    pub fn open(library: &Path, file: &Path) -> Result<Pdf> {
        let pdfium = match PDFIUM.get() {
            Some(pdfium) => pdfium,
            None => {
                let bindings =
                    Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(library))
                        .map_err(|e| {
                            anyhow!("pdfium was not loaded from {}: {e:?}", library.display())
                        })?;
                PDFIUM.get_or_init(|| Pdfium::new(bindings))
            }
        };
        let document = pdfium
            .load_pdf_from_file(file, None)
            .map_err(|e| anyhow!("{}: {e:?}", file.display()))?;
        Ok(Pdf { document })
    }

    pub fn pages(&self) -> usize {
        self.document.pages().len() as usize
    }

    /// Page `index` (from 0), drawn `pixel_width` pixels wide.
    pub fn page(&self, index: usize, pixel_width: u32) -> Result<Page> {
        let page = self
            .document
            .pages()
            .get(index as _)
            .map_err(|e| anyhow!("page {index}: {e:?}"))?;
        let (width, height) = (page.width().value, page.height().value);
        let bitmap = page
            .render_with_config(&PdfRenderConfig::new().set_target_width(pixel_width as i32))
            .map_err(|e| anyhow!("page {index} was not drawn: {e:?}"))?;
        let text = page
            .text()
            .map_err(|e| anyhow!("page {index} has no text: {e:?}"))?;
        let glyphs = text
            .chars()
            .iter()
            .map(|c| {
                let rect = c.loose_bounds().ok();
                let at = |r: &PdfRect| {
                    (
                        r.left().value,
                        height - r.top().value,
                        r.width().value,
                        r.height().value,
                    )
                };
                let (x, y, w, h) = rect.as_ref().map(at).unwrap_or_default();
                Glyph {
                    c: c.unicode_char().unwrap_or(' '),
                    x,
                    y,
                    w,
                    h,
                }
            })
            .collect();
        Ok(Page {
            width,
            height,
            pixels: bitmap.as_rgba_bytes(),
            pixel_width: bitmap.width() as u32,
            pixel_height: bitmap.height() as u32,
            glyphs,
        })
    }
}

impl Page {
    /// The character at (x, y) in points: the one under the point, or the nearest on its line.
    pub fn glyph_at(&self, x: f32, y: f32) -> Option<usize> {
        let sized = self
            .glyphs
            .iter()
            .enumerate()
            .filter(|(_, g)| g.w > 0.0 && g.h > 0.0);
        let mut nearest: Option<(usize, f32)> = None;
        for (i, g) in sized {
            if x >= g.x && x <= g.x + g.w && y >= g.y && y <= g.y + g.h {
                return Some(i);
            }
            // Off a character: its line counts far more than how far along it.
            let dy = if y < g.y {
                g.y - y
            } else if y > g.y + g.h {
                y - g.y - g.h
            } else {
                0.0
            };
            let dx = if x < g.x {
                g.x - x
            } else if x > g.x + g.w {
                x - g.x - g.w
            } else {
                0.0
            };
            let distance = dy * 8.0 + dx;
            if nearest.is_none_or(|(_, d)| distance < d) {
                nearest = Some((i, distance));
            }
        }
        nearest.map(|(i, _)| i)
    }

    /// Boxes covering characters `start..end`, one for each line they run over, in points.
    pub fn boxes(&self, start: usize, end: usize) -> Vec<(f32, f32, f32, f32)> {
        let mut boxes: Vec<(f32, f32, f32, f32)> = Vec::new();
        for g in self.glyphs[start.min(self.glyphs.len())..end.min(self.glyphs.len())].iter() {
            if g.w <= 0.0 || g.h <= 0.0 {
                continue;
            }
            match boxes.last_mut() {
                // On the same line as the box before: it grows.
                Some((x, y, w, h))
                    if (g.y + g.h / 2.0) > *y && (g.y + g.h / 2.0) < *y + *h && g.x >= *x - 1.0 =>
                {
                    let right = (g.x + g.w).max(*x + *w);
                    let top = y.min(g.y);
                    let bottom = (*y + *h).max(g.y + g.h);
                    (*y, *h, *w) = (top, bottom - top, right - *x);
                }
                _ => boxes.push((g.x, g.y, g.w, g.h)),
            }
        }
        boxes
    }

    pub fn text(&self) -> Vec<char> {
        self.glyphs.iter().map(|g| g.c).collect()
    }
}
