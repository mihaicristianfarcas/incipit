//! The PDF module's tests. Those that open a PDF read the sample paper `tools/setup` downloads,
//! with the pdfium it puts in `lib/`: `cargo test -p incipit-book -- --ignored`.

use super::*;
use std::path::PathBuf;

#[test]
fn lines_end_in_a_line_break_alone_in_the_text_and_the_glyphs_alike() {
    let text = "one\r\ntwo\rthree\n".to_string();
    assert_eq!(plain_lines(text.clone()), "one\ntwo\nthree\n");
    let glyphs: Vec<Glyph> = text
        .chars()
        .map(|c| Glyph {
            c,
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        })
        .collect();
    let plain: String = plain_glyphs(glyphs).iter().map(|g| g.c).collect();
    assert_eq!(plain, plain_lines(text));
}

fn sample() -> Pdf {
    let root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    Pdf::open(&root.join("lib/lib"), &root.join("samples/paper.pdf")).unwrap()
}

/// Glyphs of lines of text, each line's characters 5 points wide and 10 high, the line
/// starting `top` points down, and each line ended by a line end pdfium added.
fn glyphs(lines: &[(&str, f32)]) -> Vec<Glyph> {
    lines
        .iter()
        .flat_map(|&(line, top)| {
            line.chars()
                .enumerate()
                .map(move |(i, c)| Glyph {
                    c,
                    x: 50.0 + 5.0 * i as f32,
                    y: top,
                    w: 5.0,
                    h: 10.0,
                })
                .chain([Glyph {
                    c: '\n',
                    x: 0.0,
                    y: 0.0,
                    w: 0.0,
                    h: 0.0,
                }])
        })
        .collect()
}

fn sheet(lines: &[(&str, f32)]) -> Page {
    Page {
        width: 600.0,
        height: 800.0,
        pixels: Vec::new(),
        pixel_width: 0,
        pixel_height: 0,
        glyphs: glyphs(lines),
        pictures: Vec::new(),
    }
}

#[test]
fn the_keyboard_selects_the_lines_from_the_reading_line_to_the_paragraphs_end() {
    // Two lines of a paragraph, then a gap of more than 1.6 lines, then another.
    let page = sheet(&[("one", 100.0), ("two", 112.0), ("three", 140.0)]);
    let text: String = page.text().into_iter().collect();
    let (first, last) = page.lines_at(105.0).unwrap();
    assert_eq!(&text[first..=last], "one\ntwo");
    // From the second line, which the reading line reaches.
    let (first, last) = page.lines_at(111.0).unwrap();
    assert_eq!(&text[first..=last], "two");
    // Under every line, nothing.
    assert_eq!(page.lines_at(200.0), None);
}

#[test]
fn the_keyboard_selects_six_lines_at_most() {
    let lines: Vec<(String, f32)> = (0..9)
        .map(|n| (format!("line{n}"), 100.0 + 12.0 * n as f32))
        .collect();
    let lines: Vec<(&str, f32)> = lines.iter().map(|(s, y)| (s.as_str(), *y)).collect();
    let page = sheet(&lines);
    let text: String = page.text().into_iter().collect();
    let (first, last) = page.lines_at(100.0).unwrap();
    assert_eq!(
        &text[first..=last],
        "line0\nline1\nline2\nline3\nline4\nline5"
    );
}

#[test]
fn text_set_up_the_margin_is_not_a_line_of_the_passage() {
    // A stamp up the page's left margin, its characters one above the other and last in
    // the page's text, as on the sample paper's first page, beside two lines.
    let mut page = sheet(&[("one", 100.0), ("two", 112.0)]);
    page.glyphs
        .extend("stamp".chars().enumerate().map(|(i, c)| Glyph {
            c,
            x: 10.0,
            y: 150.0 - 8.0 * i as f32,
            w: 10.0,
            h: 8.0,
        }));
    let text: String = page.text().into_iter().collect();
    let (first, last) = page.lines_at(105.0).unwrap();
    assert_eq!(&text[first..=last], "one\ntwo");
}

