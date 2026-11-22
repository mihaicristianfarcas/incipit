//! What an EPUB's files need before their words can be read: XHTML made readable by an HTML
//! parser, the paths between the book's files, and the size a picture says it is.
//!
//! An EPUB's chapters are XHTML, and the parser is an HTML one (`scraper`, which a page that is
//! not quite XML does not stop). The one place the two differ that matters here is an element
//! closed in its own tag: `<span id="page12"/>` is closed in XML, but HTML reads it as opened, so
//! everything after it falls inside it, and the paragraphs that follow a page marker would become
//! one. `open_closed` writes such a tag out as an opening and a closing tag first, leaving the
//! elements HTML knows to be empty (`<br/>`, `<img/>`) as they are.

use std::borrow::Cow;

use percent_encoding::percent_decode_str;

/// The elements HTML knows to have no content: a tag of one of these closed in itself is
/// already read right.
const EMPTY: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// `xhtml` with every element closed in its own tag (`<a id="x"/>`) written as an opening and a
/// closing tag (`<a id="x"></a>`), but for the elements HTML has as empty. Comments, CDATA and
/// processing instructions are passed over, and a `>` inside a quoted attribute does not end its
/// tag.
pub fn open_closed(xhtml: &str) -> Cow<'_, str> {
    if !xhtml.contains("/>") {
        return Cow::Borrowed(xhtml);
    }
    let bytes = xhtml.as_bytes();
    let mut out = String::with_capacity(xhtml.len() + xhtml.len() / 16);
    let mut copied = 0;
    let mut i = 0;
    while let Some(lt) = memchr(b'<', &bytes[i..]).map(|at| i + at) {
        let rest = &xhtml[lt..];
        // What is not an opening tag is passed over whole.
        let passed = [
            ("<!--", "-->"),
            ("<![CDATA[", "]]>"),
            ("<!", ">"),
            ("<?", ">"),
            ("</", ">"),
        ]
        .iter()
        .find(|(start, _)| rest.starts_with(start));
        if let Some((_, end)) = passed {
            match rest.find(end) {
                Some(at) => {
                    i = lt + at + end.len();
                    continue;
                }
                None => break,
            }
        }
        let Some(end) = tag_end(bytes, lt + 1) else {
            break;
        };
        let name_end = xhtml[lt + 1..end]
            .find(|c: char| c.is_whitespace() || c == '/' || c == '>')
            .map_or(end, |at| lt + 1 + at);
        let name = &xhtml[lt + 1..name_end];
        let closed = end > lt + 1 && bytes[end - 1] == b'/';
        if closed && !name.is_empty() && !EMPTY.contains(&name.to_ascii_lowercase().as_str()) {
            out.push_str(&xhtml[copied..end - 1]);
            out.push_str("></");
            out.push_str(name);
            out.push('>');
            copied = end + 1;
        }
        i = end + 1;
    }
    out.push_str(&xhtml[copied..]);
    Cow::Owned(out)
}

/// Where the tag that starts at `from` (just after its `<`) ends: the index of its `>`, not
/// counting one inside quotes. `None` when it never ends.
fn tag_end(bytes: &[u8], from: usize) -> Option<usize> {
    let mut quote = None;
    for (at, &b) in bytes.iter().enumerate().skip(from) {
        match (quote, b) {
            (None, b'"' | b'\'') => quote = Some(b),
            (Some(q), b) if b == q => quote = None,
            (None, b'>') => return Some(at),
            _ => {}
        }
    }
    None
}

fn memchr(needle: u8, haystack: &[u8]) -> Option<usize> {
    haystack.iter().position(|&b| b == needle)
}

/// Where a link or a picture in the book leads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Href {
    /// A file of the book, by its path from the book's root, and a place in it (the `id` after
    /// the `#`), when the link names one.
    Inside(String, Option<String>),
    /// Somewhere outside the book: a web address, a mail address.
    Outside(String),
}

