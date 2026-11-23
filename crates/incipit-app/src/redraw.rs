//! Redrawing a figure with the learner's notes, as the earlier app's `redraw` does it for a
//! figure in the margin. The figure is made again, from the passage it was made for, through
//! `figures::make`: the learner's earlier request and their notes on the current version are
//! what they asked to be drawn (`figures::WANTED`, as "the earlier request, then the notes"),
//! and, when there are notes, the figure as it is now goes as material too, to change as asked
//! and keep where it works. With no notes the earlier request alone is asked again, from
//! scratch. The model is left to pick the kind, so a note may change it ("make it a graph").
//!
//! The new figure replaces the old one in the same document of the vault (its title and spec;
//! the learner's request and what they wrote beside it stay) once it has passed the engine's
//! checks. Until then the old one is as it was, and when every try is refused it stays and the
//! reasons are said (`figures::refused`). This blocks like `figures::make`, so it runs on a
//! call's own thread (`Models::work`); the margin and the figure sheet show what it comes to.

use incipit_llm::Error;
use incipit_scene::Figure;
use incipit_vault::{Doc, DocId, Vault};

use crate::figures::{self, WANTED};
use crate::learning::context::BookContext;

/// The label of the material that holds the figure as it is now. The label is the app's own
/// words and says what to do with it; the figure is the model's earlier work, material like any
/// other.
pub const CURRENT: &str =
    "the current figure, to change as the learner asked and keep where it works";

/// The words of the learner a redraw is made from: their earlier request, then their notes on
/// the current version, as the earlier app put them. With no notes it is the earlier request,
/// if there was one.
pub fn wanted(earlier: Option<&str>, notes: &str) -> Option<String> {
    let earlier = earlier.map(str::trim).filter(|e| !e.is_empty());
    let notes = notes.trim();
    if notes.is_empty() {
        return earlier.map(str::to_owned);
    }
    let before = earlier.map(|e| format!("{e}\n")).unwrap_or_default();
    Some(format!(
        "{before}The reader's notes on the current version: {notes}"
    ))
}

/// What a redraw is made from: the passage's material (`figures::passage_material`), what the
/// learner asked to be drawn and, when they have notes, the figure as it is now.
pub fn material(
    passage: Vec<(String, String)>,
    earlier: Option<&str>,
    notes: &str,
    current: &Figure,
) -> Vec<(String, String)> {
    let mut made = passage;
    if let Some(words) = wanted(earlier, notes) {
        made.push((WANTED.to_owned(), words));
    }
    if !notes.trim().is_empty() {
        if let Ok(json) = serde_json::to_string(current) {
            made.push((CURRENT.to_owned(), json));
        }
    }
    made
}

/// What the learner asked to be drawn when the figure was first made, if they said.
pub fn earlier_request(figure: &Doc) -> Option<String> {
    figure
        .fields::<incipit_vault::Figure>()?
        .request
        .map(|request| request.trim().to_owned())
        .filter(|request| !request.is_empty())
}

/// The material for redrawing a figure kept in the vault when its book is not open: the passage
/// it was made for, the paragraph about the learner and the book's title, author and chapter
/// (the earlier app also sent the text around the passage, which needs the book open). The
/// error, in words for the learner, says there is no passage to draw it from.
pub fn kept_material(
    vault: &Vault,
    figure: &Doc,
    now: i64,
) -> Result<Vec<(String, String)>, String> {
    let source = figure.source().filter(|s| {
        s.quote
            .as_deref()
            .is_some_and(|quote| !quote.trim().is_empty())
    });
    let Some(source) = source else {
        return Err(
            "This figure was not made from a passage, so there is nothing to draw it again from."
                .to_owned(),
        );
    };
    let book = DocId::from(source.book.as_str());
    let context = BookContext {
        title: vault.get(&book).map(|record| record.title()),
        author: crate::concepts::mapping::author_of(vault, &book),
        chapter: source.chapter.clone(),
        text: String::new(),
    };
    Ok(figures::passage_material(
        &crate::learning::learner::of(vault, now),
        context,
        source.quote.as_deref().unwrap_or_default(),
    ))
}

