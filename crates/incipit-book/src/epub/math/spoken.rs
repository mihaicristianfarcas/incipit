//! Spoken math, the notation screen readers speak and pictures of formulas are often described
//! in: "StartFraction upper E 0 Over c squared EndFraction" is `E₀/c²`. It is read word by
//! word into pieces (a variable, a number, an operator, a group), and the pieces are joined with
//! the spaces printed math has: none between a number and a variable, one around an operator.
//! Words that are not this notation (a picture's description) give `None`, so they are kept as
//! they are.

use super::{capital, double_struck, greek, lowered, raised, FUNCTIONS};

/// What a piece of a formula is, for the spaces around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    /// A variable, a number, a fraction: what operators join. Juxtaposed ones are a product,
    /// written with no space (`ct`).
    Operand,
    /// A function's name (`sin`), with a space before what it applies to.
    Function,
    /// An operator written with a space either side (`=`, `+`).
    Binary,
    /// An operator written with none (`/`, `·`).
    Tight,
    /// A sign before what it applies to (`−1`).
    Unary,
    /// `,` `;` `:`: no space before, one after.
    Comma,
    /// A full stop at the end.
    Stop,
    /// An ellipsis among terms, with a space either side.
    Spaced,
    Open,
    Close,
}

#[derive(Clone, Debug)]
struct Piece {
    text: String,
    role: Role,
    /// A variable (a letter, a Greek letter, with its primes): a number right after it is its
    /// index, as spoken math says `x 1` for x₁.
    variable: bool,
}

impl Piece {
    fn new(text: impl Into<String>, role: Role) -> Piece {
        Piece {
            text: text.into(),
            role,
            variable: false,
        }
    }

    fn variable(text: impl Into<String>) -> Piece {
        Piece {
            variable: true,
            ..Piece::new(text, Role::Operand)
        }
    }
}

/// Spoken math as Unicode math, or `None` when `said` is not spoken math (a word it does not
/// know, a structure left open).
pub fn spoken(said: &str) -> Option<String> {
    let words: Vec<&str> = said.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }
    let mut at = 0;
    let pieces = sequence(&words, &mut at, &[])?;
    (at == words.len() && !pieces.is_empty()).then(|| joined(&pieces))
}

/// The pieces of `words` from `at` until one of `until` (left for the caller) or the end.
fn sequence(words: &[&str], at: &mut usize, until: &[&str]) -> Option<Vec<Piece>> {
    let mut pieces: Vec<Piece> = Vec::new();
    while let Some(&word) = words.get(*at).filter(|w| !until.contains(w)) {
        *at += 1;
        match word {
            // A typeface's name: the letter after it is read as it is.
            "normal" | "bold" | "italic" | "script" | "fraktur" | "sans-serif" => {}
            "upper" => {
                let letter = words.get(*at)?;
                *at += 1;
                pieces.push(Piece::variable(capital(letter)?));
            }
            "double-struck" => {
                let letter = match *words.get(*at)? {
                    "upper" => {
                        *at += 1;
                        words.get(*at)?
                    }
                    letter => letter,
                };
                *at += 1;
                pieces.push(Piece::variable(double_struck(letter)?));
            }
            "prime" => attach(&mut pieces, "′"),
            "double-prime" => attach(&mut pieces, "″"),
            "triple-prime" => attach(&mut pieces, "‴"),
            "squared" => attach(&mut pieces, "²"),
            "cubed" => attach(&mut pieces, "³"),
            "degree" | "degrees" => attach(&mut pieces, "°"),
            "percent" => attach(&mut pieces, "%"),
            "Superscript" | "Subscript" => {
                let inner = sequence(words, at, &["Baseline"])?;
                ended(words, at, "Baseline");
                let inner = joined(&inner);
                let shifted = if word == "Superscript" {
                    raised(&inner)
                } else {
                    lowered(&inner)
                };
                attach(&mut pieces, &shifted);
            }
            "StartFraction" => {
                let over = sequence(words, at, &["Over"])?;
                expect(words, at, "Over")?;
                let under = sequence(words, at, &["EndFraction"])?;
                expect(words, at, "EndFraction")?;
                pieces.push(Piece::new(fraction(&over, &under), Role::Operand));
            }
            "StartRoot" => {
                let inner = sequence(words, at, &["EndRoot"])?;
                expect(words, at, "EndRoot")?;
                pieces.push(Piece::new(format!("√{}", grouped(&inner)), Role::Operand));
            }
            "StartAbsoluteValue" => {
                let inner = sequence(words, at, &["EndAbsoluteValue"])?;
                expect(words, at, "EndAbsoluteValue")?;
                pieces.push(Piece::new(format!("|{}|", joined(&inner)), Role::Operand));
            }
            "StartLayout" => pieces.push(Piece::new(layout(words, at)?, Role::Operand)),
            "long" => {
                let arrow = match *words.get(*at)? {
                    "right-arrow" => "⟶",
                    "left-arrow" => "⟵",
                    "left-right-arrow" => "⟷",
                    _ => return None,
                };
                *at += 1;
                pieces.push(Piece::new(arrow, Role::Binary));
            }
            "n-ary" => {
                let sign = match *words.get(*at)? {
                    "summation" => "∑",
                    "product" => "∏",
                    "union" => "⋃",
                    "intersection" => "⋂",
                    _ => return None,
                };
                *at += 1;
                pieces.push(Piece::new(sign, Role::Operand));
            }
            number if is_number(number) => match pieces.last_mut() {
                // A number right after a variable is its index: `x 1` is x₁.
                Some(last) if last.variable => last.text.push_str(&lowered(number)),
                _ => pieces.push(Piece::new(number, Role::Operand)),
            },
            letter if letter.len() == 1 && letter.chars().all(|c| c.is_ascii_alphabetic()) => {
                pieces.push(Piece::variable(letter))
            }
            other => pieces.push(named(other)?),
        }
    }
    Some(pieces)
}

