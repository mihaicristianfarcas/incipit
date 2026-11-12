//! What each kind of document holds beyond the common properties, read and written as typed
//! values: `doc.fields::<Card>()` and `doc.set_fields(&card)`. Some live in properties (a card's
//! deck), some in the body in a shape a person reads easily in any editor (a card's `## Front`
//! and `## Back`, a session's `## You` and `## Tutor` turns, a figure's fenced `figure` block,
//! an annotation's passage as a `> [!quote]` callout above the answer).
//!
//! Readers are forgiving, since files are also written by hand and by agents: a missing field
//! gets its default, a card without sections is all front, a figure whose block is not JSON has
//! no spec.

use crate::doc::Doc;
use crate::kind::Kind;
use crate::value::Value;

/// Where in a book a document comes from: the book's id, and as much of the place as is known.
/// The reader may keep more about a passage here (its boxes on the page); those keys are kept.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Source {
    /// The book's id (its `book.md`'s `id`).
    pub book: String,
    pub chapter: Option<String>,
    /// A PDF's page, from 1.
    pub page: Option<u32>,
    /// A block of a reflowable book (EPUB), from 0.
    pub block: Option<u32>,
    /// The passage, word for word.
    pub quote: Option<String>,
    /// A few words before and after the passage, to find it again when the text moves.
    pub prefix: Option<String>,
    pub suffix: Option<String>,
}

impl Source {
    pub(crate) fn from_value(value: &Value) -> Option<Source> {
        let map = value.as_map()?;
        Some(Source {
            book: map.text("book")?,
            chapter: map.text("chapter"),
            page: map.get("page").and_then(Value::as_u32),
            block: map.get("block").and_then(Value::as_u32),
            quote: map.text("quote"),
            prefix: map.text("prefix"),
            suffix: map.text("suffix"),
        })
    }

    /// The source as a property, over the old one so its other keys stay.
    pub(crate) fn to_value(&self, old: Option<&Value>) -> Value {
        let mut map = old.and_then(Value::as_map).cloned().unwrap_or_default();
        let mut put = |key: &str, value: Option<Value>| match value {
            Some(value) => map.set(key, value),
            None => {
                map.remove(key);
            }
        };
        put("book", Some(Value::from(self.book.as_str())));
        put("chapter", self.chapter.clone().map(Value::from));
        put("page", self.page.map(|p| Value::Int(p.into())));
        put("block", self.block.map(|b| Value::Int(b.into())));
        put("quote", self.quote.clone().map(Value::from));
        put("prefix", self.prefix.clone().map(Value::from));
        put("suffix", self.suffix.clone().map(Value::from));
        Value::Map(map)
    }

    /// "p. 12" or "block 40": where it is, in a few characters.
    pub fn place(&self) -> String {
        match (self.page, self.block) {
            (Some(page), _) => format!("p. {page}"),
            (None, Some(block)) => format!("block {block}"),
            (None, None) => String::new(),
        }
    }
}

/// A kind's own fields, read from a document of that kind and written back into it.
pub trait Fields: Sized {
    const KIND: Kind;
    fn read(doc: &Doc) -> Self;
    fn write(&self, doc: &mut Doc);
}

impl Doc {
    /// This kind's own fields; `None` when the document is of another kind.
    pub fn fields<F: Fields>(&self) -> Option<F> {
        (self.kind() == F::KIND).then(|| F::read(self))
    }

    /// Writes a kind's own fields; does nothing to a document of another kind.
    pub fn set_fields<F: Fields>(&mut self, fields: &F) {
        if self.kind() == F::KIND {
            fields.write(self);
        }
    }
}

fn text_or_none(text: &str) -> Option<Value> {
    let text = text.trim();
    (!text.is_empty()).then(|| Value::from(text))
}

/// A note's own field.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Note {
    /// An explanation's grade, 0 to 100.
    pub score: Option<f64>,
}