/// What `href` leads to, written in the file at `from` (its path from the book's root). A
/// relative path is taken from that file's folder, `..` and `.` worked out and `%20` and the
/// like decoded, as the files are named in the book's archive. A path that climbs out of the
/// book is no place in it.
pub fn resolve(from: &str, href: &str) -> Option<Href> {
    let href = href.trim();
    if href.is_empty() {
        return None;
    }
    // A scheme (`https:`, `mailto:`) before any `/`, `#` or `?` makes it a place outside.
    let scheme = href
        .find(':')
        .filter(|&at| at > 0 && !href[..at].contains(['/', '#', '?']));
    if scheme.is_some() {
        return Some(Href::Outside(href.to_owned()));
    }
    let (path, fragment) = match href.split_once('#') {
        Some((path, fragment)) => (path, Some(fragment)),
        None => (href, None),
    };
    let path = path.split('?').next().unwrap_or_default();
    let fragment = fragment
        .filter(|f| !f.is_empty())
        .map(|f| percent_decode_str(f).decode_utf8_lossy().into_owned());
    if path.is_empty() {
        // A place in the same file.
        return Some(Href::Inside(from.to_owned(), fragment));
    }
    let decoded = percent_decode_str(path).decode_utf8_lossy();
    let mut parts: Vec<&str> = if decoded.starts_with('/') {
        Vec::new()
    } else {
        from.split('/').collect()
    };
    // The file's own name is not part of its folder.
    if !decoded.starts_with('/') {
        parts.pop();
    }
    for part in decoded.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(Href::Inside(parts.join("/"), fragment))
}

/// The width and height a picture's file says it has, in pixels, read from its first bytes:
/// PNG, JPEG, GIF and WebP, and SVG from its root's `width` and `height` (`svg_size`). `None`
/// for another kind, an SVG sized only by its page, or a file too short or broken to say.
pub fn picture_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if let Some(size) = svg_size(bytes) {
        return Some(size);
    }
    let be16 = |at: usize| {
        Some(u32::from(u16::from_be_bytes(
            bytes.get(at..at + 2)?.try_into().ok()?,
        )))
    };
    let le16 = |at: usize| {
        Some(u32::from(u16::from_le_bytes(
            bytes.get(at..at + 2)?.try_into().ok()?,
        )))
    };
    let be32 = |at: usize| Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some((be32(16)?, be32(20)?));
    }
    if bytes.starts_with(b"GIF8") {
        return Some((le16(6)?, le16(8)?));
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        let le24 = |at: usize| {
            let b = bytes.get(at..at + 3)?;
            Some(u32::from(b[0]) | u32::from(b[1]) << 8 | u32::from(b[2]) << 16)
        };
        return match bytes.get(12..16)? {
            b"VP8 " => Some((le16(26)? & 0x3fff, le16(28)? & 0x3fff)),
            b"VP8L" => {
                let b = bytes.get(21..25)?;
                let bits = u32::from_le_bytes(b.try_into().ok()?);
                Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
            }
            b"VP8X" => Some((le24(24)? + 1, le24(27)? + 1)),
            _ => None,
        };
    }
    if bytes.starts_with(&[0xff, 0xd8]) {
        // The markers, each with its length, up to the frame's, which holds the size.
        let mut at = 2;
        while at + 9 < bytes.len() {
            if bytes[at] != 0xff {
                return None;
            }
            let marker = bytes[at + 1];
            let length = be16(at + 2)? as usize;
            let frame = matches!(marker, 0xc0..=0xcf) && !matches!(marker, 0xc4 | 0xc8 | 0xcc);
            if frame {
                return Some((be16(at + 7)?, be16(at + 5)?));
            }
            at += 2 + length;
        }
    }
    None
}

/// How many pixels each unit an SVG may be sized in is, at the size a book's text is set in
/// (16 pixels to the em, and an x-height of half that, as browsers take it).
const UNITS: &[(&str, f32)] = &[
    ("px", 1.0),
    ("pt", 4.0 / 3.0),
    ("pc", 16.0),
    ("in", 96.0),
    ("cm", 96.0 / 2.54),
    ("mm", 96.0 / 25.4),
    ("em", 16.0),
    ("ex", 8.0),
];

