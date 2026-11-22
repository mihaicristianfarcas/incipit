//! TeX, as MathML's `alttext` or TeX annotation or a picture's alternative text gives it: the
//! commonest commands (fractions, roots, Greek letters, operators, accents), with indices and
//! powers written as Unicode where they can be. A command it does not know is left as its name.

use super::{capital, greek, lowered, raised, FUNCTIONS};

/// Whether a picture's alternative text is TeX rather than words.
pub fn looks_like_tex(text: &str) -> bool {
    let text = text.trim();
    text.contains('\\')
        || text.contains("^{")
        || text.contains("_{")
        || (text.len() > 1 && text.starts_with('$') && text.ends_with('$'))
}

/// TeX as Unicode math.
pub fn tex(source: &str) -> String {
    let source = source
        .trim()
        .trim_start_matches("\\(")
        .trim_end_matches("\\)")
        .trim_start_matches("\\[")
        .trim_end_matches("\\]")
        .trim_matches('$');
    let chars: Vec<char> = source.chars().collect();
    let mut at = 0;
    let out = tex_until(&chars, &mut at, None);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// TeX from `at` until `close` (a `}` that ends a group) or the end, as Unicode.
fn tex_until(chars: &[char], at: &mut usize, close: Option<char>) -> String {
    let mut out = String::new();
    while *at < chars.len() {
        let c = chars[*at];
        if Some(c) == close {
            *at += 1;
            return out;
        }
        *at += 1;
        match c {
            '{' => out.push_str(&tex_until(chars, at, Some('}'))),
            '}' => {}
            '^' => {
                let power = tex_argument(chars, at);
                out.push_str(&raised(&power));
            }
            '_' => {
                let index = tex_argument(chars, at);
                out.push_str(&lowered(&index));
            }
            '\'' => out.push('′'),
            '-' => out.push('−'),
            '*' => out.push('·'),
            '~' => out.push(' '),
            '\\' => out.push_str(&tex_command(chars, at)),
            c => out.push(c),
        }
    }
    out
}

/// The argument of `^`, `_` or a command: a group in braces, a command, or one character.
fn tex_argument(chars: &[char], at: &mut usize) -> String {
    while chars.get(*at).is_some_and(|c| c.is_whitespace()) {
        *at += 1;
    }
    match chars.get(*at) {
        Some('{') => {
            *at += 1;
            tex_until(chars, at, Some('}'))
        }
        Some('\\') => {
            *at += 1;
            tex_command(chars, at)
        }
        Some(c) => {
            *at += 1;
            match c {
                '-' => "−".to_owned(),
                '\'' => "′".to_owned(),
                c => c.to_string(),
            }
        }
        None => String::new(),
    }
}

/// A command after its backslash, as Unicode.
fn tex_command(chars: &[char], at: &mut usize) -> String {
    let start = *at;
    while chars.get(*at).is_some_and(|c| c.is_ascii_alphabetic()) {
        *at += 1;
    }
    let name: String = chars[start..*at].iter().collect();
    if name.is_empty() {
        // A symbol escaped: `\{`, `\,`, `\%`.
        let Some(&c) = chars.get(*at) else {
            return String::new();
        };
        *at += 1;
        return match c {
            ',' | ';' | ':' | '!' | ' ' => " ".to_owned(),
            '\\' => "; ".to_owned(),
            c => c.to_string(),
        };
    }
    if let Some(letter) = greek(&name) {
        return letter.to_string();
    }
    if let Some(big) =
        capital(&name).filter(|_| name.chars().next().is_some_and(char::is_uppercase))
    {
        return big;
    }
    if FUNCTIONS.contains(&name.as_str()) {
        return format!("{name} ");
    }
    let symbol = match name.as_str() {
        "frac" | "dfrac" | "tfrac" => {
            let over = tex_argument(chars, at);
            let under = tex_argument(chars, at);
            let wrap = |part: String| {
                if part.chars().count() > 1 && part.chars().any(|c| !c.is_alphanumeric()) {
                    format!("({part})")
                } else {
                    part
                }
            };
            return format!("{}/{}", wrap(over), wrap(under));
        }
        "sqrt" => {
            let degree = if chars.get(*at) == Some(&'[') {
                let end = chars[*at..].iter().position(|c| *c == ']').map(|e| *at + e);
                end.map(|end| {
                    let degree: String = chars[*at + 1..end].iter().collect();
                    *at = end + 1;
                    raised(&degree)
                })
            } else {
                None
            };
            let inner = tex_argument(chars, at);
            let inner = if inner.chars().count() > 1 {
                format!("({inner})")
            } else {
                inner
            };
            return format!("{}√{inner}", degree.unwrap_or_default());
        }
        "mathrm" | "mathbf" | "mathit" | "mathsf" | "mathtt" | "mathcal" | "mathbb"
        | "boldsymbol" | "text" | "textrm" | "textit" | "textbf" | "operatorname" | "mbox" => {
            return tex_argument(chars, at);
        }
        "left" | "right" | "big" | "Big" | "bigg" | "Bigg" | "displaystyle" | "textstyle"
        | "limits" | "nolimits" => "",
        "cdot" => "·",
        "times" => "×",
        "div" => "÷",
        "pm" => "±",
        "mp" => "∓",
        "le" | "leq" => "≤",
        "ge" | "geq" => "≥",
        "ne" | "neq" => "≠",
        "approx" => "≈",
        "equiv" => "≡",
        "sim" => "∼",
        "propto" => "∝",
        "infty" => "∞",
        "partial" => "∂",
        "nabla" => "∇",
        "sum" => "∑",
        "prod" => "∏",
        "int" => "∫",
        "oint" => "∮",
        "to" | "rightarrow" => "→",
        "leftarrow" => "←",
        "Rightarrow" => "⇒",
        "Leftarrow" => "⇐",
        "Leftrightarrow" | "iff" => "⇔",
        "leftrightarrow" => "↔",
        "mapsto" => "↦",
        "ldots" | "dots" | "cdots" => "…",
        "prime" => "′",
        "circ" => "∘",
        "in" => "∈",
        "notin" => "∉",
        "subset" => "⊂",
        "subseteq" => "⊆",
        "cup" => "∪",
        "cap" => "∩",
        "forall" => "∀",
        "exists" => "∃",
        "neg" | "lnot" => "¬",
        "land" | "wedge" => "∧",
        "lor" | "vee" => "∨",
        "langle" => "⟨",
        "rangle" => "⟩",
        "hbar" => "ħ",
        "ell" => "ℓ",
        "degree" => "°",
        "quad" | "qquad" => " ",
        "varepsilon" => "ε",
        "vartheta" => "ϑ",
        "varphi" => "φ",
        other => return other.to_owned(),
    };
    symbol.to_owned()
}