/// A word of spoken math that stands alone: a Greek letter, an operator, a bracket, a sign.
fn named(word: &str) -> Option<Piece> {
    if let Some(letter) = greek(word) {
        return Some(Piece::variable(letter));
    }
    if FUNCTIONS.contains(&word) {
        return Some(Piece::new(word, Role::Function));
    }
    let (text, role) = match word {
        "plus" => ("+", Role::Binary),
        "minus" => ("−", Role::Binary),
        "plus-or-minus" => ("±", Role::Binary),
        "minus-or-plus" => ("∓", Role::Binary),
        "times" => ("×", Role::Binary),
        "divided-by" => ("÷", Role::Binary),
        "equals" => ("=", Role::Binary),
        "not-equals" => ("≠", Role::Binary),
        "less-than" => ("<", Role::Binary),
        "greater-than" => (">", Role::Binary),
        "less-than-or-equal-to" | "less-than-or-equals" => ("≤", Role::Binary),
        "greater-than-or-equal-to" | "greater-than-or-equals" => ("≥", Role::Binary),
        "approximately-equals" | "almost-equals" => ("≈", Role::Binary),
        "identical-to" => ("≡", Role::Binary),
        "proportional-to" => ("∝", Role::Binary),
        "tilde" => ("∼", Role::Binary),
        "right-arrow" => ("→", Role::Binary),
        "left-arrow" => ("←", Role::Binary),
        "left-right-arrow" => ("↔", Role::Binary),
        "element-of" => ("∈", Role::Binary),
        "slash" => ("/", Role::Tight),
        "dot" | "dot-operator" | "middle-dot" => ("·", Role::Tight),
        "negative" => ("−", Role::Unary),
        "comma" => (",", Role::Comma),
        "semicolon" => (";", Role::Comma),
        "colon" => (":", Role::Comma),
        "period" => (".", Role::Stop),
        "ellipsis" | "horizontal-ellipsis" | "midline-horizontal-ellipsis" => ("…", Role::Spaced),
        "left-parenthesis" => ("(", Role::Open),
        "right-parenthesis" => (")", Role::Close),
        "left-bracket" => ("[", Role::Open),
        "right-bracket" => ("]", Role::Close),
        "left-brace" => ("{", Role::Open),
        "right-brace" => ("}", Role::Close),
        "left-angle-bracket" => ("⟨", Role::Open),
        "right-angle-bracket" => ("⟩", Role::Close),
        "vertical-bar" => ("|", Role::Operand),
        "one-half" => ("½", Role::Operand),
        "one-third" => ("⅓", Role::Operand),
        "two-thirds" => ("⅔", Role::Operand),
        "one-fourth" | "one-quarter" => ("¼", Role::Operand),
        "three-fourths" | "three-quarters" => ("¾", Role::Operand),
        "infinity" => ("∞", Role::Operand),
        "partial-differential" => ("∂", Role::Operand),
        "nabla" => ("∇", Role::Operand),
        "integral" => ("∫", Role::Operand),
        _ => return None,
    };
    Some(Piece::new(text, role))
}

