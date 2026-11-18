//! Find: a field over the window that searches the open vault as you type and opens what you
//! pick. It is opened from anywhere with a key, so it is the quick way from one thing to
//! another without going through a page's list. The documents are found by the vault's search;
//! the books' text by each book's index (`book_index`), as the earlier app searched each
//! chapter of the books: a line of the contents that holds every word of the query is a result,
//! ranked among the documents as body text, by eight tenths.
//!
//! The books' indexes are taken when the sheet opens, off the window's thread and one book at
//! a time (the kept one, or made by reading the book when its file changed or it has none),
//! and each book's passages join the results as soon as its index is there. The passages are
//! searched off the window's thread; a search the next has overtaken is dropped.
//!
//! Without clicking: INCIPIT_FIND=words opens it with those words typed.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    div, prelude::*, px, Context, Entity, EventEmitter, IntoElement, KeyDownEvent, Render,
    Subscription, Window,
};
use incipit_book::TextIndex;
use incipit_ui::{field, sheet, FieldEvent, TextField, Type};
use incipit_vault::{DocId, Hit, Kind};

use crate::book_index::{self, Shelved};
use crate::workspace::Workspace;

/// How many results are shown: what fits the sheet without scrolling.
const SHOWN: usize = 8;

/// What the learner did in the sheet.
pub enum Found {
    Open(Kind, DocId),
    /// A passage of a book's text: the book, and the page or block to open it at.
    Read(DocId, usize),
    Closed,
}

/// A passage of a book's text that has every word of the query.
#[derive(Clone, Debug, PartialEq)]
pub struct Passage {
    pub book: DocId,
    /// The book's title and the line of the contents the passage is under.
    pub title: String,
    pub snippet: String,
    pub part: usize,
    pub score: u32,
}

/// One result: a document of the vault, or a passage of a book.
#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    Doc(Hit),
    Passage(Passage),
}

impl Row {
    fn score(&self) -> u32 {
        match self {
            Row::Doc(hit) => hit.score,
            Row::Passage(passage) => passage.score,
        }
    }
}

pub struct FindView {
    workspace: Entity<Workspace>,
    words: Entity<TextField>,
    rows: Vec<Row>,
    at: usize,
    /// Each book's index, by the book, and the vault they are of.
    shelf: HashMap<DocId, (Shelved, Arc<TextIndex>)>,
    shelf_of: Option<PathBuf>,
    /// The books' indexes are being taken.
    stocking: bool,
    /// Counts the searches, so that only the newest one's passages are taken.
    asked: u64,
    _typed: Subscription,
}

impl EventEmitter<Found> for FindView {}

