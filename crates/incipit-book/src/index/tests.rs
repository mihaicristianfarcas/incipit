use super::*;
use crate::find::MOST;
use crate::pdf::Line;

/// A page whose lines are 0.05 of its height apart, the first 0.1 down, as the earlier app's
/// tests made them.
fn page(lines: &[&str]) -> PageText {
    let mut text = String::new();
    let mut placed = Vec::new();
    for (n, line) in lines.iter().enumerate() {
        let start = text.chars().count();
        text.push_str(line);
        placed.push(Line {
            start,
            end: start + line.chars().count(),
            top: 0.1 + n as f32 * 0.05,
        });
        text.push('\n');
    }
    PageText {
        text,
        lines: placed,
    }
}

fn pages() -> [PageText; 2] {
    [
        page(&[
            "All through the calculus we are dealing with quantities that are grow-",
            "ing, and with rates of growth.",
        ]),
        page(&["A ﬁnal word on Café Growth.", "Nothing here."]),
    ]
}

fn entry(title: &str, page: usize, y: Option<f32>) -> Entry {
    Entry {
        title: title.into(),
        depth: 0,
        page,
        y,
    }
}

fn book() -> TextIndex {
    TextIndex::of_pages(&pages(), Vec::new(), Source::default())
}

fn quotes(hits: &[Hit]) -> Vec<(usize, &str)> {
    hits.iter().map(|h| (h.part, h.quote.as_str())).collect()
}

#[test]
fn a_word_hyphenated_across_two_lines_is_found_whole_with_the_line_it_starts_on() {
    let hits = book().find("growing", MOST);
    assert_eq!(quotes(&hits), [(0, "growing")]);
    assert!((hits[0].y.unwrap() - 0.08).abs() < 1e-6);
    assert!(hits[0].suffix.starts_with(", and with rates"));
    // In the page's own text it runs over the hyphen and the line end.
    let text: Vec<char> = pages()[0].text.chars().collect();
    let marked: String = text[hits[0].start..hits[0].end].iter().collect();
    assert_eq!(marked, "grow-\ning");
}

#[test]
fn case_accents_and_ligatures_do_not_matter() {
    let book = book();
    assert_eq!(
        quotes(&book.find("cafe growth", MOST)),
        [(1, "Café Growth")]
    );
    assert_eq!(quotes(&book.find("final", MOST)), [(1, "ﬁnal")]);
    assert_eq!(book.find("GROWTH", MOST).len(), 2);
}

#[test]
fn a_phrases_words_begin_words_one_after_another_on_one_page() {
    let book = book();
    // Over a line break and punctuation, but not from one page to the next.
    assert_eq!(
        quotes(&book.find("growing and", MOST)),
        [(0, "growing, and")]
    );
    assert_eq!(book.find("growth a", MOST), []);
    assert_eq!(
        quotes(&book.find("rat of   gro", MOST)),
        [(0, "rates of gro")]
    );
    // The words must be next to one another, in the order typed.
    assert_eq!(book.find("rates growth", MOST), []);
    assert_eq!(book.find("growth of", MOST), []);
}

#[test]
fn a_word_is_found_from_its_start_only() {
    let book = book();
    assert_eq!(book.find("rowth", MOST), []);
    assert_eq!(quotes(&book.find("gro", MOST)).len(), 3);
    // A word no word of the book begins finds nothing, whatever else is typed.
    assert_eq!(book.find("growth zebra", MOST), []);
}

#[test]
fn a_query_needs_two_letters_and_the_places_stop_at_the_limit() {
    let book = book();
    assert_eq!(book.find("a", MOST), []);
    assert_eq!(book.find("  ", MOST), []);
    assert_eq!(book.find("--", MOST), []);
    assert_eq!(book.find("th", 2).len(), 2);
    assert_eq!(book.find("th", MOST).len(), 3);
}

#[test]
fn a_reflowable_books_blocks_are_found_in() {
    let book = TextIndex::of_blocks(
        [
            (0, "Replication means keeping a copy."),
            (2, "Leaders and followers keep copies too."),
        ],
        Vec::new(),
        Source::default(),
    );
    let hits = book.find("cop", MOST);
    let found: Vec<(usize, &str, &str)> = hits
        .iter()
        .map(|h| (h.part, h.quote.as_str(), h.prefix.as_str()))
        .collect();
    assert_eq!(
        found,
        [
            (0, "cop", "Replication means keeping a "),
            (2, "cop", "Leaders and followers keep "),
        ]
    );
    assert_eq!((hits[0].start, hits[0].end), (28, 31));
    assert_eq!(hits[0].y, None);
}

