//! What Markdown is read into: blocks, marks, clozes and math.

use super::draw::{backward_pieces, breaks_badly, pieces, Piece};
use super::*;

fn parse_plain(source: &str) -> Vec<Block> {
    parse(source, None)
}

/// The one paragraph of `source`, read as a card's text.
fn card(source: &str, cloze: Cloze) -> Inline {
    match parse(source, Some(cloze)).into_iter().next() {
        Some(Block::Paragraph(inline)) => inline,
        _ => panic!("a paragraph"),
    }
}

/// The stretches of `inline` that are `part` of a cloze.
fn parts(inline: &Inline, part: ClozePart) -> Vec<&str> {
    inline
        .runs
        .iter()
        .filter(|(_, m)| m.cloze == Some(part))
        .map(|(r, _)| &inline.text[r.clone()])
        .collect()
}

#[test]
fn a_cloze_is_two_braces_around_words_on_one_line() {
    let words = |text: &'static str| -> Vec<&'static str> {
        cloze_spans(text).into_iter().map(|r| &text[r]).collect()
    };
    assert_eq!(
        words("The {{mitochondria}} of {{a cell}}."),
        ["mitochondria", "a cell"]
    );
    assert!(words("{{}} {one} {{two} {{ three\nfour }}").is_empty());
    // The earliest opening braces count, and a brace may be among the words.
    assert_eq!(words("{{{a}}}"), ["{a"]);
    assert_eq!(words("{{a}b}} {{c}}"), ["c"]);
    assert_eq!(words("{{ spaced out }}"), [" spaced out "]);
}

#[test]
fn a_shown_cloze_loses_its_braces_and_stands_out() {
    let shown = card(
        "The {{mitochondria}} powers **the {{cell}}**.",
        Cloze::Shown,
    );
    assert_eq!(shown.text, "The mitochondria powers the cell.");
    assert_eq!(parts(&shown, ClozePart::Words), ["mitochondria", "cell"]);
    // The emphasis around a cloze stays on it.
    assert!(shown
        .runs
        .iter()
        .any(|(r, m)| m.bold && m.cloze.is_some() && &shown.text[r.clone()] == "cell"));
}

#[test]
fn a_hidden_cloze_is_a_blank_that_ends_in_its_mark() {
    let hidden = card("The {{mitochondria}} powers the cell.", Cloze::Hidden);
    // The mark stands in for the last three letters: the blank is about as wide.
    assert_eq!(hidden.text, "The mitochond[…] powers the cell.");
    assert_eq!(parts(&hidden, ClozePart::Blank), ["mitochond"]);
    assert_eq!(parts(&hidden, ClozePart::Mark), ["[…]"]);
    // Every other word is drawn as it is.
    let plain: Vec<&(Range<usize>, Marks)> = hidden
        .runs
        .iter()
        .filter(|(_, m)| m.cloze.is_none())
        .collect();
    assert!(plain.is_empty());
}

#[test]
fn a_short_cloze_is_only_its_mark() {
    for source in ["2 + 2 = {{4}}", "2 + 2 = {{four}}"] {
        let hidden = card(source, Cloze::Hidden);
        assert!(parts(&hidden, ClozePart::Blank)
            .iter()
            .all(|b| b.len() <= 1));
        assert!(!hidden.text.contains('4') || source.contains("four"));
    }
    assert_eq!(card("2 + 2 = {{4}}", Cloze::Hidden).text, "2 + 2 = […]");
    assert_eq!(card("It is {{DNA}}.", Cloze::Hidden).text, "It is […].");
    // Characters are whole ones: an accent written apart stays with its letter.
    assert_eq!(kept_of_blank("ae\u{301}iou"), "ae\u{301}");
}

#[test]
fn a_cloze_is_found_in_text_the_parser_hands_over_in_pieces() {
    let shown = card("An {{array [0] of_ints}} here", Cloze::Shown);
    assert_eq!(parts(&shown, ClozePart::Words), ["array [0] of_ints"]);
    assert_eq!(shown.text, "An array [0] of_ints here");
}

#[test]
fn braces_in_code_math_and_a_links_words_are_not_a_cloze() {
    for source in [
        "Write `{{name}}` in the template.",
        "So $x^{{2}}$ grows.",
        "See [{{this}}](https://example.com).",
    ] {
        let read = card(source, Cloze::Hidden);
        assert!(read.runs.iter().all(|(_, m)| m.cloze.is_none()), "{source}");
        assert!(!read.text.contains(BLANK_MARK), "{source}");
    }
    assert_eq!(
        card("So $x^{{2}}$ grows.", Cloze::Shown).text,
        "So $x^{{2}}$ grows."
    );
    // Emphasis inside a link is text of its own, as in the earlier app.
    let inside = card("See [*{{this}}*](https://example.com).", Cloze::Shown);
    assert_eq!(parts(&inside, ClozePart::Words), ["this"]);
}

