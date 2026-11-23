//! The typesetter the window's Markdown is given (`incipit_ui::typeset_with`): LaTeX typeset by
//! `incipit-math`, so math in notes, answers, cards, quizzes and sessions sits on its line's
//! baseline, inside sentences and on lines of its own.

/// Typesets LaTeX for incipit-ui's Markdown (`incipit_ui::typeset_with`).
pub fn typeset(
    latex: &str,
    display: bool,
    size: f32,
    ink: (u8, u8, u8),
    density: f32,
) -> Result<incipit_ui::Typeset, String> {
    incipit_math::render(latex, display, size, ink, density).map(|r| incipit_ui::Typeset {
        width: r.width,
        height: r.height,
        baseline: r.baseline,
        pixels: r.pixels,
        pixel_width: r.pixel_width,
        pixel_height: r.pixel_height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdowns_typesetter_sets_good_math_and_refuses_what_does_not_parse() {
        let set = typeset(r"\frac{a}{b} + x^2", false, 15.0, (20, 20, 20), 2.0).unwrap();
        assert!(set.width > 0.0 && set.baseline > 0.0 && set.baseline < set.height);
        assert_eq!(
            set.pixels.len(),
            (set.pixel_width * set.pixel_height * 4) as usize
        );
        // What cannot be typeset is said so, and Markdown keeps it as written.
        assert!(typeset(r"\frac{a", false, 15.0, (20, 20, 20), 2.0).is_err());
    }
}
