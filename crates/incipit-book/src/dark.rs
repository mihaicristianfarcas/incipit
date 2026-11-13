//! A PDF's page drawn dark, for reading when the app is dark, as the earlier app's reader drew
//! it (with its PDF viewer's dark mode): the white of the paper becomes a dark ground, black ink
//! a light one, and every other colour of the page goes along the same ramp by how light it is,
//! in the OKLab colour space, keeping its hue where it has one. Pictures in the page stay as
//! they are, but for a scan: a picture that covers most of the page and looks like paper with
//! ink on it is turned dark as the page is. The two colours and every number here are the
//! earlier app's.
//!
//! The earlier app recoloured each colour the page was drawn with, before drawing it. Here the
//! page is drawn by pdfium and its pixels are recoloured afterwards, outside the pictures' boxes;
//! the soft edges of the text, blends of ink and paper, go along the ramp with them.

use std::collections::HashMap;

/// The dark ground the paper's white becomes, and the light ink black becomes.
pub const GROUND: [u8; 3] = [0x14, 0x12, 0x10];
pub const INK: [u8; 3] = [0xea, 0xe6, 0xe0];

/// A colour this far from grey in OKLab (its chroma) or further keeps its hue whole; one nearer
/// takes that much of it, and a grey takes the ground's and the ink's.
const NEUTRAL_CHROMA: f64 = 0.04;
/// How far outside sRGB a colour may come out of OKLab and still count as inside.
const GAMUT_EPSILON: f64 = 5e-4;
/// The halvings of a colour's chroma when bringing one outside sRGB back inside.
const GAMUT_STEPS: usize = 12;
/// The colours kept between two clearings, a page being drawn in few.
const KEPT: usize = 4096;
/// A scan covers this share of the page at least.
const SCAN_COVERAGE: f64 = 0.7;
/// A scan looks like paper: of a sample of its pixels, this share at least is white…
const SCAN_WHITE: f64 = 0.6;
/// …this share at most is a strong colour…
const SCAN_SATURATED: f64 = 0.15;
/// …and it has ink: a dark pixel at least.
const SCAN_INK: usize = 1;
/// A scan is judged on a sample at most this many pixels on its longer side.
const SAMPLE: f64 = 64.0;

/// The colour ramp of a dark page.
pub struct Dark {
    ground: [f64; 3],
    ink: [f64; 3],
    /// Every grey, already on the ramp: most of a page of text is grey.
    greys: [[u8; 3]; 256],
}

impl Default for Dark {
    fn default() -> Self {
        Dark::new()
    }
}

impl Dark {
    pub fn new() -> Dark {
        let lab = |c: [u8; 3]| {
            let [r, g, b] = c.map(|v| f64::from(v) / 255.0);
            oklab(r, g, b)
        };
        let mut dark = Dark {
            ground: lab(GROUND),
            ink: lab(INK),
            greys: [[0; 3]; 256],
        };
        for v in 0..=255u8 {
            dark.greys[v as usize] = dark.ramp([v, v, v]);
        }
        dark
    }

    /// Where a colour of the page goes on a dark page.
    pub fn colour(&self, rgb: [u8; 3]) -> [u8; 3] {
        let [r, g, b] = rgb;
        if r == g && g == b {
            return self.greys[r as usize];
        }
        self.ramp(rgb)
    }