#[test]
fn a_cloze_does_not_reach_across_emphasis_or_a_line_break() {
    for source in ["{{the *cell* wall}}", "{{the cell\nwall}}"] {
        let read = card(source, Cloze::Hidden);
        assert!(read.runs.iter().all(|(_, m)| m.cloze.is_none()), "{source}");
    }
}

#[test]
fn text_that_is_not_a_cards_keeps_its_braces() {
    let Some(Block::Paragraph(p)) = parse_plain("A {{template}} here.").into_iter().next() else {
        panic!("a paragraph")
    };
    assert_eq!(p.text, "A {{template}} here.");
    assert!(p.runs.is_empty());
}

/// The formulas of the blocks of `source`, by block: (TeX, written `$$…$$`), and a block of
/// math as (TeX, true) alone.
fn formulas(source: &str) -> Vec<Vec<(String, bool)>> {
    parse_plain(source)
        .into_iter()
        .map(|block| match block {
            Block::Paragraph(p) => p.math.into_iter().map(|m| (m.tex, m.display)).collect(),
            Block::Math(tex, _) => vec![(tex, true)],
            _ => Vec::new(),
        })
        .collect()
}

fn math(tex: &str, display: bool) -> (String, bool) {
    (tex.to_string(), display)
}

#[test]
fn math_in_a_line_is_found_and_keeps_its_written_form() {
    let Some(Block::Paragraph(p)) = parse_plain(r"The area is $\pi r^2$ here.").pop() else {
        panic!("a paragraph")
    };
    assert_eq!(p.text, r"The area is $\pi r^2$ here.");
    assert_eq!(p.math.len(), 1);
    assert_eq!(&p.text[p.math[0].range.clone()], r"$\pi r^2$");
    assert_eq!(p.math[0].tex, r"\pi r^2");
    assert!(!p.math[0].display);
}

#[test]
fn display_math_on_lines_of_its_own_is_a_block_and_in_a_sentence_is_not() {
    assert_eq!(
        formulas("Before\n\n$$\nE = mc^2\n$$\n\nAfter"),
        [vec![], vec![math("\nE = mc^2\n", true)], vec![]]
    );
    // On its own line between lines of text, it parts them.
    let blocks = parse_plain("So:\n$$x^2$$\nand more.");
    assert!(matches!(&blocks[0], Block::Paragraph(p) if p.text == "So:"));
    assert!(matches!(&blocks[1], Block::Math(tex, false) if tex == "x^2"));
    assert!(matches!(&blocks[2], Block::Paragraph(p) if p.text == "and more."));
    // Inside a sentence, it stays in the line.
    assert_eq!(formulas("We have $$x^2$$ here."), [vec![math("x^2", true)]]);
    // A fence of math is a block of math.
    let blocks = parse_plain("```math\n\\frac{a}{b}\n```");
    assert!(matches!(&blocks[0], Block::Math(tex, true) if tex == "\\frac{a}{b}"));
}

#[test]
fn a_lone_dollar_code_and_escaped_dollars_stay_text() {
    for source in [
        "It costs $5.",
        "It costs $5 and $10 is more.",
        r"Write \$x\$ for a dollar.",
        "Write `$x$` in code.",
        "```\n$x$\n```",
        "A $ x $ with spaces.",
    ] {
        assert!(formulas(source).iter().all(Vec::is_empty), "{source}");
    }
    assert_eq!(
        formulas("It costs $5. The formula $x$ holds."),
        [vec![math("x", false)]]
    );
}

#[test]
fn math_written_with_backslashes_is_read_as_dollars_outside_code() {
    assert_eq!(
        normalize_math(r"So \( x^2 \) and \[y\]."),
        r"So $x^2$ and $$y$$."
    );
    assert_eq!(
        normalize_math(r"Code `\(x\)` stays."),
        r"Code `\(x\)` stays."
    );
    assert_eq!(normalize_math(r"No math \(here."), r"No math \(here.");
    assert_eq!(
        formulas("Lines:\n\\[\na+b\n\\]\nend"),
        [vec![], vec![math("\na+b\n", true)], vec![]]
    );
}