impl Fields for Note {
    const KIND: Kind = Kind::Note;
    fn read(doc: &Doc) -> Self {
        Note {
            score: doc.properties().get("score").and_then(Value::as_f64),
        }
    }
    fn write(&self, doc: &mut Doc) {
        doc.set_property("score", self.score.map(Value::Float));
    }
}

/// A concept's own fields. The body explains it in free Markdown; its other names are the common
/// `aliases`, and the concepts close to it that it does not need first are links in the common
/// `related`, so Obsidian reads both as it reads them anywhere.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Concept {
    /// A sentence or two.
    pub summary: String,
    /// Links to the concepts a learner needs first (`"[[Limit]]"`). A concept listed here is not
    /// listed again in `related`.
    pub builds_on: Vec<String>,
    /// The passages it came from: a book and the page or block, with the passage's words.
    pub sources: Vec<Source>,
}

impl Fields for Concept {
    const KIND: Kind = Kind::Concept;
    fn read(doc: &Doc) -> Self {
        let p = doc.properties();
        Concept {
            summary: p.text("summary").unwrap_or_default(),
            builds_on: p.texts("builds-on"),
            sources: p
                .get("sources")
                .and_then(Value::as_list)
                .map(|items| items.iter().filter_map(Source::from_value).collect())
                .unwrap_or_default(),
        }
    }
    fn write(&self, doc: &mut Doc) {
        doc.set_property("summary", text_or_none(&self.summary));
        doc.set_property("builds-on", Some(Value::from(self.builds_on.clone())));
        let old = doc.properties().get("sources").and_then(Value::as_list);
        let sources = self
            .sources
            .iter()
            .enumerate()
            .map(|(i, s)| s.to_value(old.and_then(|o| o.get(i))))
            .collect();
        doc.set_property("sources", Some(Value::List(sources)));
    }
}

/// A flashcard. Its review history is not in the file but in `log/reviews.jsonl`, so reviewing
/// never changes the card.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub front: String,
    pub back: String,
    /// A book, or "general".
    pub deck: String,
    /// The id of a figure shown with it.
    pub figure: Option<String>,
    pub suspended: bool,
}

impl Default for Card {
    fn default() -> Self {
        Card {
            front: String::new(),
            back: String::new(),
            deck: DEFAULT_DECK.to_owned(),
            figure: None,
            suspended: false,
        }
    }
}

/// The deck of a card that does not say.
const DEFAULT_DECK: &str = "general";

/// The text under a `## Heading` line (matched without regard to case), up to the next
/// second-level heading in `stops`.
fn section<'a>(body: &'a str, heading: &str, stops: &[&str]) -> Option<&'a str> {
    let is_heading = |line: &str, name: &str| {
        line.trim_end()
            .strip_prefix("##")
            .is_some_and(|rest| rest.trim().eq_ignore_ascii_case(name))
    };
    let mut start = None;
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        match start {
            None if is_heading(line, heading) => start = Some(offset + line.len()),
            Some(from) if stops.iter().any(|stop| is_heading(line, stop)) => {
                return Some(&body[from..offset]);
            }
            _ => {}
        }
        offset += line.len();
    }
    start.map(|from| &body[from..])
}

impl Fields for Card {
    const KIND: Kind = Kind::Card;
    fn read(doc: &Doc) -> Self {
        let body = doc.body();
        let front = section(body, "Front", &["Back"]).unwrap_or(body);
        let back = section(body, "Back", &[]).unwrap_or("");
        let p = doc.properties();
        Card {
            front: front.trim().to_owned(),
            back: back.trim().to_owned(),
            deck: p.text("deck").unwrap_or_else(|| DEFAULT_DECK.to_owned()),
            figure: p.text("figure"),
            suspended: p.get("suspended").and_then(Value::as_bool) == Some(true),
        }
    }
    fn write(&self, doc: &mut Doc) {
        doc.set_property("deck", text_or_none(&self.deck));
        doc.set_property("figure", self.figure.as_deref().and_then(text_or_none));
        doc.set_property("suspended", self.suspended.then_some(Value::Bool(true)));
        doc.set_body(format!(
            "## Front\n\n{}\n\n## Back\n\n{}\n",
            self.front.trim(),
            self.back.trim()
        ));
    }
}