#[test]
fn runs_side_by_side_are_one_line_of_the_passage() {
    // Two columns' lines at nearly the same height count once.
    let page = sheet(&[("left", 100.0), ("right", 101.0), ("next", 112.0)]);
    let text: String = page.text().into_iter().collect();
    let (first, last) = page.lines_at(100.0).unwrap();
    assert_eq!(&text[first..=last], "left\nright\nnext");
}

/// A page 792 points high of lines 14 points apart in 10 point type, the first with its
/// baseline 100 points below the page's top; each ends with a line end pdfium added.
fn page(lines: &[&str]) -> Vec<(char, Option<(f32, f32)>)> {
    lines
        .iter()
        .enumerate()
        .flat_map(|(n, line)| {
            let baseline = 692.0 - 14.0 * n as f32;
            line.chars()
                .map(move |c| (c, Some((baseline, 10.0))))
                .chain([('\n', None)])
        })
        .collect()
}

#[test]
fn a_place_on_a_page_is_a_share_of_its_height_from_its_top() {
    let close = |y: Option<f32>, to: f32| y.is_some_and(|y| (y - to).abs() < 1e-4);
    assert!(close(down(720.0, 792.0), 0.0909));
    assert!(close(down(436.994, 792.0), 0.4482));
    // A place above or below the page is at its edge.
    assert_eq!(down(900.0, 792.0), Some(0.0));
    assert_eq!(down(-10.0, 792.0), Some(1.0));
    assert_eq!(down(0.0, 0.0), None);
}

#[test]
fn characters_on_one_baseline_are_a_line_whose_top_is_above_its_largest_type() {
    let tops = line_tops(
        &[
            // A heading in 12 point type and the line end after it.
            Some((710.0, 12.0)),
            Some((710.0, 12.0)),
            None,
            // A line in 10 point type with a larger letter in it, lowered a point.
            Some((685.0, 10.0)),
            Some((684.0, 14.0)),
            Some((685.0, 10.0)),
            // A mark raised by more than half its own size is a line of its own, and what
            // follows it is on the line before it again.
            Some((689.0, 7.0)),
            None,
            Some((685.0, 10.0)),
            // The other column's line on the heading's baseline is the heading's line.
            Some((710.5, 10.0)),
        ],
        792.0,
    );
    // (792 - 710 - 0.85 × 12) / 792, and so on, to three decimals.
    let heading = Some(0.091);
    let body = Some(0.12);
    let mark = Some(0.123);
    assert_eq!(tops[..3], [heading, heading, heading]);
    assert_eq!(tops[3..6], [body, body, body]);
    assert_eq!(tops[6..], [mark, mark, body, heading]);
    // What comes before the first line goes with it; a page without a line has no tops.
    assert_eq!(line_tops(&[None, Some((710.0, 12.0))], 792.0), [heading; 2]);
    assert_eq!(line_tops(&[None, None], 792.0), [None, None]);
    assert!(line_tops(&[], 792.0).is_empty());
}

#[test]
fn a_pages_text_is_cut_at_the_lines_at_its_top_and_bottom() {
    // Their tops are 0.116, 0.133, 0.151, 0.169 and 0.186 down the page.
    let text = page(&["before", "Heading", "its text", "Next heading", "after"]);
    let cut = |top, bottom| text_between(&text, 792.0, top, bottom);
    assert_eq!(
        cut(Some(0.133), None),
        "Heading\nits text\nNext heading\nafter\n"
    );
    assert_eq!(cut(Some(0.134), None), "its text\nNext heading\nafter\n");
    // The line at the bottom is not taken.
    assert_eq!(cut(None, Some(0.169)), "before\nHeading\nits text\n");
    assert_eq!(cut(Some(0.13), Some(0.165)), "Heading\nits text\n");
    // A bottom at the page's top or above leaves nothing, and so do a top below a bottom
    // and a page without a line.
    assert_eq!(cut(None, Some(-0.005)), "");
    assert_eq!(cut(Some(0.169), Some(0.133)), "");
    assert_eq!(text_between(&[(' ', None)], 792.0, Some(0.0), None), "");
    assert_eq!(cut(None, None).len(), 43);
}