#[test]
fn a_cards_cloze_and_its_math_stay_apart() {
    let shown = card(r"The {{area}} is $\pi r^{2}$.", Cloze::Shown);
    assert_eq!(parts(&shown, ClozePart::Words), ["area"]);
    assert_eq!(shown.math.len(), 1);
    assert_eq!(&shown.text[shown.math[0].range.clone()], r"$\pi r^{2}$");
    // Braces inside math are never a cloze, and the braces around math are not either.
    for source in [r"So $\frac{{a}}{b}$ holds.", "So {{$x$}} holds."] {
        let read = card(source, Cloze::Hidden);
        assert!(read.runs.iter().all(|(_, m)| m.cloze.is_none()), "{source}");
        assert_eq!(read.math.len(), 1, "{source}");
    }
}

#[test]
fn a_line_with_math_is_cut_into_words_and_line_breaks() {
    let text = " is a bc  d\nnext";
    let words: Vec<_> = pieces(text, 0..text.len())
        .into_iter()
        .map(|p| match p {
            Piece::Words(r) => &text[r],
            Piece::Turned { .. } => "turned",
            Piece::Break => "|",
        })
        .collect();
    assert_eq!(words, [" ", "is ", "a ", "bc  ", "d", "|", "next"]);
}

#[test]
fn blocks_and_marks_come_out_of_markdown() {
    let b = parse_plain(
        "# Title\n\nSome **bold** and `code`, a [link](https://x.y).\n\n- \
            one\n- two\n  1. deep\n\n```\nfn a() {}\n```\n\n> quoted\n\n| a | b \
            |\n|---|---|\n| 1 | 2 |\n",
    );
    assert!(matches!(b[0], Block::Heading(1, _)));
    let Block::Paragraph(p) = &b[1] else {
        panic!("a paragraph")
    };
    assert_eq!(p.text, "Some bold and code, a link.");
    assert!(p
        .runs
        .iter()
        .any(|(r, m)| m.bold && &p.text[r.clone()] == "bold"));
    assert!(p
        .runs
        .iter()
        .any(|(r, m)| m.code && &p.text[r.clone()] == "code"));
    assert_eq!(p.links[0].1, "https://x.y");
    let Block::List { items, start: None } = &b[2] else {
        panic!("a list")
    };
    assert_eq!(items.len(), 2);
    assert!(items[1]
        .iter()
        .any(|b| matches!(b, Block::List { start: Some(1), .. })));
    assert!(matches!(&b[3], Block::Code(c) if c == "fn a() {}"));
    assert!(matches!(b[4], Block::Quote(_)));
    assert!(matches!(&b[5], Block::Table(rows) if rows.len() == 2 && rows[1][1].text == "2"));
}

#[test]
fn text_gpui_would_cut_inside_a_word_is_known() {
    assert!(!breaks_badly(
        "Plain words, café and naïve, «quoted» — and Привет."
    ));
    assert!(breaks_badly("Let M′ be the mid-point"));
    assert!(breaks_badly("so E₀ is the rest energy"));
    assert!(breaks_badly("τὸ φῶς ἐκπέμπεται"));
    assert!(breaks_badly("नमस्ते दुनिया"));
    assert!(breaks_badly("a vector a\u{20D7} here"));
}

#[test]
fn text_set_right_to_left_keeps_gpuis_lines() {
    assert!(!breaks_badly("תורת היחסות"));
    assert!(!breaks_badly("النظرية النسبية and M′"));
}

/// The words of `text` as a paragraph set from the right takes them.
fn set_backward(text: &str) -> Vec<String> {
    backward_pieces(text, pieces(text, 0..text.len()))
        .into_iter()
        .map(|piece| match piece {
            Piece::Words(range) => text[range].to_owned(),
            Piece::Turned { words, marks } => format!("{}{}", &text[marks], &text[words]),
            Piece::Break => "|".to_owned(),
        })
        .collect()
}

#[test]
fn a_paragraph_set_from_the_right_keeps_its_words_and_their_order() {
    assert_eq!(set_backward("תורת היחסות של"), ["תורת", "היחסות", "של"]);
    assert_eq!(
        set_backward("כתב Albert Einstein בשנת 1905."),
        ["כתב", "Albert Einstein", "בשנת", ".1905"]
    );
    assert_eq!(
        set_backward("לפי Albert Einstein, שני (Lorentz) [1]."),
        ["לפי", ",Albert Einstein", "שני", ".(Lorentz) [1]"]
    );
    assert_eq!(set_backward("שורה\nשנייה"), ["שורה", "|", "שנייה"]);
}