#[test]
fn the_text_around_a_place_is_thirty_two_characters_each_side() {
    let text = "a".repeat(40) + " needle " + &"b".repeat(40);
    let book = TextIndex::of_blocks([(0, text.as_str())], Vec::new(), Source::default());
    let hits = book.find("needle", MOST);
    assert_eq!(hits[0].prefix, format!("{} ", "a".repeat(31)));
    assert_eq!(hits[0].suffix, format!(" {}", "b".repeat(31)));
}

#[test]
fn running_heads_and_page_numbers_are_not_found() {
    // Four pages, each with the book's title at its top and its number at its foot.
    let pages: Vec<PageText> = (1..=4)
        .map(|n| {
            let number = (10 + n).to_string();
            let mut page = page(&["A Book of Growth", "The text on growth.", &number]);
            page.lines[0].top = 0.03;
            page.lines[2].top = 0.95;
            page
        })
        .collect();
    let book = TextIndex::of_pages(&pages, Vec::new(), Source::default());
    assert_eq!(book.find("growth", MOST).len(), 4);
    assert_eq!(book.find("book", MOST), []);
    assert_eq!(book.find("12", MOST), []);
    // A line near the top that does not repeat stays.
    let mut once = pages.clone();
    once[0].text = once[0]
        .text
        .replacen("A Book of Growth", "A Book of Plants", 1);
    let book = TextIndex::of_pages(&once, Vec::new(), Source::default());
    assert_eq!(book.find("plants", MOST).len(), 1);
}

#[test]
fn a_place_falls_in_the_line_of_the_contents_its_line_is_in() {
    let contents = vec![
        entry("One", 0, Some(0.5)),
        entry("Two", 1, None),
        entry("Two, later", 1, Some(0.4)),
    ];
    let book = TextIndex::of_pages(
        &[
            page(&["xylophone"]),
            page(&["one", "two", "three", "four", "five", "six", "seven"]),
        ],
        contents,
        Source::default(),
    );
    // "seven" is 0.4 down the second page and "five" 0.3: after and before the third line.
    assert_eq!(book.find("seven", MOST)[0].chapter, Some(2));
    assert_eq!(book.find("five", MOST)[0].chapter, Some(1));
    // The first page's line is above where the first line of the contents leads.
    assert_eq!(book.find("xylophone", MOST)[0].chapter, None);
}

#[test]
fn the_search_gives_each_line_of_the_contents_that_has_every_word() {
    let contents = vec![entry("Rates", 0, Some(0.12)), entry("Café", 1, None)];
    let book = TextIndex::of_pages(&pages(), contents, Source::default());
    let passages = book.search("growth");
    let titles: Vec<(Option<usize>, &str)> = passages
        .iter()
        .map(|p| (p.chapter, p.title.as_str()))
        .collect();
    assert_eq!(titles, [(Some(0), "Rates"), (Some(1), "Café")]);
    assert_eq!(passages[0].part, 0);
    assert!((passages[0].y.unwrap() - 0.13).abs() < 1e-6);
    assert!(passages[1]
        .snippet
        .starts_with("A ﬁnal word on Café Growth."));
    // The text before the first line of the contents is a passage of its own.
    let before = book.search("calculus");
    assert_eq!(before.len(), 1);
    assert_eq!((before[0].chapter, before[0].title.as_str()), (None, ""));
    // Every word must be in it, and each says whether it was there whole.
    assert_eq!(book.search("growth nothing").len(), 1);
    assert_eq!(book.search("growing word"), []);
    assert_eq!(book.search("gro word")[0].whole, [false, true]);
    assert_eq!(book.search("growth")[0].whole, [true]);
}

#[test]
fn a_snippet_starts_fifty_characters_before_and_is_cut_at_160() {
    let text = format!("{} needle {}", "a".repeat(100), "b".repeat(300));
    let book = TextIndex::of_blocks(
        [(0, text.as_str())],
        vec![entry("All", 0, None)],
        Source::default(),
    );
    let snippet = &book.search("needle")[0].snippet;
    assert!(snippet.starts_with("…aaa"));
    assert!(snippet.ends_with("bbb…"));
    assert_eq!(snippet.chars().count(), 160 + 2);
}