impl FindView {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        let words = cx.new(|cx| TextField::new(cx).placeholder("Find a note, a book, a card"));
        let typed = cx.subscribe(
            &words,
            |this: &mut Self, _, event: &FieldEvent, cx| match event {
                FieldEvent::Changed => this.search(cx),
                FieldEvent::Submit => this.pick(this.at, cx),
                FieldEvent::Cancel => cx.emit(Found::Closed),
            },
        );
        FindView {
            workspace,
            words,
            rows: Vec::new(),
            at: 0,
            shelf: HashMap::new(),
            shelf_of: None,
            stocking: false,
            asked: 0,
            _typed: typed,
        }
    }

    /// Start afresh with `words` typed, and take the keyboard.
    pub fn start(&mut self, words: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.words.update(cx, |field, cx| field.set_text(words, cx));
        self.stock(cx);
        self.search(cx);
        TextField::focus(&self.words, window, cx);
    }

    /// Takes the index of each book of the vault whose index is missing or was made from
    /// another state of its file, one book at a time off the window's thread, and searches
    /// again as each lands.
    fn stock(&mut self, cx: &mut Context<Self>) {
        let Some(vault) = self.workspace.read(cx).vault.clone() else {
            self.shelf.clear();
            self.shelf_of = None;
            return;
        };
        let root = vault.root().to_path_buf();
        if self.shelf_of.as_ref() != Some(&root) {
            self.shelf.clear();
            self.shelf_of = Some(root.clone());
        }
        if self.stocking {
            return;
        }
        let books = book_index::shelved(&vault);
        self.shelf
            .retain(|id, (shelved, _)| books.iter().any(|b| b == shelved && b.id == *id));
        let wanted: Vec<(Shelved, Option<incipit_book::Source>)> = books
            .into_iter()
            .map(|book| {
                let known = self.shelf.get(&book.id).map(|(_, index)| index.source);
                (book, known)
            })
            .collect();
        self.stocking = true;
        cx.spawn(async move |this, cx| {
            for (book, known) in wanted {
                let vault = vault.clone();
                let taken = {
                    let book = book.clone();
                    cx.background_executor().spawn(async move {
                        // Unchanged since its index was taken: nothing to do.
                        if known.is_some() && book_index::source_of(&book.file) == known {
                            return None;
                        }
                        Some(book_index::index(
                            Some(&vault),
                            Some(&book.id),
                            &book.file,
                            book.pdf,
                            None,
                        ))
                    })
                };
                let taken = taken.await;
                let going = this.update(cx, |this, cx| {
                    if this.shelf_of.as_ref() != Some(&root) {
                        return false;
                    }
                    match taken {
                        Some(Ok(index)) => {
                            this.shelf.insert(book.id.clone(), (book, Arc::new(index)));
                            this.search(cx);
                        }
                        Some(Err(problem)) => eprintln!("{problem}"),
                        None => {}
                    }
                    true
                });
                if !going.unwrap_or(false) {
                    break;
                }
            }
            let _ = this.update(cx, |this, _| this.stocking = false);
        })
        .detach();
    }

    fn search(&mut self, cx: &mut Context<Self>) {
        self.asked += 1;
        let query = self.words.read(cx).text().trim().to_string();
        let docs = match (&self.workspace.read(cx).vault, query.is_empty()) {
            (Some(vault), false) => vault.search(&query, SHOWN),
            _ => Vec::new(),
        };
        self.rows = docs.iter().cloned().map(Row::Doc).collect();
        self.at = 0;
        cx.notify();
        let books: Vec<(DocId, String, Arc<TextIndex>)> = self
            .shelf
            .iter()
            .map(|(id, (book, index))| (id.clone(), book.title.clone(), index.clone()))
            .collect();
        if query.is_empty() || books.is_empty() {
            return;
        }
        let asked = self.asked;
        let passages = cx
            .background_executor()
            .spawn(async move { passages(&books, &query) });
        cx.spawn(async move |this, cx| {
            let passages = passages.await;
            let _ = this.update(cx, |this, cx| {
                if this.asked != asked {
                    return;
                }
                this.rows = merge(docs, passages, SHOWN);
                cx.notify();
            });
        })
        .detach();
    }

    fn pick(&mut self, index: usize, cx: &mut Context<Self>) {
        match self.rows.get(index) {
            Some(Row::Doc(hit)) => cx.emit(Found::Open(hit.kind, hit.id.clone())),
            Some(Row::Passage(passage)) => cx.emit(Found::Read(passage.book.clone(), passage.part)),
            None => {}
        }
    }

    fn key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let count = self.rows.len();
        if count == 0 {
            return;
        }
        match event.keystroke.key.as_str() {
            "down" => self.at = (self.at + 1) % count,
            "up" => self.at = (self.at + count - 1) % count,
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

/// The passages of the books' text that have every word of `query`, best first, at most
/// `SHOWN`: of equal rank, by the book's title, then in the book's order.
fn passages(books: &[(DocId, String, Arc<TextIndex>)], query: &str) -> Vec<Passage> {
    let mut found: Vec<Passage> = books
        .iter()
        .flat_map(|(book, title, index)| {
            index.search(query).into_iter().map(move |passage| Passage {
                book: book.clone(),
                title: if passage.title.is_empty() {
                    title.clone()
                } else {
                    format!("{title} · {}", passage.title)
                },
                snippet: passage.snippet,
                part: passage.part,
                score: book_index::score(&passage.whole),
            })
        })
        .collect();
    found.sort_by(|a, b| {
        b.score.cmp(&a.score).then_with(|| {
            a.title
                .split(" · ")
                .next()
                .cmp(&b.title.split(" · ").next())
        })
    });
    found.truncate(SHOWN);
    found
}

/// The documents and the passages as one list, best first, at most `limit`: each in its own
/// order, a document before a passage of the same rank.
fn merge(docs: Vec<Hit>, passages: Vec<Passage>, limit: usize) -> Vec<Row> {
    let mut docs = docs.into_iter().map(Row::Doc).peekable();
    let mut passages = passages.into_iter().map(Row::Passage).peekable();
    let mut rows = Vec::new();
    while rows.len() < limit {
        let row = match (docs.peek(), passages.peek()) {
            (Some(doc), Some(passage)) if passage.score() > doc.score() => passages.next(),
            (Some(_), _) => docs.next(),
            (None, Some(_)) => passages.next(),
            (None, None) => break,
        };
        rows.extend(row);
    }
    rows
}

/// What a kind of document is called beside a result.
pub fn kind_word(kind: Kind) -> &'static str {
    match kind {
        Kind::Note => "Note",
        Kind::Concept => "Concept",
        Kind::Card => "Card",
        Kind::Annotation => "In a book",
        Kind::Session => "Session",
        Kind::Figure => "Figure",
        Kind::Book => "Book",
    }
}