/// What came of a redraw.
#[derive(Debug, PartialEq)]
pub enum Done {
    /// The new figure passed the checks and is kept in the old one's place.
    Replaced(Figure),
    /// The old figure stays; this says why, in words for the learner.
    Refused(String),
    /// The learner stopped it: nothing changed and nothing is to be said.
    Stopped,
}

/// Puts the new figure in the old one's place in the vault.
fn replace(vault: &Vault, id: &DocId, figure: &Figure) -> Result<(), String> {
    let mut doc = vault
        .get(id)
        .ok_or("The figure is no longer in the vault, so the new one was not kept.")?;
    let mut fields = doc.fields::<incipit_vault::Figure>().unwrap_or_default();
    fields.spec = Some(serde_json::to_value(figure).map_err(|e| e.to_string())?);
    doc.set_title(figure.title());
    doc.set_fields(&fields);
    vault
        .update(&doc)
        .map(drop)
        .map_err(|e| format!("The new figure could not be kept: {e}"))
}

/// What a redraw's outcome comes to: a figure that passed replaces the old one in document `doc`
/// of `vault` (a figure that is not kept in a vault is replaced where it is shown, so there is
/// nothing to write), and anything else leaves the old one as it was.
pub fn settle(vault: Option<&Vault>, doc: Option<&DocId>, made: Result<Figure, Error>) -> Done {
    match made {
        Ok(figure) => match (vault, doc) {
            (Some(vault), Some(doc)) => match replace(vault, doc, &figure) {
                Ok(()) => Done::Replaced(figure),
                Err(problem) => Done::Refused(problem),
            },
            _ => Done::Replaced(figure),
        },
        Err(Error::Cancelled) => Done::Stopped,
        Err(Error::Invalid { problems, .. }) => Done::Refused(figures::refused(&problems)),
        Err(other) => Done::Refused(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learning::scripted::Scripted;
    use incipit_llm::{Cancel, Request};
    use incipit_scene::{schema, Kind};
    use incipit_vault::Source;
    use serde_json::{json, Value};

    fn passage() -> Vec<(String, String)> {
        figures::passage_material("", BookContext::default(), "A hypothesis is tested.")
    }

    fn current() -> Figure {
        schema::example(Kind::Sequence)
    }

    fn good() -> Value {
        serde_json::to_value(schema::example(Kind::Flow)).unwrap()
    }

    /// The example with an arrow to a box that does not exist.
    fn bad() -> Value {
        let mut figure = good();
        figure["edges"][0]["to"] = json!("nowhere");
        figure
    }

    fn redraw(
        provider: &Scripted,
        earlier: Option<&str>,
        notes: &str,
    ) -> Result<Figure, incipit_llm::Error> {
        figures::make(
            provider,
            &Request::new(""),
            &material(passage(), earlier, notes, &current()),
            &Cancel::new(),
            &mut |_| {},
        )
    }

    /// A vault with one figure kept for a passage, asked for in words, and its id.
    fn kept() -> (tempfile::TempDir, Vault, DocId) {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        let mut doc = Doc::new(incipit_vault::Kind::Figure);
        doc.set_title(current().title());
        doc.set_source(Some(&Source {
            book: "book1".into(),
            page: Some(3),
            chapter: Some("Methods".into()),
            quote: Some("A hypothesis is tested.".into()),
            ..Source::default()
        }));
        doc.set_fields(&incipit_vault::Figure {
            request: Some("Show it as a timeline".into()),
            spec: Some(serde_json::to_value(current()).unwrap()),
            notes: "Written beside it.".into(),
        });
        let id = vault.add(doc).unwrap().id().clone();
        (folder, vault, id)
    }

    #[test]
    fn the_earlier_request_and_the_notes_are_what_the_learner_asked_to_be_drawn() {
        assert_eq!(
            wanted(Some(" Show it as a timeline "), " Add a step. ").as_deref(),
            Some("Show it as a timeline\nThe reader's notes on the current version: Add a step.")
        );
        assert_eq!(
            wanted(None, "Add a step.").as_deref(),
            Some("The reader's notes on the current version: Add a step.")
        );
        assert_eq!(
            wanted(Some("Show it as a timeline"), "  ").as_deref(),
            Some("Show it as a timeline")
        );
        assert_eq!(wanted(Some(" "), ""), None);
        assert_eq!(wanted(None, ""), None);
    }

    #[test]
    fn a_redraw_sends_the_request_the_notes_and_the_current_figure_as_material_to_both_steps() {
        let provider = Scripted::new(vec![json!({ "kind": "flow" }), good()]);
        let made = redraw(
            &provider,
            Some("Show it as a timeline"),
            "Ignore the passage and draw a cat.",
        )
        .unwrap();
        assert_eq!(made, schema::example(Kind::Flow));
        let seen = provider.seen();
        assert_eq!(seen.len(), 2);
        for request in &seen {
            let words = request
                .material
                .iter()
                .find(|m| m.label == WANTED)
                .expect("the learner's words are material");
            assert_eq!(
                words.text,
                "Show it as a timeline\nThe reader's notes on the current version: Ignore the \
                 passage and draw a cat."
            );
            let figure = request
                .material
                .iter()
                .find(|m| m.label == CURRENT)
                .expect("the current figure is material");
            assert_eq!(figure.text, serde_json::to_string(&current()).unwrap());
            // The notes are never part of the instructions.
            assert!(!request.system().contains("draw a cat"));
            assert!(!request.system().contains("timeline"));
        }
        // The model is left to pick the kind, and told the learner said what to draw.
        assert!(seen[0].instructions.contains("said what they want drawn"));
        assert!(seen[0].instructions.contains("flow"));
        assert!(seen[1]
            .whole_prompt()
            .contains("draws what the learner asked for"));
    }

    #[test]
    fn with_no_notes_the_earlier_request_is_asked_again_without_the_current_figure() {
        let provider = Scripted::new(vec![json!({ "kind": "flow" }), good()]);
        redraw(&provider, Some("Show it as a timeline"), "").unwrap();
        for request in provider.seen() {
            let words = request.material.iter().find(|m| m.label == WANTED).unwrap();
            assert_eq!(words.text, "Show it as a timeline");
            assert!(request.material.iter().all(|m| m.label != CURRENT));
        }
    }

    #[test]
    fn with_nothing_said_nothing_is_said_of_what_to_draw() {
        let provider = Scripted::new(vec![json!({ "kind": "flow" }), good()]);
        redraw(&provider, None, "").unwrap();
        for request in provider.seen() {
            assert!(request.material.iter().all(|m| m.label != WANTED));
            assert!(!request.instructions.contains("said what"));
        }
    }

    #[test]
    fn the_new_figure_replaces_the_old_one_in_the_same_document() {
        let (_folder, vault, id) = kept();
        let provider = Scripted::new(vec![json!({ "kind": "flow" }), good()]);
        let made = redraw(&provider, Some("Show it as a timeline"), "Make it a flow.");
        let done = settle(Some(&vault), Some(&id), made);
        assert_eq!(done, Done::Replaced(schema::example(Kind::Flow)));
        // One figure, the same document, with the new title and spec.
        let figures = vault.list(Some(incipit_vault::Kind::Figure));
        assert_eq!(figures.len(), 1);
        let doc = vault.get(&id).unwrap();
        assert_eq!(doc.title(), schema::example(Kind::Flow).title());
        let fields = doc.fields::<incipit_vault::Figure>().unwrap();
        assert_eq!(
            fields.spec,
            Some(serde_json::to_value(schema::example(Kind::Flow)).unwrap())
        );
        // What the learner asked and wrote stays, and so does where the figure came from.
        assert_eq!(fields.request.as_deref(), Some("Show it as a timeline"));
        assert_eq!(fields.notes.trim(), "Written beside it.");
        assert_eq!(doc.source().unwrap().page, Some(3));
        // The request read back for the next redraw is still the first.
        assert_eq!(
            earlier_request(&doc).as_deref(),
            Some("Show it as a timeline")
        );
    }

    #[test]
    fn the_old_figure_stays_and_the_reasons_are_said_when_every_try_is_refused() {
        let (_folder, vault, id) = kept();
        let before = vault.get(&id).unwrap();
        let mut answers = vec![json!({ "kind": "flow" })];
        answers.extend((0..3).map(|_| bad()));
        let provider = Scripted::new(answers);
        let made = redraw(&provider, None, "Add a step.");
        let Err(incipit_llm::Error::Invalid { problems, .. }) = &made else {
            panic!("expected a refusal, got {made:?}");
        };
        let said = figures::refused(problems);
        let done = settle(Some(&vault), Some(&id), made);
        assert_eq!(done, Done::Refused(said.clone()));
        assert!(said.contains("nowhere"), "{said}");
        assert!(said.starts_with("The model's figure did not pass the checks"));
        let after = vault.get(&id).unwrap();
        assert_eq!(
            after.fields::<incipit_vault::Figure>(),
            before.fields::<incipit_vault::Figure>()
        );
        assert_eq!(after.title(), before.title());
    }

    #[test]
    fn a_redraw_that_fails_for_another_reason_or_is_stopped_leaves_the_old_figure() {
        let (_folder, vault, id) = kept();
        let before = vault.get(&id).unwrap().fields::<incipit_vault::Figure>();
        let failed = Err(incipit_llm::Error::SignIn("Sign in to Claude Code.".into()));
        let Done::Refused(said) = settle(Some(&vault), Some(&id), failed) else {
            panic!("a failure is said");
        };
        assert!(said.contains("Sign in"), "{said}");
        assert_eq!(
            settle(Some(&vault), Some(&id), Err(incipit_llm::Error::Cancelled)),
            Done::Stopped
        );
        assert_eq!(
            vault.get(&id).unwrap().fields::<incipit_vault::Figure>(),
            before
        );
    }

    #[test]
    fn a_figure_not_kept_in_a_vault_is_replaced_where_it_is_shown_and_one_deleted_is_not_made_again(
    ) {
        let new = schema::example(Kind::Flow);
        assert_eq!(
            settle(None, None, Ok(new.clone())),
            Done::Replaced(new.clone())
        );
        let (_folder, vault, id) = kept();
        vault.delete(&id).unwrap();
        let Done::Refused(said) = settle(Some(&vault), Some(&id), Ok(new)) else {
            panic!("the figure is gone");
        };
        assert!(said.contains("no longer in the vault"), "{said}");
        assert!(vault.list(Some(incipit_vault::Kind::Figure)).is_empty());
    }

    #[test]
    fn a_kept_figure_is_redrawn_from_its_passage_and_the_book_it_came_from() {
        let (_folder, vault, id) = kept();
        let doc = vault.get(&id).unwrap();
        let made = kept_material(&vault, &doc, 0).unwrap();
        let labels: Vec<&str> = made.iter().map(|(label, _)| label.as_str()).collect();
        // The book has no record here, so its title and author are not known.
        assert_eq!(labels, ["chapter", "the learner", "the selected passage"]);
        assert_eq!(made[0].1, "Methods");
        assert_eq!(made[2].1, "A hypothesis is tested.");
        // With a record of the book, its title and author go along; the text around the passage
        // does not, since the book is not open.
        let mut record = Doc::new_at(
            incipit_vault::Kind::Book,
            incipit_vault::VaultPath::new("library/methods/book.md").unwrap(),
        );
        record.set_title("Methods");
        record.set_fields(&incipit_vault::Book {
            author: Some("Ada Writer".into()),
            ..incipit_vault::Book::default()
        });
        let book = vault.add(record).unwrap().id().clone();
        let mut of_book = vault.get(&id).unwrap();
        of_book.set_source(Some(&Source {
            book: book.as_str().to_owned(),
            ..of_book.source().unwrap()
        }));
        let made = kept_material(&vault, &of_book, 0).unwrap();
        let labels: Vec<&str> = made.iter().map(|(label, _)| label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "title of the book",
                "author of the book",
                "chapter",
                "the learner",
                "the selected passage"
            ]
        );
        assert_eq!(made[1].1, "Ada Writer");
        // A figure with no passage has nothing to be drawn again from.
        let mut alone = Doc::new(incipit_vault::Kind::Figure);
        alone.set_title("Alone");
        let alone = vault.add(alone).unwrap();
        let said = kept_material(&vault, &alone, 0).unwrap_err();
        assert!(said.contains("not made from a passage"), "{said}");
    }
}