    fn ramp(&self, rgb: [u8; 3]) -> [u8; 3] {
        // The poles are the two colours exactly.
        match rgb {
            [255, 255, 255] => return GROUND,
            [0, 0, 0] => return INK,
            _ => {}
        }
        let [r, g, b] = rgb.map(|v| f64::from(v) / 255.0);
        let [_, a, bb] = oklab(r, g, b);
        let light = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let along = |i: usize| self.ink[i] + (self.ground[i] - self.ink[i]) * light;
        let (l, ramp_a, ramp_b) = (along(0), along(1), along(2));
        let hue = (a.hypot(bb) / NEUTRAL_CHROMA).min(1.0);
        let out_a = ramp_a + (a - ramp_a) * hue;
        let out_b = ramp_b + (bb - ramp_b) * hue;
        srgb_in_gamut(l, out_a, out_b).map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)
    }

    /// Turns a drawn page dark: its RGBA `pixels`, `width` wide, but inside `pictures` (boxes in
    /// pixels: left, top, right, bottom) that are not scans.
    pub fn recolour(&self, pixels: &mut [u8], width: usize, pictures: &[[usize; 4]]) {
        if width == 0 {
            return;
        }
        let height = pixels.len() / 4 / width;
        let kept: Vec<[usize; 4]> = pictures
            .iter()
            .map(|p| {
                [
                    p[0].min(width),
                    p[1].min(height),
                    p[2].min(width),
                    p[3].min(height),
                ]
            })
            .filter(|p| p[0] < p[2] && p[1] < p[3])
            .filter(|p| !is_scan(pixels, width, height, *p))
            .collect();
        let mut colours: HashMap<[u8; 3], [u8; 3]> = HashMap::new();
        for (y, row) in pixels.chunks_exact_mut(width * 4).enumerate() {
            let across: Vec<(usize, usize)> = kept
                .iter()
                .filter(|p| p[1] <= y && y < p[3])
                .map(|p| (p[0], p[2]))
                .collect();
            for (x, p) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                if across.iter().any(|&(left, right)| left <= x && x < right) {
                    continue;
                }
                let rgb = [p[0], p[1], p[2]];
                let out = if rgb[0] == rgb[1] && rgb[1] == rgb[2] {
                    self.greys[rgb[0] as usize]
                } else {
                    if colours.len() >= KEPT {
                        colours.clear();
                    }
                    *colours.entry(rgb).or_insert_with(|| self.ramp(rgb))
                };
                p[..3].copy_from_slice(&out);
            }
        }
    }
}

/// Whether a picture's box on the page holds a scan: it covers most of the page, and a sample of
/// its pixels is mostly white, has few strong colours, and has some ink.
fn is_scan(pixels: &[u8], width: usize, height: usize, picture: [usize; 4]) -> bool {
    let [left, top, right, bottom] = picture;
    let (w, h) = (right - left, bottom - top);
    if ((w * h) as f64) < SCAN_COVERAGE * (width * height) as f64 {
        return false;
    }
    let step = (w.max(h) as f64 / SAMPLE).max(1.0);
    let (mut total, mut white, mut saturated, mut ink) = (0usize, 0usize, 0usize, 0usize);
    let mut sy = top as f64;
    while (sy as usize) < bottom {
        let mut sx = left as f64;
        while (sx as usize) < right {
            let at = ((sy as usize) * width + sx as usize) * 4;
            let [r, g, b] = [pixels[at], pixels[at + 1], pixels[at + 2]];
            let (most, least) = (r.max(g).max(b), r.min(g).min(b));
            total += 1;
            if least > 200 && most - least < 32 {
                white += 1;
            } else if most > 60 && f64::from(most - least) / f64::from(most) > 0.35 {
                saturated += 1;
            }
            if most < 100 {
                ink += 1;
            }
            sx += step;
        }
        sy += step;
    }
    total > 0
        && white as f64 / total as f64 >= SCAN_WHITE
        && saturated as f64 / total as f64 <= SCAN_SATURATED
        && ink >= SCAN_INK
}

