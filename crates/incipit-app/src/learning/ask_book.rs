//! Asking a whole book: a question answered from the book's own words, each claim followed by
//! the number of the excerpt it comes from, which takes the reader there. The excerpts are the
//! chapter the learner is reading and the passages of the book the question is most about
//! (`incipit_book::TextIndex::excerpts`), each numbered and labelled with where it is; they and
//! the conversation so far are material and turns, apart from the instructions, as for the
//! tutor. The answer is told to keep to the excerpts and to say so when they do not hold the
//! answer, so that it says what the book says rather than what a model knows.
//!
//! A citation is `[n]` in the answer. Before it is shown, each one that names an excerpt becomes
//! a link to the excerpt's place (`place:<part>`), which the reader follows (`linked`); a number
//! no excerpt has is left as written. An answer is kept in the vault with a line under it that
//! says where each number leads (`with_sources`), which is read back when the conversation is
//! taken up again (`sources_of`).

use incipit_llm::{Effort, Request};
use incipit_vault::fields::{Speaker, Turn};

use super::context::{cut, BookContext};
use super::learner;

/// The most characters of the chapter being read that go with a question, and of the passages
/// found for it: together about a long chapter, within what one request of the quick model
/// takes.
pub const CHAPTER: usize = 12_000;
pub const FOUND: usize = 18_000;
/// The scheme of a link to a place in the open book, in an answer's Markdown.
pub const PLACE: &str = "place:";

/// An excerpt as it goes to the model: where it is (its first and last part), what to call it
/// (`page 4`, `pages 6–7`, `the chapter you are reading`) and its text.
#[derive(Clone, Debug, PartialEq)]
pub struct Cited {
    pub part: usize,
    pub last: usize,
    pub label: String,
    pub text: String,
}

/// Where a run of parts is, in words: a PDF's pages from 1, an EPUB's place as a share of the
/// book, which is all a reflowable book has.
pub fn label(part: usize, last: usize, pdf: bool, parts: usize) -> String {
    if pdf {
        if last > part {
            format!("pages {}–{}", part + 1, last + 1)
        } else {
            format!("page {}", part + 1)
        }
    } else {
        format!("{}% of the way through", part * 100 / parts.max(1))
    }
}

/// The instructions for an answer from the book.
pub fn instructions() -> String {
    "You answer a learner's questions about a book they are reading, from numbered excerpts of \
     it. Keep to what the excerpts say: explain it in plain words, with an example when it helps, \
     and use what everyone knows only to make it clear. After each sentence that draws on an \
     excerpt, cite it by its number in square brackets, like [2], or [1][3] for more than one; \
     cite only the numbers you were given. When the excerpts do not hold the answer, say so \
     plainly, and say what they do cover that is close to it. Keep answers short (under 250 \
     words) unless asked for more. Markdown; $…$ for math. Reply in the language the learner \
     writes in."
        .to_owned()
}

/// The request for the answer to the last turn of `turns` (the learner's question): the
/// instructions, the book, the paragraph about the learner and the excerpts as material, and the
/// conversation as turns. The effort is the one the learner chose (`chosen`), else medium.
pub fn request(
    book: &BookContext,
    excerpts: &[Cited],
    turns: &[Turn],
    chosen: Option<Effort>,
    learner: &str,
) -> Request {
    let mut request = Request::new(instructions());
    request.effort = chosen.or(Some(Effort::Medium));
    let request = learner::add_to(request, learner);
    let request = BookContext {
        text: String::new(),
        ..book.clone()
    }
    .add_to(request, "the book's text");
    let request = request.with_material("the excerpts", numbered(excerpts));
    turns
        .iter()
        .fold(request, |request, turn| match turn.speaker {
            Speaker::Learner => request.ask(turn.text.as_str()),
            Speaker::Tutor => request.reply(turn.text.as_str()),
        })
}

