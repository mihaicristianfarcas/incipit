//! Made-up EPUBs, written into a temporary folder, with the markup real books use: chapters in
//! a folder of their own and pictures in another, page markers closed in their own tags, lists,
//! tables, figures, footnotes and both kinds of table of contents.

use std::io::Write;

use scraper::{Html, Selector};

use super::*;

/// A small PNG's header, of `w` by `h` pixels: enough for its size to be read.
fn png(w: u32, h: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    bytes.extend_from_slice(&w.to_be_bytes());
    bytes.extend_from_slice(&h.to_be_bytes());
    bytes
}

/// An EPUB of the files given (path, bytes), with a package listing `spine` in order and the
/// manifest's other `items` (id, path, media type, properties).
fn epub(
    files: &[(&str, Vec<u8>)],
    spine: &[&str],
    items: &[(&str, &str, &str, &str)],
    toc: Option<&str>,
) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("book.epub");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut add = |name: &str, bytes: &[u8]| {
        zip.start_file(name, options).unwrap();
        zip.write_all(bytes).unwrap();
    };
    add("mimetype", b"application/epub+zip");
    add(
        "META-INF/container.xml",
        br#"<?xml version="1.0"?><container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#,
    );
    let manifest: String = spine
        .iter()
        .enumerate()
        .map(|(i, href)| {
            let media = if href.ends_with(".png") {
                "image/png"
            } else {
                "application/xhtml+xml"
            };
            format!(r#"<item id="c{i}" href="{href}" media-type="{media}"/>"#)
        })
        .chain(items.iter().map(|(id, href, media, properties)| {
            format!(
                r#"<item id="{id}" href="{href}" media-type="{media}" properties="{properties}"/>"#
            )
        }))
        .collect();
    let itemrefs: String = (0..spine.len())
        .map(|i| format!(r#"<itemref idref="c{i}"/>"#))
        .collect();
    let toc = toc.map(|t| format!(r#" toc="{t}""#)).unwrap_or_default();
    let package = format!(
        r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0">
        <metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Waves</dc:title><dc:creator>A. Writer</dc:creator></metadata>
        <manifest>{manifest}</manifest><spine{toc}>{itemrefs}</spine></package>"#
    );
    add("OEBPS/content.opf", package.as_bytes());
    for (name, bytes) in files {
        add(name, bytes);
    }
    zip.finish().unwrap();
    (dir, path)
}

fn chapter(body: &str) -> Vec<u8> {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><head><title>x</title></head><body>{body}</body></html>"#
    )
    .into_bytes()
}

fn texts(book: &Book) -> Vec<String> {
    book.blocks
        .iter()
        .map(|b| match b {
            Block::Heading { level, text } => format!("h{level}: {}", text.text),
            Block::Paragraph(text) => text.text.clone(),
            Block::Text { kind, text } => format!("{kind:?}: {}", text.text),
            Block::Image { name, .. } => format!("picture: {name}"),
        })
        .collect()
}

#[test]
fn a_page_marker_closed_in_its_own_tag_does_not_join_the_paragraphs_after_it() {
    let (_dir, path) = epub(
        &[(
            "OEBPS/Text/one.xhtml",
            chapter(
                r#"<div><span epub:type="pagebreak" id="p7" title="7"/><p>First.</p><p>Second.</p></div>"#,
            ),
        )],
        &["Text/one.xhtml"],
        &[],
        None,
    );
    let book = Book::open(&path).unwrap();
    assert_eq!(texts(&book), ["First.", "Second."]);
    assert_eq!(book.title, "Waves");
    assert_eq!(book.author.as_deref(), Some("A. Writer"));
}

#[test]
fn pictures_in_another_folder_and_in_figures_are_kept_with_their_captions() {
    let (_dir, path) = epub(
        &[
            (
                "OEBPS/Text/one.xhtml",
                chapter(
                    r#"<figure id="f1"><img src="../Images/wave%20form.png" alt="A wave"/><figcaption>Figure 1. A wave.</figcaption></figure>
                    <p>The sign <img src="../Images/sym.png" alt="λ"/> is the length.</p>
                    <p>Before <img src="../Images/big.png"/> after.</p>"#,
                ),
            ),
            ("OEBPS/Images/wave form.png", png(600, 300)),
            ("OEBPS/Images/sym.png", png(12, 14)),
            ("OEBPS/Images/big.png", png(500, 400)),
        ],
        &["Text/one.xhtml"],
        &[],
        None,
    );
    let book = Book::open(&path).unwrap();
    assert_eq!(
        texts(&book),
        [
            "picture: OEBPS/Images/wave form.png",
            "Caption: Figure 1. A wave.",
            "The sign λ is the length.",
            "Before",
            "picture: OEBPS/Images/big.png",
            "after.",
        ]
    );
}

#[test]
fn lists_quotations_code_and_tables_are_kept_set_apart() {
    let (_dir, path) = epub(
        &[(
            "OEBPS/one.xhtml",
            chapter(
                r#"<ol start="3"><li>Third</li><li><p>Fourth</p><p>More of it</p><ul><li>Inner</li></ul></li></ol>
                <blockquote><p>Said elsewhere.</p></blockquote>
                <pre>fn main() {
    go();
}</pre>
                <table><caption>Speeds</caption><tr><th>Wave</th><th>Speed</th></tr><tr><td>Sound</td><td>343 m/s</td></tr></table>
                <div>Loose words <p>A paragraph.</p> more loose words</div>"#,
            ),
        )],
        &["one.xhtml"],
        &[],
        None,
    );
    let book = Book::open(&path).unwrap();
    assert_eq!(
        texts(&book),
        [
            "Item { depth: 1 }: 3. Third",
            "Item { depth: 1 }: 4. Fourth",
            "Item { depth: 1 }: More of it",
            "Item { depth: 2 }: • Inner",
            "Quote: Said elsewhere.",
            "Pre: fn main() {\n    go();\n}",
            "Caption: Speeds",
            "Row: Wave | Speed",
            "Row: Sound | 343 m/s",
            "Loose words",
            "A paragraph.",
            "more loose words",
        ]
    );
    // The header row is bold.
    let Block::Text { text, .. } = &book.blocks[7] else {
        panic!()
    };
    assert_eq!(text.spans.len(), 2);
    assert!(text.spans.iter().all(|s| s.bold));
}

#[test]
fn a_footnote_link_leads_to_its_note_and_a_web_link_keeps_its_address() {
    let (_dir, path) = epub(
        &[
            (
                "OEBPS/Text/one.xhtml",
                chapter(
                    r#"<p>Light bends<a href="notes.xhtml#n1">1</a>, as <a href="https://example.org/light">shown</a>.</p>
                    <p>A line<br/>and the next.</p>"#,
                ),
            ),
            (
                "OEBPS/Text/notes.xhtml",
                chapter(r#"<h2>Notes</h2><aside id="n1"><p>1. Near the sun.</p></aside>"#),
            ),
        ],
        &["Text/one.xhtml", "Text/notes.xhtml"],
        &[],
        None,
    );
    let book = Book::open(&path).unwrap();
    let Block::Paragraph(text) = &book.blocks[0] else {
        panic!()
    };
    assert_eq!(text.text, "Light bends1, as shown.");
    assert_eq!(
        text.links,
        [
            Link {
                start: 11,
                end: 12,
                to: Target::Block(3)
            },
            Link {
                start: 17,
                end: 22,
                to: Target::Web("https://example.org/light".into())
            },
        ]
    );
    assert_eq!(texts(&book)[1], "A line\nand the next.");
    assert_eq!(texts(&book)[3], "1. Near the sun.");
}

#[test]
fn the_contents_are_the_books_own_navigation_led_to_their_blocks() {
    let nav = r#"<html xmlns:epub="http://www.idpf.org/2007/ops"><body>
        <nav epub:type="landmarks"><ol><li><a href="Text/two.xhtml">Start</a></li></ol></nav>
        <nav epub:type="toc"><ol>
          <li><a href="Text/one.xhtml">One</a></li>
          <li><span>Part two</span><ol><li><a href="Text/two.xhtml#s2">Two, section</a></li></ol></li>
        </ol></nav></body></html>"#;
    let (_dir, path) = epub(
        &[
            ("OEBPS/Text/one.xhtml", chapter("<p>Words of one.</p>")),
            (
                "OEBPS/Text/two.xhtml",
                chapter(r#"<p>Intro.</p><h3 id="s2">Section</h3><p>Words.</p>"#),
            ),
            ("OEBPS/nav.xhtml", nav.as_bytes().to_vec()),
        ],
        &["Text/one.xhtml", "Text/two.xhtml"],
        &[("nav", "nav.xhtml", "application/xhtml+xml", "nav")],
        None,
    );
    let book = Book::open(&path).unwrap();
    let lines: Vec<(&str, usize, usize)> = book
        .contents
        .iter()
        .map(|e| (e.title.as_str(), e.depth, e.page))
        .collect();
    assert_eq!(lines, [("One", 0, 0), ("Two, section", 1, 2)]);
}

#[test]
fn an_epub2_books_contents_come_from_its_ncx_and_a_missing_chapter_is_passed_over() {
    let ncx = r#"<?xml version="1.0"?><ncx><navMap>
        <navPoint id="a"><navLabel><text>Chapter  One</text></navLabel><content src="one.xhtml"/>
          <navPoint id="b"><navLabel><text>Inside</text></navLabel><content src="one.xhtml#in"/></navPoint>
        </navPoint></navMap></ncx>"#;
    let (_dir, path) = epub(
        &[
            (
                "OEBPS/one.xhtml",
                chapter(r#"<p>Start.</p><p id="in">Inside it.</p>"#),
            ),
            ("OEBPS/toc.ncx", ncx.as_bytes().to_vec()),
        ],
        &["one.xhtml", "gone.xhtml"],
        &[("ncx", "toc.ncx", "application/x-dtbncx+xml", "")],
        Some("ncx"),
    );
    let book = Book::open(&path).unwrap();
    assert_eq!(texts(&book), ["Start.", "Inside it."]);
    let lines: Vec<(&str, usize, usize)> = book
        .contents
        .iter()
        .map(|e| (e.title.as_str(), e.depth, e.page))
        .collect();
    assert_eq!(lines, [("Chapter One", 0, 0), ("Inside", 1, 1)]);
}

#[test]
fn a_book_without_contents_of_its_own_has_its_headings() {
    let (_dir, path) = epub(
        &[(
            "OEBPS/one.xhtml",
            chapter("<h1>Title</h1><p>a</p><h2>Part</h2><h5>Small</h5>"),
        )],
        &["one.xhtml"],
        &[],
        None,
    );
    let book = Book::open(&path).unwrap();
    let lines: Vec<(&str, usize)> = book
        .contents
        .iter()
        .map(|e| (e.title.as_str(), e.depth))
        .collect();
    assert_eq!(lines, [("Title", 0), ("Part", 1)]);
}

#[test]
fn math_hidden_text_and_scripts_are_read_as_a_reader_sees_them() {
    let (_dir, path) = epub(
        &[(
            "OEBPS/one.xhtml",
            chapter(
                r#"<p>Energy is <math alttext="E = mc^2"><mi>E</mi><mo>=</mo><mi>m</mi><msup><mi>c</mi><mn>2</mn></msup></math> here.</p>
                <p>So <math><semantics><annotation encoding="application/x-tex">x^2</annotation></semantics></math>.</p>
                <p>Half is <math alttext="\frac{1}{2}"></math>.</p>
                <p>Then <img src="eq.svg" alt="" title="x prime equals c t prime"/> holds.</p>
                <p style="display: none">Hidden.</p><script>alert(1)</script><p onclick="x()">Shown.</p>"#,
            ),
        )],
        &["one.xhtml"],
        &[],
        None,
    );
    let book = Book::open(&path).unwrap();
    assert_eq!(
        texts(&book),
        [
            "Energy is E = mc² here.",
            "So x².",
            "Half is 1/2.",
            "Then x′ = ct′ holds.",
            "Shown."
        ]
    );
}

#[test]
fn a_book_locked_with_drm_is_refused_with_what_to_do_but_hidden_fonts_are_not() {
    let encrypted = |algorithm: &str| {
        format!(
            r#"<encryption><EncryptedData><EncryptionMethod Algorithm="{algorithm}"/><CipherData><CipherReference URI="OEBPS/one.xhtml"/></CipherData></EncryptedData></encryption>"#
        )
    };
    let files = |algorithm: &str| {
        vec![
            ("OEBPS/one.xhtml", chapter("<p>Words.</p>")),
            ("META-INF/encryption.xml", encrypted(algorithm).into_bytes()),
        ]
    };
    let (_dir, path) = epub(
        &files("http://www.w3.org/2001/04/xmlenc#aes128-cbc"),
        &["one.xhtml"],
        &[],
        None,
    );
    let refused = Book::open(&path).err().unwrap().to_string();
    assert!(refused.contains("DRM"), "{refused}");
    let (_dir, path) = epub(
        &files("http://www.idpf.org/2008/embedding"),
        &["one.xhtml"],
        &[],
        None,
    );
    assert!(Book::open(&path).is_ok());
}

#[test]
fn a_book_with_no_text_is_refused() {
    let (_dir, path) = epub(
        &[("OEBPS/one.xhtml", chapter("<div> </div>"))],
        &["one.xhtml"],
        &[],
        None,
    );
    assert!(Book::open(&path).is_err());
}

#[test]
fn markup_becomes_blocks_of_styled_text() {
    let page = Html::parse_document(
        "<body><div><h2>II<br/><span>Space</span></h2><p>It is <i>not</i>\n clear.</p></div></body>",
    );
    let body = page
        .select(&Selector::parse("body").unwrap())
        .next()
        .unwrap();
    let mut none = |_: &str| None;
    let mut walker = walk::Walker::new(&mut none);
    walker.chapter("one.xhtml", body);
    let (out, _) = walker.finish();
    let Block::Heading { level: 2, text } = &out[0] else {
        panic!("{out:?}")
    };
    assert_eq!(text.text, "II\nSpace");
    let Block::Paragraph(p) = &out[1] else {
        panic!("{out:?}")
    };
    assert_eq!(p.text, "It is not clear.");
    assert_eq!(
        p.spans,
        vec![Span {
            start: 6,
            end: 9,
            italic: true,
            bold: false
        }]
    );
}