/// An SVG's size in pixels, from its root element's `width` and `height` (a formula from MathJax
/// is sized in `ex`). `None` when the file is not an SVG or either is missing or a share of the
/// page.
fn svg_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let head = std::str::from_utf8(&bytes[..bytes.len().min(4096)]).ok()?;
    let start = head.find("<svg")?;
    let end = tag_end(head.as_bytes(), start + 1)?;
    let tag = &head[start..end];
    let length = |name: &str| -> Option<u32> {
        let at = tag
            .find(&format!(" {name}="))
            .map(|at| at + name.len() + 2)?;
        let quote = tag[at..].chars().next()?;
        let value = tag[at + 1..].split(quote).next()?.trim();
        let (number, unit) = value
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .map_or((value, ""), |at| value.split_at(at));
        let number: f32 = number.parse().ok()?;
        let per = if unit.is_empty() {
            1.0
        } else {
            UNITS.iter().find(|(u, _)| *u == unit.trim())?.1
        };
        Some((number * per).round() as u32)
    };
    Some((length("width")?, length("height")?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_element_closed_in_its_tag_is_opened_and_closed_but_an_empty_one_is_left() {
        assert_eq!(
            open_closed(r#"<p>One<span id="p1"/> two<br/><img src="a.png" /></p>"#),
            r#"<p>One<span id="p1"></span> two<br/><img src="a.png" /></p>"#
        );
    }

    #[test]
    fn comments_quotes_and_declarations_are_not_taken_for_tags() {
        let xhtml =
            r#"<?xml version="1.0"?><!DOCTYPE html><!-- <a/> --><a title="x/>y" href="b"/>"#;
        assert_eq!(
            open_closed(xhtml),
            r#"<?xml version="1.0"?><!DOCTYPE html><!-- <a/> --><a title="x/>y" href="b"></a>"#
        );
        // Text with no tag closed in itself comes back as it was.
        assert!(matches!(open_closed("<p>a</p>"), Cow::Borrowed(_)));
    }

    #[test]
    fn a_path_is_taken_from_the_files_folder_and_decoded() {
        let inside = |path: &str, fragment: Option<&str>| {
            Some(Href::Inside(path.to_owned(), fragment.map(str::to_owned)))
        };
        assert_eq!(
            resolve("OEBPS/Text/ch1.xhtml", "../Images/fig%201.png"),
            inside("OEBPS/Images/fig 1.png", None)
        );
        assert_eq!(
            resolve("OEBPS/Text/ch1.xhtml", "ch2.xhtml#note-3"),
            inside("OEBPS/Text/ch2.xhtml", Some("note-3"))
        );
        assert_eq!(
            resolve("OEBPS/Text/ch1.xhtml", "#fn1"),
            inside("OEBPS/Text/ch1.xhtml", Some("fn1"))
        );
        assert_eq!(
            resolve("ch1.xhtml", "./img/a.jpg"),
            inside("img/a.jpg", None)
        );
        assert_eq!(
            resolve("a/ch1.xhtml", "https://example.org/x#y"),
            Some(Href::Outside("https://example.org/x#y".into()))
        );
        assert_eq!(resolve("ch1.xhtml", "../../etc/passwd"), None);
        assert_eq!(resolve("ch1.xhtml", " "), None);
    }

    #[test]
    fn a_pictures_size_is_read_from_its_header() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&640u32.to_be_bytes());
        png.extend_from_slice(&20u32.to_be_bytes());
        assert_eq!(picture_size(&png), Some((640, 20)));
        let gif = b"GIF89a\x10\x00\x08\x00";
        assert_eq!(picture_size(gif), Some((16, 8)));
        // A JPEG: start, an APP0 segment, then a baseline frame of 300 by 200.
        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0x00, 0x00];
        jpeg.extend_from_slice(&[0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0xc8, 0x01, 0x2c, 0x03]);
        assert_eq!(picture_size(&jpeg), Some((300, 200)));
        assert_eq!(picture_size(b"<svg/>"), None);
        let formula = br#"<svg xmlns="http://www.w3.org/2000/svg" width="1.355ex" height="2.176ex" viewBox="0 0 5 9">"#;
        assert_eq!(picture_size(formula), Some((11, 17)));
        assert_eq!(
            picture_size(br#"<?xml?><svg width="300" height='150pt'>"#),
            Some((300, 200))
        );
        assert_eq!(picture_size(br#"<svg width="100%" height="100%">"#), None);
        assert_eq!(picture_size(b"\x89PNG\r\n\x1a\n"), None);
    }
}