/// Whether a model's answer to an annotation is still being written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    Pending,
    #[default]
    Ready,
    Failed,
}

/// What was made from a passage. The passage is the source's `quote`; the body shows it as a
/// callout, then the answer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Annotation {
    pub color: Option<String>,
    /// What the learner asked.
    pub request: Option<String>,
    pub status: Status,
    pub error: Option<String>,
    /// Which model wrote the answer.
    pub model: Option<String>,
    /// The body without the passage's callout.
    pub answer: String,
}

/// The body without a `> [!quote]` callout at its top.
fn without_quote(body: &str) -> &str {
    let trimmed = body.trim_start();
    if !trimmed.starts_with("> [!quote]") {
        return body;
    }
    let mut at = body.len() - trimmed.len();
    for line in trimmed.split_inclusive('\n') {
        if !line.starts_with('>') {
            break;
        }
        at += line.len();
    }
    &body[at..]
}

/// The passage as a callout at the top of a file, for whoever opens it.
fn quote_callout(doc: &Doc) -> String {
    let Some(source) = doc.source() else {
        return String::new();
    };
    let Some(quote) = source.quote.clone().filter(|q| !q.trim().is_empty()) else {
        return String::new();
    };
    let mut out = format!("> [!quote] {}\n", source.place()).replace(" \n", "\n");
    for line in quote.lines() {
        out.push_str("> ");
        out.push_str(line);
        out.push('\n');
    }
    out.push('\n');
    out
}

impl Fields for Annotation {
    const KIND: Kind = Kind::Annotation;
    fn read(doc: &Doc) -> Self {
        let p = doc.properties();
        Annotation {
            color: p.text("color"),
            request: p.text("request"),
            status: match p.text("status").as_deref() {
                Some("pending") => Status::Pending,
                Some("failed") => Status::Failed,
                _ => Status::Ready,
            },
            error: p.text("error"),
            model: p.text("model"),
            answer: without_quote(doc.body()).trim().to_owned(),
        }
    }
    fn write(&self, doc: &mut Doc) {
        doc.set_property("color", self.color.as_deref().and_then(text_or_none));
        doc.set_property("request", self.request.as_deref().and_then(text_or_none));
        let status = match self.status {
            Status::Pending => Some(Value::from("pending")),
            Status::Failed => Some(Value::from("failed")),
            Status::Ready => None,
        };
        doc.set_property("status", status);
        doc.set_property("error", self.error.as_deref().and_then(text_or_none));
        doc.set_property("model", self.model.as_deref().and_then(text_or_none));
        let body = format!("{}{}\n", quote_callout(doc), self.answer.trim());
        doc.set_body(body);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// The tutor answers.
    #[default]
    Answer,
    /// The tutor asks questions that lead the learner to the answer.
    Socratic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Speaker {
    Learner,
    Tutor,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Turn {
    pub speaker: Speaker,
    pub text: String,
}

/// A conversation with the tutor: `## You` and `## Tutor` sections, after the passage it is about.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Session {
    pub mode: Mode,
    pub turns: Vec<Turn>,
}

impl Fields for Session {
    const KIND: Kind = Kind::Session;
    fn read(doc: &Doc) -> Self {
        let mut turns: Vec<Turn> = Vec::new();
        for line in without_quote(doc.body()).split_inclusive('\n') {
            let speaker = match line.trim_end() {
                "## You" => Some(Speaker::Learner),
                "## Tutor" => Some(Speaker::Tutor),
                _ => None,
            };
            match (speaker, turns.last_mut()) {
                (Some(speaker), _) => turns.push(Turn {
                    speaker,
                    text: String::new(),
                }),
                (None, Some(turn)) => turn.text.push_str(line),
                (None, None) => {}
            }
        }
        for turn in &mut turns {
            turn.text = turn.text.trim().to_owned();
        }
        Session {
            mode: match doc.properties().text("mode").as_deref() {
                Some("socratic") => Mode::Socratic,
                _ => Mode::Answer,
            },
            turns,
        }
    }
    fn write(&self, doc: &mut Doc) {
        let mode = (self.mode == Mode::Socratic).then(|| Value::from("socratic"));
        doc.set_property("mode", mode);
        let mut body = quote_callout(doc);
        for (i, turn) in self.turns.iter().enumerate() {
            if i > 0 {
                body.push('\n');
            }
            let who = match turn.speaker {
                Speaker::Learner => "You",
                Speaker::Tutor => "Tutor",
            };
            body.push_str(&format!("## {who}\n\n{}\n", turn.text.trim()));
        }
        doc.set_body(body);
    }
}

/// A figure kept on its own: its spec (the JSON `incipit-scene` checks and draws) in a fenced
/// `figure` block, then anything written about it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Figure {
    /// What the learner asked for.
    pub request: Option<String>,
    /// `None` when the block is missing or is not JSON.
    pub spec: Option<serde_json::Value>,
    /// The body after the block.
    pub notes: String,
}

/// The text inside the first fenced block of this language, and the body around it.
fn fenced<'a>(body: &'a str, language: &str) -> Option<(&'a str, &'a str, &'a str)> {
    let open = format!("```{language}");
    let mut offset = 0;
    let mut inside: Option<usize> = None;
    for line in body.split_inclusive('\n') {
        let bare = line.trim_end();
        match inside {
            None if bare == open => inside = Some(offset),
            Some(start) if bare == "```" => {
                let content_start = start + body[start..].find('\n').map_or(0, |n| n + 1);
                return Some((
                    &body[..start],
                    &body[content_start..offset],
                    &body[offset + line.len()..],
                ));
            }
            _ => {}
        }
        offset += line.len();
    }
    None
}