impl Render for FindView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        let rows = self.rows.iter().enumerate().map(|(index, row)| {
            let on = index == self.at;
            let (title, snippet, kind) = match row {
                Row::Doc(hit) => (hit.title.clone(), hit.snippet.clone(), kind_word(hit.kind)),
                Row::Passage(passage) => (
                    passage.title.clone(),
                    Some(passage.snippet.clone()),
                    "Book text",
                ),
            };
            div()
                .id(("find-hit", index))
                .px(px(10.0))
                .py(px(7.0))
                .rounded(px(8.0))
                .flex()
                .items_center()
                .gap(px(10.0))
                .cursor_pointer()
                .when(on, |row| row.bg(t.selected))
                .hover(|row| row.bg(t.selected))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().truncate().child(title))
                        .children(snippet.map(|line| {
                            div()
                                .truncate()
                                .text_size(px(Type::META))
                                .text_color(t.tertiary)
                                .child(line)
                        })),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(px(Type::META))
                        .text_color(t.tertiary)
                        .child(kind),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.pick(index, cx)))
        });
        let nothing = self.rows.is_empty() && !self.words.read(cx).text().trim().is_empty();
        let content = div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .on_key_down(cx.listener(|this, event, _, cx| this.key(event, cx)))
            .child(field(&self.words, t))
            .children(rows)
            .when(nothing, |content| {
                content.child(
                    div()
                        .px(px(10.0))
                        .text_color(t.tertiary)
                        .child("Nothing in the vault has those words."),
                )
            });
        sheet("find", 520.0, t, content).initial(TextField::handle(&self.words, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use incipit_vault::VaultPath;

    fn doc(title: &str, score: u32) -> Hit {
        Hit {
            id: DocId::from(title),
            title: title.into(),
            path: VaultPath::new(&format!("notes/{title}.md")).unwrap(),
            kind: Kind::Note,
            score,
            snippet: None,
        }
    }

    fn passage(title: &str, score: u32) -> Passage {
        Passage {
            book: DocId::from("book"),
            title: title.into(),
            snippet: String::new(),
            part: 0,
            score,
        }
    }

    fn titles(rows: &[Row]) -> Vec<&str> {
        rows.iter()
            .map(|row| match row {
                Row::Doc(hit) => hit.title.as_str(),
                Row::Passage(passage) => passage.title.as_str(),
            })
            .collect()
    }

    #[test]
    fn documents_and_passages_are_one_list_best_first_documents_first_of_a_rank() {
        let docs = vec![doc("Waves", 40), doc("Light", 3), doc("Notes", 2)];
        let passages = vec![passage("Optics · Light", 4), passage("Optics · Waves", 3)];
        let rows = merge(docs.clone(), passages.clone(), SHOWN);
        assert_eq!(
            titles(&rows),
            [
                "Waves",
                "Optics · Light",
                "Light",
                "Optics · Waves",
                "Notes"
            ]
        );
        assert_eq!(
            titles(&merge(docs, passages, 2)),
            ["Waves", "Optics · Light"]
        );
        assert_eq!(merge(Vec::new(), Vec::new(), SHOWN), []);
    }

    #[test]
    fn a_books_passages_are_named_by_the_book_and_their_line_of_the_contents() {
        let contents = vec![incipit_book::Entry {
            title: "Waves".into(),
            depth: 0,
            page: 1,
            y: None,
        }];
        let index = TextIndex::of_blocks(
            [(0, "Light before the contents."), (1, "Light as waves.")],
            contents,
            incipit_book::Source::default(),
        );
        let books = vec![(DocId::from("b1"), "Optics".to_string(), Arc::new(index))];
        let found = passages(&books, "light");
        let named: Vec<(&str, usize, u32)> = found
            .iter()
            .map(|p| (p.title.as_str(), p.part, p.score))
            .collect();
        assert_eq!(named, [("Optics", 0, 3), ("Optics · Waves", 1, 3)]);
        assert_eq!(passages(&books, "light waves").len(), 1);
        assert_eq!(passages(&books, "sound"), []);
    }
}
