//! What the book crate makes of a file:
//!   look LIBRARY_DIR FILE.pdf    pages, and the first page's size, characters and opening text
//!   look - FILE.epub             the title, and how many blocks of each kind, with the first few

use std::path::Path;
use std::time::Instant;

use incipit_book::{Block, Book, Pdf};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [library, file] = &args[..] else {
        return eprintln!("usage: look LIBRARY_DIR FILE");
    };
    if file.ends_with(".pdf") {
        let started = Instant::now();
        let pdf = Pdf::open(Path::new(library), Path::new(file)).expect("the PDF opens");
        let opened = started.elapsed();
        let page = pdf.page(0, 1400).expect("the first page is drawn");
        println!(
            "{} pages, opened in {opened:?}; page 1 drawn in {:?}",
            pdf.pages(),
            started.elapsed() - opened
        );
        println!(
            "{} x {} pt, {} x {} px, {} characters",
            page.width,
            page.height,
            page.pixel_width,
            page.pixel_height,
            page.glyphs.len()
        );
        println!("{}", page.text().iter().take(300).collect::<String>());
    } else {
        let started = Instant::now();
        let book = Book::open(Path::new(file)).expect("the EPUB opens");
        let count = |f: fn(&Block) -> bool| book.blocks.iter().filter(|b| f(b)).count();
        println!(
            "{:?} in {:?}: {} headings, {} paragraphs, {} pictures",
            book.title,
            started.elapsed(),
            count(|b| matches!(b, Block::Heading { .. })),
            count(|b| matches!(b, Block::Paragraph(_))),
            count(|b| matches!(b, Block::Image { .. })),
        );
        for block in book.blocks.iter().skip(40).take(6) {
            match block {
                Block::Heading { level, text } => println!("h{level}: {}", text.text),
                Block::Paragraph(p) => println!("p: {:.110} ({} spans)", p.text, p.spans.len()),
                Block::Image { name, bytes } => println!("img: {name} ({} bytes)", bytes.len()),
            }
        }
    }
}