fn to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn from_linear(c: f64) -> f64 {
    if c <= 0.0031308 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// An sRGB colour (0 to 1) in OKLab.
fn oklab(r: f64, g: f64, b: f64) -> [f64; 3] {
    let (r, g, b) = (to_linear(r), to_linear(g), to_linear(b));
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    [
        0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
    ]
}

fn linear_rgb(l: f64, a: f64, b: f64) -> [f64; 3] {
    let lc = (l + 0.3963377774 * a + 0.2158037573 * b).powi(3);
    let mc = (l - 0.1055613458 * a - 0.0638541728 * b).powi(3);
    let sc = (l - 0.0894841775 * a - 1.291485548 * b).powi(3);
    [
        4.0767416621 * lc - 3.3077115913 * mc + 0.2309699292 * sc,
        -1.2684380046 * lc + 2.6097574011 * mc - 0.3413193965 * sc,
        -0.0041960863 * lc - 0.7034186147 * mc + 1.707614701 * sc,
    ]
}

/// An OKLab colour in sRGB, if it is inside it.
fn srgb_exact(l: f64, a: f64, b: f64) -> Option<[f64; 3]> {
    let rgb = linear_rgb(l, a, b);
    rgb.iter()
        .all(|c| (-GAMUT_EPSILON..=1.0 + GAMUT_EPSILON).contains(c))
        .then(|| rgb.map(|c| from_linear(c.clamp(0.0, 1.0))))
}

/// An OKLab colour in sRGB, with its chroma lessened until it fits when it does not.
fn srgb_in_gamut(l: f64, a: f64, b: f64) -> [f64; 3] {
    if let Some(rgb) = srgb_exact(l, a, b) {
        return rgb;
    }
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..GAMUT_STEPS {
        let mid = (lo + hi) / 2.0;
        if srgb_exact(l, a * mid, b * mid).is_some() {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    linear_rgb(l, a * lo, b * lo).map(|c| from_linear(c.clamp(0.0, 1.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(width: usize, height: usize, fill: impl Fn(usize, usize) -> [u8; 3]) -> Vec<u8> {
        (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .flat_map(|(x, y)| {
                let [r, g, b] = fill(x, y);
                [r, g, b, 255]
            })
            .collect()
    }

    fn at(pixels: &[u8], width: usize, x: usize, y: usize) -> [u8; 3] {
        let i = (y * width + x) * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    }

    #[test]
    fn paper_turns_to_the_dark_ground_and_black_ink_to_the_light_one() {
        let dark = Dark::new();
        assert_eq!(dark.colour([255, 255, 255]), GROUND);
        assert_eq!(dark.colour([0, 0, 0]), INK);
        // A grey goes between them, the lighter the darker.
        let [mid, ..] = dark.colour([128, 128, 128]);
        let [light, ..] = dark.colour([200, 200, 200]);
        assert!(GROUND[0] < light && light < mid && mid < INK[0]);
    }

    #[test]
    fn a_colour_keeps_its_hue_and_a_near_grey_does_not() {
        let dark = Dark::new();
        // A strong red stays red.
        let [r, g, b] = dark.colour([200, 30, 30]);
        assert!(r > g + 40 && r > b + 40, "{r} {g} {b}");
        // A grey hardly tinted takes the ramp's warm grey.
        let tinted = dark.colour([129, 128, 128]);
        let grey = dark.colour([128, 128, 128]);
        assert!(tinted.iter().zip(grey).all(|(a, b)| a.abs_diff(b) <= 2));
    }

    #[test]
    fn a_picture_stays_as_it_is_and_the_page_around_it_turns_dark() {
        let (width, height) = (20, 20);
        // White paper with a blue photo in a box.
        let mut pixels = picture(width, height, |x, y| {
            if (5..10).contains(&x) && (5..10).contains(&y) {
                [10, 60, 220]
            } else {
                [255, 255, 255]
            }
        });
        Dark::new().recolour(&mut pixels, width, &[[5, 5, 10, 10]]);
        assert_eq!(at(&pixels, width, 7, 7), [10, 60, 220]);
        assert_eq!(at(&pixels, width, 0, 0), GROUND);
        assert_eq!(at(&pixels, width, 12, 7), GROUND);
    }

    #[test]
    fn a_scan_turns_dark_as_the_page_does() {
        let (width, height) = (40, 40);
        // A scanned page: one picture of paper with a line of ink, over the whole page.
        let mut pixels = picture(width, height, |_, y| {
            if y == 20 {
                [20, 20, 20]
            } else {
                [250, 250, 248]
            }
        });
        let dark = Dark::new();
        dark.recolour(&mut pixels, width, &[[0, 0, width, height]]);
        assert_eq!(at(&pixels, width, 3, 3), dark.colour([250, 250, 248]));
        assert_eq!(at(&pixels, width, 3, 20), dark.colour([20, 20, 20]));
    }

    #[test]
    fn a_photo_over_the_whole_page_is_not_a_scan() {
        let (width, height) = (40, 40);
        let mut pixels = picture(width, height, |x, _| [30, (x * 6) as u8, 200]);
        let before = pixels.clone();
        Dark::new().recolour(&mut pixels, width, &[[0, 0, width, height]]);
        assert_eq!(pixels, before);
    }
}