#[test]
fn a_pages_lines_are_where_pdfium_ends_them_each_with_the_top_of_its_first_character() {
    let mut chars = page(&["Heading", "its text"]);
    // pdfium ends a line in \r\n; and a line of only what it added is no line.
    chars.insert(7, ('\r', None));
    chars.insert(0, ('\n', None));
    let text = lines_of(chars, 792.0);
    assert_eq!(text.text, "\nHeading\nits text\n");
    let line = |start, end, top| Line { start, end, top };
    assert_eq!(text.lines, [line(1, 8, 0.116), line(9, 17, 0.133)]);
    // A page whose last line has no line end.
    let mut chars = page(&["one", "two"]);
    chars.pop();
    assert_eq!(
        lines_of(chars, 792.0).lines,
        [line(0, 3, 0.116), line(4, 7, 0.133)]
    );
    assert_eq!(lines_of(Vec::new(), 792.0), PageText::default());
}

#[test]
fn a_title_is_found_where_it_is_a_heading_whatever_its_case_accents_and_line_breaks() {
    let text = lines_of(
        page(&[
            "the end of the relativity",
            "chapter, on relativité.",
            "CHAPTER 2",
            "Relativité",
            "3.1 Its text",
        ]),
        792.0,
    );
    // The tops are 0.116, 0.133, 0.151, 0.169 and 0.186 down the page.
    assert_eq!(title_top(&text, "Chapter 2: Relativite"), Some(0.151));
    assert_eq!(title_top(&text, "relativité"), Some(0.169));
    assert_eq!(title_top(&text, "Its text"), Some(0.186));
    // In the text, its words are not its heading: before or after them on their line is
    // more than a number.
    assert_eq!(title_top(&text, "relativity chapter"), None);
    assert_eq!(title_top(&text, "the end of the"), None);
    // Too short to look for, or not there.
    assert_eq!(title_top(&text, "Its"), None);
    assert_eq!(title_top(&text, "Chapter 3"), None);
    assert_eq!(title_top(&PageText::default(), "Chapter 2"), None);
    assert!(numbered("") && numbered("31") && numbered("chapteriv") && numbered("part2"));
    assert!(!numbered("onrelativity") && !numbered("chapter2a"));
    // The page is cut a little above the title's line, and never below its top.
    assert!((above_title(0.151) - 0.136).abs() < 1e-6);
    assert!((above_title(0.004) + 0.005).abs() < 1e-6);
}

#[test]
#[ignore = "requires pdfium in lib/ and the sample paper from tools/setup"]
fn a_drawn_page_turns_dark_but_for_its_pictures() {
    let pdf = sample();
    let mut page = pdf.page(0, 1280).unwrap();
    let mut pixels = std::mem::take(&mut page.pixels);
    let started = std::time::Instant::now();
    crate::Dark::new().recolour(&mut pixels, page.pixel_width as usize, &[]);
    eprintln!(
        "page 1 at {} by {} pixels turned dark in {:?}",
        page.pixel_width,
        page.pixel_height,
        started.elapsed()
    );
    // The paper's corner is the dark ground.
    assert_eq!(&pixels[..3], &crate::dark::GROUND);
    // Every page's pictures sit on it.
    for index in 0..pdf.pages() {
        let page = pdf.page(index, 200).unwrap();
        for &(x, y, w, h) in &page.pictures {
            assert!(x >= -1.0 && y >= -1.0 && w > 0.0 && h > 0.0);
            assert!(x + w <= page.width + 1.0 && y + h <= page.height + 1.0);
        }
    }
}