fn is_number(word: &str) -> bool {
    word.chars().any(|c| c.is_ascii_digit()) && word.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// Writes `mark` (a prime, a power) on the piece before it, or alone when there is none.
fn attach(pieces: &mut Vec<Piece>, mark: &str) {
    match pieces.last_mut() {
        Some(last) if matches!(last.role, Role::Operand | Role::Close) => last.text.push_str(mark),
        _ => pieces.push(Piece::new(mark, Role::Operand)),
    }
}

fn expect(words: &[&str], at: &mut usize, word: &str) -> Option<()> {
    (words.get(*at) == Some(&word)).then(|| *at += 1)
}

/// Steps over `word` when it is next: a structure the notation may leave unclosed at the end.
fn ended(words: &[&str], at: &mut usize, word: &str) {
    if words.get(*at) == Some(&word) {
        *at += 1;
    }
}

/// A layout (aligned equations, cases) from after `StartLayout` to its `EndLayout`: its rows,
/// each read as a formula, apart by "; ".
fn layout(words: &[&str], at: &mut usize) -> Option<String> {
    let mut depth = 0;
    let start = *at;
    while *at < words.len() {
        match words[*at] {
            "StartLayout" => depth += 1,
            "EndLayout" if depth == 0 => break,
            "EndLayout" => depth -= 1,
            _ => {}
        }
        *at += 1;
    }
    let inside = &words[start..*at];
    expect(words, at, "EndLayout")?;
    let mut rows: Vec<Vec<&str>> = Vec::new();
    let mut i = 0;
    while i < inside.len() {
        let ordinal = is_ordinal(inside[i]);
        match inside.get(i + 1) {
            Some(&"Row") if ordinal => {
                rows.push(Vec::new());
                i += 2;
                continue;
            }
            Some(&"Column") if ordinal => {
                i += 2;
                continue;
            }
            _ => {}
        }
        match rows.last_mut() {
            Some(row) => row.push(inside[i]),
            None => rows.push(vec![inside[i]]),
        }
        i += 1;
    }
    let rows = rows
        .iter()
        .map(|row| {
            let mut at = 0;
            let pieces = sequence(row, &mut at, &[])?;
            (at == row.len()).then(|| joined(&pieces))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(rows.join("; "))
}

/// `1st`, `2nd`, `3rd`, `12th`.
fn is_ordinal(word: &str) -> bool {
    let digits = word.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit())
        && matches!(&word[digits.len()..], "st" | "nd" | "rd" | "th")
}

/// A fraction on a line: its parts in brackets when an operator or more than one term would
/// make it read otherwise.
fn fraction(over: &[Piece], under: &[Piece]) -> String {
    let top = if has_operator(over) {
        format!("({})", joined(over))
    } else {
        joined(over)
    };
    format!("{top}/{}", grouped(under))
}

/// The pieces as one term: in brackets when there is more than one.
fn grouped(pieces: &[Piece]) -> String {
    if pieces.len() > 1 {
        format!("({})", joined(pieces))
    } else {
        joined(pieces)
    }
}

fn has_operator(pieces: &[Piece]) -> bool {
    pieces.iter().any(|p| {
        matches!(
            p.role,
            Role::Binary | Role::Tight | Role::Comma | Role::Spaced | Role::Unary
        )
    })
}

/// The pieces written out, spaced as printed math is: operators apart, products together.
fn joined(pieces: &[Piece]) -> String {
    let mut out = String::new();
    let mut before: Option<Role> = None;
    let mut open = 0usize;
    for piece in pieces {
        let mut role = piece.role;
        // A plus or minus with nothing before it to apply to is a sign.
        if role == Role::Binary
            && matches!(piece.text.as_str(), "−" | "+" | "±" | "∓")
            && matches!(
                before,
                None | Some(
                    Role::Binary
                        | Role::Tight
                        | Role::Unary
                        | Role::Open
                        | Role::Comma
                        | Role::Spaced
                )
            )
        {
            role = Role::Unary;
        }
        match role {
            Role::Binary | Role::Spaced => {
                trim_end(&mut out);
                out.push(' ');
                out.push_str(&piece.text);
                out.push(' ');
            }
            Role::Tight | Role::Stop => {
                trim_end(&mut out);
                out.push_str(&piece.text);
            }
            Role::Comma => {
                trim_end(&mut out);
                out.push_str(&piece.text);
                out.push(' ');
            }
            Role::Close => {
                // A bracket that closes nothing opened here (the brace that ends a case
                // layout) is left out.
                if open == 0 {
                    continue;
                }
                open -= 1;
                trim_end(&mut out);
                out.push_str(&piece.text);
            }
            Role::Open | Role::Operand | Role::Unary | Role::Function => {
                if before == Some(Role::Function)
                    || (role == Role::Function
                        && matches!(before, Some(Role::Operand | Role::Close)))
                {
                    out.push(' ');
                }
                if role == Role::Open {
                    open += 1;
                }
                out.push_str(&piece.text);
            }
        }
        before = Some(role);
    }
    out.trim().to_owned()
}

fn trim_end(out: &mut String) {
    out.truncate(out.trim_end().len());
}
