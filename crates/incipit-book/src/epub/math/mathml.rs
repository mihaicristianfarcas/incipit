//! MathML, read by its structure: a subscript as a subscript, a fraction as a fraction, a root
//! as a root. Its bare text would run them together (`<msup>x 2</msup>` reads "x2").

use scraper::ElementRef;

use super::{lowered, raised};

/// A MathML formula as Unicode math, from its structure. Empty when it has no presentation
/// markup (only an annotation).
pub fn mathml(math: ElementRef) -> String {
    let out = mathml_children(math);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn mathml_children(element: ElementRef) -> String {
    element
        .children()
        .filter_map(|child| match child.value() {
            scraper::Node::Element(_) => ElementRef::wrap(child).map(mathml_element),
            scraper::Node::Text(text) => Some(text.trim().to_owned()),
            _ => None,
        })
        .collect()
}

fn mathml_element(element: ElementRef) -> String {
    let parts = || -> Vec<String> {
        element
            .children()
            .filter_map(ElementRef::wrap)
            .map(mathml_element)
            .collect()
    };
    let text = || element.text().collect::<String>().trim().to_owned();
    match element.value().name() {
        "mi" | "mn" | "mtext" | "ms" => text(),
        "mo" => {
            let op = text().replace('-', "−");
            match op.as_str() {
                "=" | "+" | "−" | "<" | ">" | "≤" | "≥" | "≈" | "≠" | "→" | "←" | "⇒" | "×"
                | "±" | "∈" | "≡" => format!(" {op} "),
                "," | ";" => format!("{op} "),
                _ => op,
            }
        }
        "mspace" => " ".to_owned(),
        "msub" => {
            let p = parts();
            format!(
                "{}{}",
                p.first().cloned().unwrap_or_default(),
                lowered(p.get(1).map_or("", String::as_str))
            )
        }
        "msup" => {
            let p = parts();
            format!(
                "{}{}",
                p.first().cloned().unwrap_or_default(),
                raised(p.get(1).map_or("", String::as_str))
            )
        }
        "msubsup" | "munderover" => {
            let p = parts();
            format!(
                "{}{}{}",
                p.first().cloned().unwrap_or_default(),
                lowered(p.get(1).map_or("", String::as_str)),
                raised(p.get(2).map_or("", String::as_str))
            )
        }
        "munder" => {
            let p = parts();
            format!(
                "{}{}",
                p.first().cloned().unwrap_or_default(),
                lowered(p.get(1).map_or("", String::as_str))
            )
        }
        "mover" => {
            let p = parts();
            let base = p.first().cloned().unwrap_or_default();
            let over = p.get(1).map_or("", |s| s.trim());
            // An accent over a letter is that letter with the accent on it.
            let accent = match over {
                "¯" | "‾" | "_" | "―" => Some('\u{304}'),
                "^" | "ˆ" => Some('\u{302}'),
                "~" | "˜" => Some('\u{303}'),
                "˙" | "." => Some('\u{307}'),
                "¨" => Some('\u{308}'),
                "→" | "⃗" => Some('\u{20D7}'),
                _ => None,
            };
            match accent {
                Some(accent) => format!("{base}{accent}"),
                None => format!("{base}{}", raised(over)),
            }
        }
        "mfrac" => {
            let p = parts();
            let wrap = |part: &str| {
                let part = part.trim();
                if part.chars().count() > 1 && part.chars().any(|c| !c.is_alphanumeric()) {
                    format!("({part})")
                } else {
                    part.to_owned()
                }
            };
            format!(
                "{}/{}",
                wrap(p.first().map_or("", String::as_str)),
                wrap(p.get(1).map_or("", String::as_str))
            )
        }
        "msqrt" => {
            let inner = mathml_children(element);
            let inner = inner.trim();
            if inner.chars().count() > 1 {
                format!("√({inner})")
            } else {
                format!("√{inner}")
            }
        }
        "mroot" => {
            let p = parts();
            format!(
                "{}√({})",
                raised(p.get(1).map_or("", String::as_str)),
                p.first().map_or("", String::as_str).trim()
            )
        }
        "mfenced" => {
            let e = element.value();
            let open = e.attr("open").unwrap_or("(");
            let close = e.attr("close").unwrap_or(")");
            let separator = e.attr("separators").unwrap_or(",").trim();
            let separator = separator
                .chars()
                .next()
                .map_or(String::new(), |c| format!("{c} "));
            format!("{open}{}{close}", parts().join(&separator))
        }
        "mtable" => element
            .children()
            .filter_map(ElementRef::wrap)
            .map(|row| {
                row.children()
                    .filter_map(ElementRef::wrap)
                    .map(|cell| mathml_children(cell).trim().to_owned())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>()
            .join("; "),
        "semantics" => element
            .children()
            .filter_map(ElementRef::wrap)
            .find(|child| !child.value().name().starts_with("annotation"))
            .map(mathml_element)
            .unwrap_or_default(),
        "annotation" | "annotation-xml" | "mphantom" | "none" | "mprescripts" => String::new(),
        _ => mathml_children(element),
    }
}