#[test]
fn an_index_kept_as_bytes_reads_back_the_same() {
    let contents = vec![entry("Rates", 0, Some(0.12)), entry("Café", 1, None)];
    let source = Source {
        len: 1234,
        modified: 5678,
    };
    let index = TextIndex::of_pages(&pages(), contents.clone(), source);
    let bytes = index.encode();
    let read = TextIndex::decode(&bytes).unwrap();
    assert_eq!(read.source, source);
    assert_eq!(read.contents(), &contents[..]);
    assert_eq!(read.size(), index.size());
    for query in ["growing", "cafe growth", "th", "rat of gro"] {
        assert_eq!(read.find(query, MOST), index.find(query, MOST), "{query}");
        assert_eq!(read.search(query), index.search(query), "{query}");
    }
}

#[test]
fn bytes_that_are_not_an_index_are_refused_without_a_panic() {
    let bytes = book().encode();
    assert!(TextIndex::decode(b"").is_err());
    assert!(TextIndex::decode(b"something else entirely").is_err());
    // Cut anywhere, it is refused.
    for end in 0..bytes.len() {
        assert!(TextIndex::decode(&bytes[..end]).is_err(), "cut at {end}");
    }
    // Any byte changed, it is refused or read as some index, never a panic.
    for at in 0..bytes.len() {
        let mut broken = bytes.clone();
        broken[at] ^= 0xa5;
        let _ = TextIndex::decode(&broken);
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(TextIndex::decode(&longer).is_err());
}

/// How long the index of each sample book takes to make, keep, read back and query, printed:
/// `cargo test -p incipit-book -- --ignored --nocapture sample_books`.
#[test]
#[ignore = "requires pdfium in lib/ and the sample books from tools/setup"]
fn the_sample_books_indexes_are_made_kept_and_queried() {
    use std::path::PathBuf;
    use std::time::Instant;

    let root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    let started = Instant::now();
    let book = crate::Book::open(&root.join("samples/book.epub")).unwrap();
    let opened = started.elapsed();
    let blocks: Vec<(usize, &str)> = book
        .blocks
        .iter()
        .enumerate()
        .filter_map(|(i, block)| match block {
            crate::Block::Heading { text, .. } | crate::Block::Paragraph(text) => {
                Some((i, text.text.as_str()))
            }
            crate::Block::Image { .. } => None,
        })
        .collect();
    let characters: usize = blocks.iter().map(|(_, t)| t.chars().count()).sum();
    let started = Instant::now();
    let index = TextIndex::of_blocks(blocks, Vec::new(), Source::default());
    let built = started.elapsed();
    report("book.epub", &index, characters, opened, built);

    let started = Instant::now();
    let pdf = crate::Pdf::open(&root.join("lib/lib"), &root.join("samples/paper.pdf")).unwrap();
    let pages: Vec<PageText> = (0..pdf.pages())
        .map(|i| pdf.page_text(i).unwrap())
        .collect();
    let read = started.elapsed();
    let characters: usize = pages.iter().map(|p| p.text.chars().count()).sum();
    let started = Instant::now();
    let index = TextIndex::of_pages(&pages, pdf.contents(), Source::default());
    let built = started.elapsed();
    report("paper.pdf", &index, characters, read, built);
}

fn report(
    name: &str,
    index: &TextIndex,
    characters: usize,
    read: std::time::Duration,
    built: std::time::Duration,
) {
    use std::time::Instant;
    let started = Instant::now();
    let bytes = index.encode();
    let encoded = started.elapsed();
    let started = Instant::now();
    let back = TextIndex::decode(&bytes).unwrap();
    let decoded = started.elapsed();
    let (distinct, words) = index.size();
    println!(
        "{name}: {characters} characters, {words} words, {distinct} distinct; read in {read:?}, \
         indexed in {built:?}; {} bytes kept, written in {encoded:?}, read back in {decoded:?}",
        bytes.len()
    );
    for query in [
        "the",
        "light",
        "space time",
        "relativity",
        "attention",
        "zzz",
    ] {
        let started = Instant::now();
        let found = back.find(query, MOST).len();
        let find = started.elapsed();
        let started = Instant::now();
        let passages = back.search(query).len();
        let search = started.elapsed();
        println!("  {query:?}: {found} places in {find:?}; {passages} passages in {search:?}");
    }
    assert!(words > 1000);
}