#[test]
#[ignore = "requires pdfium in lib/ and the sample paper from tools/setup"]
fn a_run_is_cut_above_the_next_title_when_only_its_page_is_known() {
    let pdf = sample();
    // "3 Model Architecture" is on the paper's second page, 0.81 down.
    let cut = |before: &str| Cut {
        before: Some(before.into()),
        ..Cut::default()
    };
    let text = &pdf.texts(1..2, cut("Model Architecture")).unwrap()[0];
    assert!(text.starts_with("1 Introduction"));
    assert!(text.ends_with("such as [17, 18] and [9].\n"), "{text}");
    // A title not on the page takes it whole; a bottom that is known comes first.
    let whole = pdf.text(1).unwrap();
    assert_eq!(pdf.texts(1..2, cut("Not a title here")).unwrap()[0], whole);
    let bottom = Cut {
        bottom: Some(0.44),
        ..Cut::default()
    };
    let known = Cut {
        bottom: Some(0.44),
        ..cut("Model Architecture")
    };
    assert_eq!(
        pdf.texts(1..2, known).unwrap(),
        pdf.texts(1..2, bottom).unwrap()
    );
}

#[test]
#[ignore = "requires pdfium in lib/ and the sample paper from tools/setup"]
fn a_pages_lines_are_its_text_as_drawn() {
    let pdf = sample();
    let text = pdf.page_text(1).unwrap();
    assert_eq!(text.text, pdf.text(1).unwrap());
    let chars: Vec<char> = text.text.chars().collect();
    let background = text
        .lines
        .iter()
        .find(|l| chars[l.start..l.end].iter().collect::<String>() == "2 Background")
        .unwrap();
    // Its top is 0.457 down the page (see the cut below).
    assert!((background.top - 0.457).abs() < 1e-3);
    assert!(text.lines.windows(2).all(|w| w[0].end < w[1].start));
}

#[test]
#[ignore = "requires pdfium in lib/ and the sample paper from tools/setup"]
fn the_contents_say_how_far_down_its_page_a_section_starts() {
    let contents = sample().contents();
    let background = contents.iter().find(|e| e.title == "Background").unwrap();
    // Its line of the outline leads to 436.994 points above the bottom of a page of 792.
    assert_eq!(background.page, 1);
    assert!((background.y.unwrap() - 0.4482).abs() < 1e-4);
    assert!(contents
        .iter()
        .all(|e| e.y.is_some_and(|y| y > 0.0 && y < 1.0)));
}

#[test]
#[ignore = "requires pdfium in lib/ and the sample paper from tools/setup"]
fn a_run_of_pages_is_cut_on_its_first_and_last_page() {
    let pdf = sample();
    let whole = pdf.texts(1..3, Cut::default()).unwrap();
    assert_eq!(whole, [pdf.text(1).unwrap(), pdf.text(2).unwrap()]);
    // On the paper's second page "2 Background" has its top 0.457 down the page and the
    // line before it 0.42; "3 Model Architecture" 0.81 and the line before it 0.773.
    let section = Cut {
        top: Some(0.44),
        bottom: Some(0.8),
        before: None,
    };
    let cut = &pdf.texts(1..2, section.clone()).unwrap()[0];
    assert!(cut.starts_with("2 Background\nThe goal of reducing"));
    assert!(cut.ends_with("such as [17, 18] and [9].\n"));
    assert!(!whole.iter().any(|page| page.contains('\r')));
    assert!(whole[0].contains(cut.as_str()));
    // Over two pages the first is cut at its top only, the last at its bottom only.
    let both = pdf.texts(1..3, section).unwrap();
    assert!(both[0].starts_with("2 Background"));
    assert!(both[0].ends_with("\n2"));
    assert!(both[1].starts_with("Figure 1"));
    assert!(both[1].ends_with("We also modify the self-attention\n"));
    // A bottom above the page takes nothing from the last page.
    let none = Cut {
        bottom: Some(-0.005),
        ..Cut::default()
    };
    assert_eq!(
        pdf.texts(1..3, none).unwrap(),
        [whole[0].clone(), String::new()]
    );
}