/// The excerpts as the model reads them: each under its number and where it is.
fn numbered(excerpts: &[Cited]) -> String {
    if excerpts.is_empty() {
        return "(No passage of the book has the question's words.)".to_owned();
    }
    excerpts
        .iter()
        .enumerate()
        .map(|(i, e)| format!("[{}] ({})\n{}", i + 1, e.label, e.text.trim()))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The excerpts for a question: the chapter being read first (cut to `CHAPTER`), then the
/// passages found, each cut so that all of them fit `FOUND`, a passage found inside the chapter
/// left out as said already.
pub fn excerpts(chapter: Option<Cited>, found: Vec<Cited>) -> Vec<Cited> {
    let mut out: Vec<Cited> = Vec::new();
    if let Some(mut chapter) = chapter.filter(|c| !c.text.trim().is_empty()) {
        chapter.text = cut(&chapter.text, CHAPTER);
        out.push(chapter);
    }
    let mut room = FOUND;
    for mut excerpt in found {
        let inside = out
            .first()
            .is_some_and(|c| c.part <= excerpt.part && excerpt.last <= c.last);
        if inside || room == 0 {
            continue;
        }
        excerpt.text = cut(&excerpt.text, room);
        room = room.saturating_sub(excerpt.text.chars().count());
        out.push(excerpt);
    }
    out
}

/// The words a follow-up question is looked up with: its own, and the question before it, which
/// names what "it" and "that" are.
pub fn looked_up(turns: &[Turn]) -> String {
    let mut asked = turns
        .iter()
        .rev()
        .filter(|t| t.speaker == Speaker::Learner)
        .map(|t| t.text.as_str());
    match (asked.next(), asked.next()) {
        (Some(last), Some(before)) => format!("{last} {before}"),
        (Some(last), None) => last.to_owned(),
        _ => String::new(),
    }
}

/// The answer with each citation of an excerpt made a link to its place: `[2]` becomes
/// `[[2]](place:<part>)`, whose words are `[2]` (Markdown allows brackets in a link's words when
/// they pair; escaped brackets, `\[2\]`, would be read as math). A number no excerpt has, and
/// brackets that hold anything but a number, are left as written; so is everything inside code.
pub fn linked(answer: &str, cited: &[Cited]) -> String {
    let mut out = String::with_capacity(answer.len() + 32);
    let mut in_code = false;
    let mut rest = answer;
    while let Some(at) = rest.find(['[', '`']) {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        if rest.starts_with('`') {
            in_code = !in_code;
            out.push('`');
            rest = &rest[1..];
            continue;
        }
        let number = rest[1..]
            .find(']')
            .map(|end| &rest[1..1 + end])
            .filter(|n| !n.is_empty() && n.len() <= 3 && n.chars().all(|c| c.is_ascii_digit()));
        // A citation already a link's words (`[2](…)`) is left as it is.
        let excerpt = number
            .filter(|n| !in_code && !rest[n.len() + 2..].starts_with('('))
            .and_then(|n| n.parse::<usize>().ok())
            .and_then(|n| Some((n, cited.get(n.checked_sub(1)?)?)));
        match (number, excerpt) {
            (Some(n), Some((number, excerpt))) => {
                out.push_str(&format!("[[{number}]]({PLACE}{})", excerpt.part));
                rest = &rest[n.len() + 2..];
            }
            _ => {
                out.push('[');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// An answer as it is kept in the vault: its citations followed by a line that says where each
/// excerpt is (`places`, in the order of their numbers), so that a reader of the file, in
/// Obsidian or elsewhere, knows what `[2]` was.
pub fn with_sources(answer: &str, places: &[String]) -> String {
    if places.is_empty() {
        return answer.to_owned();
    }
    let sources: Vec<String> = places
        .iter()
        .enumerate()
        .map(|(i, place)| format!("[{}] {place}", i + 1))
        .collect();
    format!("{}\n\n*From* {}", answer.trim_end(), sources.join(" · "))
}

/// What `with_sources` put under a kept answer: the answer without its line of sources, and
/// the places that line names, in the order of their numbers. An answer without such a line, or
/// with one that does not read as `with_sources` wrote it, is given back whole, with no places.
pub fn sources_of(kept: &str) -> (String, Vec<String>) {
    const LINE: &str = "\n\n*From* ";
    let whole = || (kept.trim().to_owned(), Vec::new());
    let kept_end = kept.trim_end();
    let Some(at) = kept_end.rfind(LINE) else {
        return whole();
    };
    let line = &kept_end[at + LINE.len()..];
    if line.contains('\n') {
        return whole();
    }
    let mut places = Vec::new();
    for (i, piece) in line.split(" · ").enumerate() {
        match piece.strip_prefix(&format!("[{}] ", i + 1)) {
            Some(place) if !place.is_empty() => places.push(place.to_owned()),
            _ => return whole(),
        }
    }
    (kept_end[..at].trim().to_owned(), places)
}

/// The pages a PDF's place names, from 0: `page 4` is (3, 3), `pages 6–7` is (5, 6).
pub fn pages_of(place: &str) -> Option<(usize, usize)> {
    let number = |n: &str| n.trim().parse::<usize>().ok()?.checked_sub(1);
    if let Some(pages) = place.strip_prefix("pages ") {
        let (first, last) = pages.split_once('–')?;
        return Some((number(first)?, number(last)?));
    }
    let page = number(place.strip_prefix("page ")?)?;
    Some((page, page))
}

/// The part a link of an answer leads to, if it is a place in the book.
pub fn place_of(link: &str) -> Option<usize> {
    link.strip_prefix(PLACE)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cited(part: usize, last: usize, text: &str) -> Cited {
        Cited {
            part,
            last,
            label: label(part, last, true, 20),
            text: text.into(),
        }
    }

    #[test]
    fn a_citation_becomes_a_link_to_its_excerpts_place() {
        let excerpts = [cited(3, 3, "a"), cited(8, 9, "b")];
        assert_eq!(
            linked("Light bends [1]. It is slow [2][3]. See `x[1]`.", &excerpts),
            "Light bends [[1]](place:3). It is slow [[2]](place:8)[3]. See `x[1]`."
        );
        // Brackets that are not a citation, and a link already, stay as they are.
        assert_eq!(
            linked("[a] and [](x) [1](y)", &excerpts),
            "[a] and [](x) [1](y)"
        );
        assert_eq!(place_of("place:8"), Some(8));
        assert_eq!(place_of("https://x"), None);
    }

    #[test]
    fn a_kept_answer_says_where_each_of_its_citations_is() {
        let places = ["page 2".to_owned(), "pages 5–6".to_owned()];
        assert_eq!(
            with_sources("Light bends [1][2].\n", &places),
            "Light bends [1][2].\n\n*From* [1] page 2 · [2] pages 5–6"
        );
        assert_eq!(with_sources("Not in the book.", &[]), "Not in the book.");
    }

    #[test]
    fn a_kept_answers_sources_are_read_back_as_they_were_written() {
        let places = vec!["page 2".to_owned(), "“Space and Time”".to_owned()];
        let kept = with_sources("Light bends [1][2].", &places);
        assert_eq!(
            sources_of(&kept),
            ("Light bends [1][2].".to_owned(), places)
        );
        // Another last line, or one numbered otherwise, is the answer's own.
        assert_eq!(
            sources_of("A.\n\n*From* the book"),
            ("A.\n\n*From* the book".into(), vec![])
        );
        assert_eq!(
            sources_of("A.\n\n*From* [2] page 1").1,
            Vec::<String>::new()
        );
        assert_eq!(sources_of("No sources.").0, "No sources.");
        assert_eq!(pages_of("page 4"), Some((3, 3)));
        assert_eq!(pages_of("pages 6–7"), Some((5, 6)));
        assert_eq!(pages_of("page 0"), None);
        assert_eq!(pages_of("“Space and Time”"), None);
    }

    #[test]
    fn an_excerpt_is_called_by_its_pages_or_its_share_of_the_book() {
        assert_eq!(label(3, 3, true, 20), "page 4");
        assert_eq!(label(5, 6, true, 20), "pages 6–7");
        assert_eq!(label(50, 52, false, 200), "25% of the way through");
    }

    #[test]
    fn the_chapter_comes_first_and_what_it_holds_is_not_sent_twice() {
        let chapter = cited(4, 7, "the chapter");
        let found = vec![cited(5, 5, "inside it"), cited(12, 12, "elsewhere")];
        let out = excerpts(Some(chapter), found);
        let texts: Vec<&str> = out.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, ["the chapter", "elsewhere"]);
        // The passages found are cut to fit their room.
        let long = "x".repeat(FOUND + 50);
        let out = excerpts(None, vec![cited(1, 1, &long), cited(2, 2, "more")]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text.chars().count(), FOUND);
    }

    #[test]
    fn a_follow_up_is_looked_up_with_the_question_before_it() {
        let turn = |speaker, text: &str| Turn {
            speaker,
            text: text.into(),
        };
        let turns = [
            turn(Speaker::Learner, "What is simultaneity?"),
            turn(Speaker::Tutor, "It is …"),
            turn(Speaker::Learner, "Why does it depend on motion?"),
        ];
        assert_eq!(
            looked_up(&turns),
            "Why does it depend on motion? What is simultaneity?"
        );
    }

    #[test]
    fn the_excerpts_go_to_the_model_numbered_with_where_they_are() {
        let text = numbered(&[cited(0, 0, "One."), cited(4, 5, "Two.")]);
        assert_eq!(text, "[1] (page 1)\nOne.\n\n[2] (pages 5–6)\nTwo.");
    }
}