impl Fields for Figure {
    const KIND: Kind = Kind::Figure;
    fn read(doc: &Doc) -> Self {
        let (spec, notes) = match fenced(doc.body(), "figure") {
            Some((before, inside, after)) => (
                serde_json::from_str(inside).ok(),
                format!("{}{}", before.trim(), after.trim()),
            ),
            None => (None, doc.body().trim().to_owned()),
        };
        Figure {
            request: doc.properties().text("request"),
            spec,
            notes,
        }
    }
    fn write(&self, doc: &mut Doc) {
        doc.set_property("request", self.request.as_deref().and_then(text_or_none));
        let spec = self
            .spec
            .as_ref()
            .and_then(|s| serde_json::to_string_pretty(s).ok())
            .unwrap_or_else(|| "null".to_owned());
        let notes = self.notes.trim();
        let tail = if notes.is_empty() {
            String::new()
        } else {
            format!("\n{notes}\n")
        };
        doc.set_body(format!("```figure\n{spec}\n```\n{tail}"));
    }
}

/// A book's record. The book's file sits beside it in its folder; what is extracted from the
/// file (pages, text) is a cache in `.incipit/`, made again when missing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Book {
    pub author: Option<String>,
    /// The book's file name in its folder.
    pub file: Option<String>,
    /// `pdf` or `epub`.
    pub format: Option<String>,
    /// The first 16 hex digits of the file's SHA-256: which edition the notes were made on.
    pub edition: Option<String>,
    /// How many pages a PDF has, or blocks any other book has. The reader writes it when the
    /// book is first opened; progress through the book is measured against it.
    pub parts: Option<u32>,
}

impl Fields for Book {
    const KIND: Kind = Kind::Book;
    fn read(doc: &Doc) -> Self {
        let p = doc.properties();
        Book {
            author: p.text("author"),
            file: p.text("file"),
            format: p.text("format"),
            edition: p.text("edition"),
            parts: p.get("parts").and_then(Value::as_u32),
        }
    }
    fn write(&self, doc: &mut Doc) {
        doc.set_property("author", self.author.as_deref().and_then(text_or_none));
        doc.set_property("file", self.file.as_deref().and_then(text_or_none));
        doc.set_property("format", self.format.as_deref().and_then(text_or_none));
        doc.set_property("edition", self.edition.as_deref().and_then(text_or_none));
        doc.set_property("parts", self.parts.map(|n| Value::Int(n.into())));
    }
}

