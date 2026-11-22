//! Formulas in a line of text, written as a reader reads them: `x′ = ax − bct`, `E₀/c²`,
//! `√(1 − v²/c²)`. A line of an EPUB's text is one run of characters (what is selected, found
//! and noted), so a formula inside a sentence cannot be a picture there; it is kept as Unicode
//! math instead, close to how it is printed: letters for variables, `₀` and `²` for indices and
//! powers, `/` for fractions, `√` for roots, real minus and times signs.
//!
//! A book gives a formula in one of three ways, each read here:
//!
//! - MathML (`mathml`): its structure, a subscript as a subscript, a fraction as a fraction.
//!   Its bare text would run them together (`<msup>x 2</msup>` reads "x2").
//! - TeX, in MathML's `alttext` or its TeX annotation, or as a picture's alternative text
//!   (`tex`): the commonest commands, Greek letters and operators.
//! - Spoken words, as a picture of a formula's alternative text often is (`spoken`): the
//!   notation screen readers speak ("x prime minus c t Superscript prime Baseline equals 0"),
//!   which Project Gutenberg's books and others give every formula drawn as a picture. Words
//!   that are not that notation (a picture's description) are left as they are.
//!
//! What has no Unicode form (a subscript `q`, a fraction inside a power) is written in the
//! usual plain-text way: `_(q)`, `^(…)`.
//!
//! Each notation is read in a module of its own; the names of symbols (Greek letters, functions)
//! and the writing of indices and powers are shared here.

mod mathml;
mod spoken;
#[cfg(test)]
mod tests;
mod tex;

pub use mathml::mathml;
pub use spoken::spoken;
pub use tex::{looks_like_tex, tex};

/// Names of functions, written as they are.
pub(super) const FUNCTIONS: &[&str] = &[
    "sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan", "sinh", "cosh", "tanh",
    "log", "ln", "exp", "lim", "max", "min", "sup", "inf", "det", "dim", "deg", "gcd", "mod",
];

/// The Greek letters by the names speech and TeX give them: the small letter, and the capital
/// where its shape is not a Latin letter's.
pub(super) const GREEK: &[(&str, char, Option<char>)] = &[
    ("alpha", 'α', None),
    ("beta", 'β', None),
    ("gamma", 'γ', Some('Γ')),
    ("delta", 'δ', Some('Δ')),
    ("epsilon", 'ε', None),
    ("zeta", 'ζ', None),
    ("eta", 'η', None),
    ("theta", 'θ', Some('Θ')),
    ("iota", 'ι', None),
    ("kappa", 'κ', None),
    ("lamda", 'λ', Some('Λ')),
    ("lambda", 'λ', Some('Λ')),
    ("mu", 'μ', None),
    ("nu", 'ν', None),
    ("xi", 'ξ', Some('Ξ')),
    ("omicron", 'ο', None),
    ("pi", 'π', Some('Π')),
    ("rho", 'ρ', None),
    ("sigma", 'σ', Some('Σ')),
    ("tau", 'τ', None),
    ("upsilon", 'υ', Some('Υ')),
    ("phi", 'φ', Some('Φ')),
    ("chi", 'χ', None),
    ("psi", 'ψ', Some('Ψ')),
    ("omega", 'ω', Some('Ω')),
];

pub(super) fn greek(name: &str) -> Option<char> {
    GREEK
        .iter()
        .find(|(n, ..)| *n == name)
        .map(|(_, small, _)| *small)
}

/// A capital letter, Latin or Greek (`upper Delta` is Δ).
pub(super) fn capital(name: &str) -> Option<String> {
    if name.len() == 1 && name.chars().all(|c| c.is_ascii_alphabetic()) {
        return Some(name.to_ascii_uppercase());
    }
    let lower = name.to_ascii_lowercase();
    GREEK
        .iter()
        .find(|(n, ..)| *n == lower)
        .map(|(_, small, big)| big.unwrap_or(*small).to_string())
}

pub(super) fn double_struck(letter: &str) -> Option<String> {
    Some(
        match letter.to_ascii_uppercase().as_str() {
            "R" => "ℝ",
            "N" => "ℕ",
            "Z" => "ℤ",
            "Q" => "ℚ",
            "C" => "ℂ",
            _ => return None,
        }
        .to_owned(),
    )
}

/// `text` raised, as a power: in superscript letters when it has them all, else `^(text)`.
pub fn raised(text: &str) -> String {
    shifted(text, '^', |c| {
        Some(match c {
            '0' => '⁰',
            '1' => '¹',
            '2' => '²',
            '3' => '³',
            '4' => '⁴',
            '5' => '⁵',
            '6' => '⁶',
            '7' => '⁷',
            '8' => '⁸',
            '9' => '⁹',
            '+' => '⁺',
            '−' | '-' => '⁻',
            '=' => '⁼',
            '(' => '⁽',
            ')' => '⁾',
            'a' => 'ᵃ',
            'b' => 'ᵇ',
            'c' => 'ᶜ',
            'd' => 'ᵈ',
            'e' => 'ᵉ',
            'f' => 'ᶠ',
            'g' => 'ᵍ',
            'h' => 'ʰ',
            'i' => 'ⁱ',
            'j' => 'ʲ',
            'k' => 'ᵏ',
            'l' => 'ˡ',
            'm' => 'ᵐ',
            'n' => 'ⁿ',
            'o' => 'ᵒ',
            'p' => 'ᵖ',
            'r' => 'ʳ',
            's' => 'ˢ',
            't' => 'ᵗ',
            'u' => 'ᵘ',
            'v' => 'ᵛ',
            'w' => 'ʷ',
            'x' => 'ˣ',
            'y' => 'ʸ',
            'z' => 'ᶻ',
            // A prime is raised already.
            '′' | '″' | '‴' => c,
            _ => return None,
        })
    })
}

/// `text` lowered, as an index: in subscript letters when it has them all, else `_(text)`.
pub fn lowered(text: &str) -> String {
    shifted(text, '_', |c| {
        Some(match c {
            '0' => '₀',
            '1' => '₁',
            '2' => '₂',
            '3' => '₃',
            '4' => '₄',
            '5' => '₅',
            '6' => '₆',
            '7' => '₇',
            '8' => '₈',
            '9' => '₉',
            '+' => '₊',
            '−' | '-' => '₋',
            '=' => '₌',
            '(' => '₍',
            ')' => '₎',
            'a' => 'ₐ',
            'e' => 'ₑ',
            'h' => 'ₕ',
            'i' => 'ᵢ',
            'j' => 'ⱼ',
            'k' => 'ₖ',
            'l' => 'ₗ',
            'm' => 'ₘ',
            'n' => 'ₙ',
            'o' => 'ₒ',
            'p' => 'ₚ',
            'r' => 'ᵣ',
            's' => 'ₛ',
            't' => 'ₜ',
            'u' => 'ᵤ',
            'v' => 'ᵥ',
            'x' => 'ₓ',
            _ => return None,
        })
    })
}

fn shifted(text: &str, mark: char, map: impl Fn(char) -> Option<char>) -> String {
    let text = text.trim();
    if text.is_empty() {
        return String::new();
    }
    match text
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(&map)
        .collect::<Option<String>>()
    {
        Some(shifted) => shifted,
        None if text.chars().count() == 1 => format!("{mark}{text}"),
        None => format!("{mark}({text})"),
    }
}