/// The first words of a text, for a title.
fn first_words(text: &str, n: usize) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() > n {
        format!("{}…", words[..n].join(" "))
    } else {
        words.join(" ")
    }
}

/// The first line of Markdown as plain words, for a title.
fn plain_line(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let plain: String = line
        .chars()
        .filter(|c| !matches!(c, '#' | '>' | '*' | '_' | '`' | '[' | ']' | '{' | '}' | '$'))
        .collect();
    plain
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(90)
        .collect()
}

/// A title made from what a document holds, for kinds whose title is not their file name.
pub(crate) fn made_title(doc: &Doc) -> Option<String> {
    let title = match doc.kind() {
        Kind::Card => plain_line(&Card::read(doc).front),
        Kind::Annotation => {
            let quote = doc.source().and_then(|s| s.quote).unwrap_or_default();
            format!(
                "{}: {}",
                doc.doc_type().unwrap_or("note"),
                first_words(&quote, 8)
            )
        }
        Kind::Session => Session::read(doc)
            .turns
            .iter()
            .find(|t| t.speaker == Speaker::Learner)
            .map(|t| plain_line(&t.text))
            .unwrap_or_default(),
        Kind::Figure => Figure::read(doc)
            .spec
            .as_ref()
            .and_then(|s| s.get("title"))
            .and_then(|t| t.as_str())
            .unwrap_or_default()
            .to_owned(),
        Kind::Book => doc
            .path()
            .parent()
            .map(|folder| folder.name().to_owned())
            .unwrap_or_default(),
        Kind::Note | Kind::Concept => String::new(),
    };
    let title = title.trim().to_owned();
    (!title.is_empty()).then_some(title)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::VaultPath;

    fn parse(path: &str, text: &str) -> Doc {
        Doc::parse(VaultPath::new(path).unwrap(), text).unwrap()
    }

    #[test]
    fn a_card_reads_its_sides_from_its_sections() {
        let doc = parse(
            "cards/a.md",
            "---\ndeck: sicp\n---\n## Front\n\nWhat is a closure?\n\n## Back\n\nA function and its environment.\n",
        );
        let card = doc.fields::<Card>().unwrap();
        assert_eq!(card.front, "What is a closure?");
        assert_eq!(card.back, "A function and its environment.");
        assert_eq!(card.deck, "sicp");
        assert_eq!(doc.title(), "What is a closure?");
    }

    #[test]
    fn a_card_written_and_read_again_is_the_same_card() {
        let mut doc = Doc::new(Kind::Card);
        let card = Card {
            front: "2 + 2?".into(),
            back: "4".into(),
            deck: "maths".into(),
            figure: None,
            suspended: true,
        };
        doc.set_fields(&card);
        let back = parse("cards/x.md", &doc.to_text());
        assert_eq!(back.fields::<Card>().unwrap(), card);
    }

    #[test]
    fn an_annotation_keeps_its_passage_as_a_callout_above_the_answer() {
        let mut doc = Doc::new(Kind::Annotation);
        doc.set_source(Some(&Source {
            book: "b1".into(),
            page: Some(12),
            quote: Some("The derivative\nis a rate.".into()),
            ..Source::default()
        }));
        doc.set_fields(&Annotation {
            answer: "It says how fast.".into(),
            status: Status::Pending,
            ..Annotation::default()
        });
        assert!(doc
            .body()
            .starts_with("> [!quote] p. 12\n> The derivative\n> is a rate.\n\n"));
        let back = parse("library/x/annotations/a.md", &doc.to_text());
        let annotation = back.fields::<Annotation>().unwrap();
        assert_eq!(annotation.answer, "It says how fast.");
        assert_eq!(annotation.status, Status::Pending);
        assert_eq!(back.source().unwrap().page, Some(12));
    }

    #[test]
    fn a_session_reads_its_turns() {
        let doc = parse(
            "library/x/sessions/s.md",
            "> [!quote] p. 3\n> A passage\n\n## You\n\nWhy?\n\n## Tutor\n\nBecause.\nTwo lines.\n",
        );
        let session = doc.fields::<Session>().unwrap();
        assert_eq!(session.turns.len(), 2);
        assert_eq!(session.turns[0].speaker, Speaker::Learner);
        assert_eq!(session.turns[1].text, "Because.\nTwo lines.");
        assert_eq!(doc.title(), "Why?");
    }

    #[test]
    fn a_books_length_is_kept_as_a_number_and_left_out_when_unknown() {
        let mut doc = parse(
            "library/x/book.md",
            "---\nkind: book\nformat: pdf\nparts: 312\n---\n",
        );
        assert_eq!(doc.fields::<Book>().unwrap().parts, Some(312));
        let mut book = doc.fields::<Book>().unwrap();
        book.parts = Some(400);
        doc.set_fields(&book);
        assert_eq!(doc.fields::<Book>().unwrap().parts, Some(400));
        book.parts = None;
        doc.set_fields(&book);
        assert_eq!(doc.fields::<Book>().unwrap().parts, None);
        assert!(!doc.properties().contains("parts"));
    }

    #[test]
    fn a_figure_reads_its_spec_and_tolerates_a_broken_one() {
        let doc = parse(
            "figures/f.md",
            "```figure\n{\"title\": \"Waves\"}\n```\n\nSome notes.\n",
        );
        let figure = doc.fields::<Figure>().unwrap();
        assert_eq!(figure.spec.unwrap()["title"], "Waves");
        assert_eq!(figure.notes, "Some notes.");
        assert_eq!(doc.title(), "Waves");
        let broken = parse("figures/g.md", "```figure\n{not json\n```\n");
        assert_eq!(broken.fields::<Figure>().unwrap().spec, None);
    }

    #[test]
    fn a_sources_own_keys_survive_a_change_of_page() {
        let mut doc = parse(
            "notes/n.md",
            "---\nsource:\n  book: b1\n  page: 3\n  rects: [[0.1, 0.2, 0.3, 0.4]]\n---\n",
        );
        let mut source = doc.source().unwrap();
        source.page = Some(4);
        doc.set_source(Some(&source));
        let back = parse("notes/n.md", &doc.to_text());
        let map = back.properties().get("source").unwrap().as_map().unwrap();
        assert_eq!(map.get("page"), Some(&Value::Int(4)));
        assert!(map.get("rects").is_some());
    }

    #[test]
    fn a_concept_keeps_its_links_as_quoted_lists_obsidian_reads() {
        let mut doc = Doc::new(Kind::Concept);
        doc.set_title("Self-attention");
        doc.set_aliases(vec!["Intra-attention".into()]);
        doc.set_related(vec!["[[Recurrence]]".into()]);
        let concept = Concept {
            summary: "Each position looks at every other.".into(),
            builds_on: vec!["[[Attention]]".into(), "[[Softmax]]".into()],
            sources: vec![Source {
                book: "b1".into(),
                chapter: Some("3 Model Architecture".into()),
                page: Some(4),
                quote: Some("An attention function can be described".into()),
                ..Source::default()
            }],
        };
        doc.set_fields(&concept);
        doc.set_body("In my words: …\n");
        let text = doc.to_text();
        assert!(
            text.contains("builds-on:\n  - \"[[Attention]]\"\n"),
            "{text}"
        );
        let back = parse("concepts/Self-attention.md", &text);
        assert_eq!(back.fields::<Concept>().unwrap(), concept);
        assert_eq!(back.aliases(), ["Intra-attention"]);
        assert_eq!(back.related(), ["[[Recurrence]]"]);
        assert_eq!(back.body(), "In my words: …\n");
    }

    #[test]
    fn fields_of_another_kind_are_not_read() {
        let doc = parse("notes/n.md", "Hello\n");
        assert!(doc.fields::<Card>().is_none());
    }
}
