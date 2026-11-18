//! The library: the vault's books, and adding one. A book added is copied into the vault, in a
//! folder of its own under `library/` with a record beside it, so the vault holds everything it
//! needs; choosing a book opens it in the reader. The books are listed by when the learner last
//! touched them, by title or by author, as the learner picks (the choice is kept in the app's own
//! folder), each with how far through it they are. INCIPIT_PAGE=library opens the page;
//! INCIPIT_OPEN=words opens the first book whose title has those words, without clicking.
//! Books given to the app from outside (`open_with`) are taken in here too, once a vault is open,
//! and their problems are said in the page's own line for problems. The books are one stop for Tab,
//! and the arrows go from one to the next.

use std::path::{Path, PathBuf};

use gpui::{
    div, prelude::*, px, Context, Entity, EventEmitter, IntoElement, MouseButton,
    PathPromptOptions, Render, ScrollHandle, Subscription, Window,
};
use incipit_ui::{
    button, card, focus_ring, keep_focus, meter, page, page_header, row_stops, segmented, Radius,
    Stop, Type,
};
use incipit_vault::{Book, Doc, DocId, Kind, Vault, VaultPath};

use crate::open_with::{take_in, Waiting};
use crate::progress::{states, words, BookState};
use crate::reader::Shelved;
use crate::workspace::{Happened, Workspace};

struct Listed {
    id: DocId,
    title: String,
    author: Option<String>,
    format: String,
    notes: usize,
    file: Option<PathBuf>,
    /// How far through, and since when it was last touched, from the activity log.
    state: BookState,
}

/// How the books are listed, as the earlier app's Library offered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Order {
    /// The book touched last first.
    #[default]
    Recent,
    Title,
    /// By author, books without one last, then by title.
    Author,
}

impl Order {
    const ALL: [Order; 3] = [Order::Recent, Order::Title, Order::Author];

    fn label(self) -> &'static str {
        match self {
            Order::Recent => "Recent",
            Order::Title => "Title",
            Order::Author => "Author",
        }
    }

    fn word(self) -> &'static str {
        match self {
            Order::Recent => "recent",
            Order::Title => "title",
            Order::Author => "author",
        }
    }

    /// The order kept in the app's folder; Recent when none is, or it can't be read.
    fn load(home: &Path) -> Order {
        let kept = std::fs::read_to_string(home.join(ORDER_FILE)).unwrap_or_default();
        Order::ALL
            .into_iter()
            .find(|o| o.word() == kept.trim())
            .unwrap_or_default()
    }

    /// Keeps the order for the next time; a failure only means it is not remembered.
    fn keep(self, home: &Path) {
        let _ = std::fs::create_dir_all(home);
        let _ = std::fs::write(home.join(ORDER_FILE), self.word());
    }
}

/// The file in the app's folder the Library's order is kept in.
const ORDER_FILE: &str = "library-order";

/// Sorts the books in `order`. Ties are broken by title, so the list never shuffles.
fn sort(books: &mut [Listed], order: Order) {
    let title = |b: &Listed| b.title.to_lowercase();
    match order {
        Order::Recent => books.sort_by_key(|b| (std::cmp::Reverse(b.state.touched), title(b))),
        Order::Title => books.sort_by_key(title),
        Order::Author => books.sort_by_key(|b| {
            let author = b.author.as_deref().map(str::trim).unwrap_or_default();
            (author.is_empty(), author.to_lowercase(), title(b))
        }),
    }
}

pub struct LibraryView {
    workspace: Entity<Workspace>,
    books: Vec<Listed>,
    order: Order,
    problem: Option<String>,
    /// A title to open as soon as it is listed (INCIPIT_OPEN).
    wanted: Option<String>,
    /// Books given to the app that are not taken in yet: no vault is open, or the ones before
    /// are still being taken in.
    waiting: Waiting,
    /// Books given to the app are being taken in, off the window's thread.
    taking: bool,
    scroll: ScrollHandle,
    _subscription: Subscription,
}

/// The book the learner chose to read.
pub struct Chosen(pub Shelved);

impl EventEmitter<Chosen> for LibraryView {}

/// Books given to the app from outside could not be added and none was opened: the Library says
/// why, so the window shows it.
pub struct Refused;

impl EventEmitter<Refused> for LibraryView {}

/// The kind of book a file is, from its name: `pdf` or `epub`.
fn format_of(file: &Path) -> Option<&'static str> {
    match file.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "pdf" => Some("pdf"),
        "epub" => Some("epub"),
        _ => None,
    }
}

/// A title or author as the file says it, cleaned as the earlier app's `ingestPdf` cleaned them:
/// Project Gutenberg's and Word's prefixes taken off, and the junk that print-to-PDF and
/// converters leave (`untitled`, `about:blank`, `Document1`, an address, a file name) dropped, as
/// the file's own name is better than these.
fn said_cleanly(value: Option<String>) -> Option<String> {
    let mut text = value?.trim().to_owned();
    let lower = text.to_lowercase();
    if let Some(rest) = lower.strip_prefix("the project gutenberg ebook #") {
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 && rest[digits..].starts_with(':') {
            let cut = "the project gutenberg ebook #".len() + digits + 1;
            text = text[cut..].trim_start().to_owned();
        }
    }
    if text.to_lowercase().starts_with("microsoft word - ") {
        text = text["microsoft word - ".len()..].to_owned();
    }
    let text = text.trim().to_owned();
    let lower = text.to_lowercase();
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    let file_name = lower.rsplit_once('.').is_some_and(|(stem, ext)| {
        !stem.is_empty()
            && stem.chars().all(word)
            && ["doc", "docx", "htm", "html", "tex", "pdf"].contains(&ext)
    });
    let document = lower
        .strip_prefix("document")
        .is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit()));
    let address = (lower.starts_with("http://") || lower.starts_with("https://"))
        && !lower.contains(char::is_whitespace);
    let junk = text.is_empty()
        || lower == "about:blank"
        || lower == "untitled"
        || document
        || address
        || file_name;
    (!junk).then_some(text)
}

/// Copy a book's file into the vault and write its record. Blocks while the file is copied.
pub fn add_book(vault: &Vault, file: &Path) -> Result<Doc, String> {
    let format = format_of(file).ok_or("Only PDF and EPUB books can be added for now.")?;
    let name = file
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("The file has no name that can be kept.")?;
    let stem = file.file_stem().and_then(|n| n.to_str()).unwrap_or("Book");
    // The title and author the book says, as the earlier app took them; the file's name when it
    // names no title worth keeping.
    let (title, author) = match format {
        "epub" => incipit_book::Book::open(file)
            .map(|b| (Some(b.title), b.author))
            .unwrap_or_default(),
        _ => incipit_book::Pdf::open(&crate::resources::pdfium(), file)
            .map(|pdf| pdf.said())
            .unwrap_or_default(),
    };
    let author = said_cleanly(author);
    let title = said_cleanly(title).unwrap_or_else(|| stem.replace(['_', '-'], " "));
    // A folder of the book's own, under a name not taken yet.
    let base = incipit_vault::kind::slug(&title, 60);
    let taken = |folder: &str| vault.root().join("library").join(folder).exists();
    let folder = (1..)
        .map(|n| {
            if n == 1 {
                base.clone()
            } else {
                format!("{base}-{n}")
            }
        })
        .find(|f| !taken(f))
        .expect("a free name exists");
    let record = VaultPath::new(&format!("library/{folder}/book.md")).map_err(|e| e.to_string())?;
    let into = vault.root().join("library").join(&folder);
    std::fs::create_dir_all(&into).map_err(|e| format!("The book's folder was not made: {e}"))?;
    std::fs::copy(file, into.join(name)).map_err(|e| format!("The book was not copied: {e}"))?;
    let mut doc = Doc::new_at(Kind::Book, record);
    doc.set_title(&title);
    doc.set_fields(&Book {
        author,
        file: Some(name.to_owned()),
        format: Some(format.to_owned()),
        ..Book::default()
    });
    let added = vault.add(doc).map_err(|e| e.to_string())?;
    crate::activity::log(
        vault,
        &crate::activity::book_added(crate::study::now(), added.id().as_str(), &title),
    );
    Ok(added)
}

/// Where a book's file is in the vault: beside its record, under the name the record holds. None
/// when the record names no file.
pub fn book_file(vault: &Vault, doc: &Doc, book: &Book) -> Option<PathBuf> {
    let (name, folder) = book.file.as_ref().zip(doc.path().parent())?;
    let path = folder.join(name).ok()?;
    Some(vault.root().join(path.as_str()))
}

impl LibraryView {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        let subscription =
            cx.subscribe(&workspace, |this: &mut Self, _, happened: &Happened, cx| {
                let books = match happened {
                    Happened::Opened => true,
                    Happened::Changed(event) => {
                        event.path.as_str().starts_with("library/")
                            || event.path.as_str() == "log/activity.jsonl"
                    }
                };
                if books {
                    this.list(cx);
                    cx.notify();
                }
                // A vault just opened takes in the books that were waiting for one.
                if matches!(happened, Happened::Opened) {
                    this.take_waiting(cx);
                }
            });
        let mut view = LibraryView {
            workspace,
            books: Vec::new(),
            order: Order::load(&crate::workspace::home()),
            problem: None,
            wanted: std::env::var("INCIPIT_OPEN").ok(),
            waiting: Waiting::default(),
            taking: false,
            scroll: ScrollHandle::new(),
            _subscription: subscription,
        };
        view.list(cx);
        view
    }

    fn list(&mut self, cx: &mut Context<Self>) {
        let Some(vault) = self.workspace.read(cx).vault.clone() else {
            self.books.clear();
            return;
        };
        let annotations = vault.list(Some(Kind::Annotation));
        let mut states = states(&vault, crate::study::now());
        self.books = vault
            .list(Some(Kind::Book))
            .into_iter()
            .map(|doc| {
                let book = doc.fields::<Book>().unwrap_or_default();
                let file = book_file(&vault, &doc, &book);
                let id = doc.id().clone();
                let state = states.remove(&id).unwrap_or(BookState {
                    progress: None,
                    touched: 0,
                    chapter: None,
                });
                let notes = annotations
                    .iter()
                    .filter(|a| a.source().is_some_and(|s| s.book == id.as_str()))
                    .count();
                Listed {
                    id,
                    title: doc.title(),
                    author: book.author,
                    format: book.format.unwrap_or_default(),
                    notes,
                    file,
                    state,
                }
            })
            .collect();
        sort(&mut self.books, self.order);
        let wanted = self
            .wanted
            .as_ref()
            .and_then(|words| self.books.iter().find(|b| b.title.contains(words.as_str())));
        if let Some(shelved) = wanted.and_then(|b| {
            Some(Shelved {
                id: Some(b.id.clone()),
                title: b.title.clone(),
                file: b.file.clone()?,
                epub: b.format == "epub",
            })
        }) {
            self.wanted = None;
            cx.emit(Chosen(shelved));
        }
    }

    /// Opens the book `id` in the reader; false when the vault has no file for it.
    pub fn open(&mut self, id: &DocId, cx: &mut Context<Self>) -> bool {
        let shelved = self.books.iter().find(|b| b.id == *id).and_then(|b| {
            Some(Shelved {
                id: Some(b.id.clone()),
                title: b.title.clone(),
                file: b.file.clone()?,
                epub: b.format == "epub",
            })
        });
        let found = shelved.is_some();
        if let Some(shelved) = shelved {
            cx.emit(Chosen(shelved));
        }
        found
    }

    /// Books given to the app from outside: each is added, or found when the same book is already
    /// here, and the first is opened in the reader. With no vault open they wait for one.
    pub fn give(&mut self, files: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.waiting.keep(files);
        self.take_waiting(cx);
    }

    /// Takes in the books waiting, off the window's thread, when a vault is open and no others
    /// are being taken in (two runs at once could give two books one folder). Books given while
    /// it works are taken in when it is done.
    fn take_waiting(&mut self, cx: &mut Context<Self>) {
        let vault = self.workspace.read(cx).vault.clone();
        let files = self.waiting.release(vault.is_some() && !self.taking);
        let Some(vault) = vault.filter(|_| !files.is_empty()) else {
            return;
        };
        self.taking = true;
        cx.spawn(async move |this, cx| {
            let taken = cx
                .background_executor()
                .spawn(async move { take_in(&vault, &files) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.taking = false;
                this.problem = (!taken.problems.is_empty()).then(|| taken.problems.join("\n"));
                this.list(cx);
                let opened = taken.open.is_some_and(|id| this.open(&id, cx));
                if !opened && this.problem.is_some() {
                    cx.emit(Refused);
                }
                cx.notify();
                this.take_waiting(cx);
            });
        })
        .detach();
    }

    /// Ask for book files and add each, off the window's thread.
    fn choose(&mut self, cx: &mut Context<Self>) {
        let Some(vault) = self.workspace.read(cx).vault.clone() else {
            return;
        };
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Add".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(files))) = chosen.await else {
                return;
            };
            let problems = cx
                .background_executor()
                .spawn(async move {
                    files
                        .iter()
                        .filter_map(|file| {
                            add_book(&vault, file)
                                .err()
                                .map(|e| format!("{}: {e}", file.display()))
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.problem = (!problems.is_empty()).then(|| problems.join("\n"));
                this.list(cx);
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for LibraryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        if self.workspace.read(cx).vault.is_none() {
            return div()
                .size_full()
                .child(crate::welcome::welcome(&self.workspace, t, cx))
                .into_any_element();
        }
        let add = button("book-add", "Add a book", t)
            .accent()
            .on_click(cx.listener(|this, _, _, cx| this.choose(cx)));
        // The books are one stop for Tab, the arrows going from one to the next.
        let stops = row_stops(
            "library-books",
            self.books.iter().map(|b| b.id.as_str()),
            Stop::Start(0),
            window,
            cx,
        );
        let rows = self.books.iter().enumerate().map(|(index, book)| {
            let (focus, ring) = stops.row(index, window);
            let shelved = book.file.clone().map(|file| Shelved {
                id: Some(book.id.clone()),
                title: book.title.clone(),
                file,
                epub: book.format == "epub",
            });
            let about = [
                book.author.clone(),
                Some(book.format.to_uppercase()).filter(|f| !f.is_empty()),
                match book.notes {
                    0 => None,
                    1 => Some("1 note".to_string()),
                    n => Some(format!("{n} notes")),
                },
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("  ·  ");
            card(t)
                .id(("book", index))
                .relative()
                .p(px(14.0))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .cursor_pointer()
                .hover(|c| c.bg(t.hover))
                .track_focus(&focus)
                .on_mouse_down(MouseButton::Left, keep_focus)
                .child(
                    div()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(book.title.clone()),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .text_size(px(Type::META))
                        .text_color(t.tertiary)
                        .child(meter(
                            book.state
                                .progress
                                .as_ref()
                                .map_or(0.0, |p| p.fraction as f32),
                            96.0,
                            t,
                        ))
                        .child(words(book.state.progress.as_ref(), false)),
                )
                .child(
                    div()
                        .text_size(px(Type::META))
                        .text_color(t.tertiary)
                        .child(about),
                )
                .on_click(cx.listener(move |_, _, _, cx| {
                    if let Some(shelved) = shelved.clone() {
                        cx.emit(Chosen(shelved));
                    }
                }))
                .when(ring, |card| {
                    card.child(focus_ring(t, Radius::ROW + 1.0, -2.0))
                })
        });
        let body =
            div()
                .flex()
                .flex_col()
                .gap(px(14.0))
                .child(page_header(
                    "Library",
                    None,
                    Some(add.into_any_element()),
                    t,
                ))
                .children(
                    self.problem
                        .clone()
                        .map(|p| div().text_color(t.danger).child(p)),
                )
                .when(!self.books.is_empty(), |body| {
                    let at = Order::ALL
                        .iter()
                        .position(|o| *o == self.order)
                        .unwrap_or(0);
                    let count = match self.books.len() {
                        1 => "1 book".to_string(),
                        n => format!("{n} books"),
                    };
                    body.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .text_size(px(Type::META))
                            .text_color(t.secondary)
                            .child("Order")
                            .child(
                                segmented("library-order", Order::ALL.map(Order::label), at, t)
                                    .on_select({
                                        let view = cx.entity().downgrade();
                                        move |i, _, cx| {
                                            let _ = view.update(cx, |this, cx| {
                                                this.order = Order::ALL[i];
                                                this.order.keep(&crate::workspace::home());
                                                sort(&mut this.books, this.order);
                                                cx.notify();
                                            });
                                        }
                                    }),
                            )
                            .child(div().flex_1())
                            .child(count),
                    )
                })
                .when(self.books.is_empty(), |body| {
                    body.child(div().py(px(30.0)).text_color(t.secondary).child(
                        "No books yet. Add a PDF or an EPUB and it is copied into your vault.",
                    ))
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .on_key_down(stops.keys())
                        .children(rows),
                );
        page("library", &self.scroll, 680.0, t, body).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_the_file_says_is_kept_unless_it_is_junk() {
        let said = |t: &str| said_cleanly(Some(t.to_owned()));
        assert_eq!(
            said(" Attention Is All You Need ").as_deref(),
            Some("Attention Is All You Need")
        );
        assert_eq!(
            said("The Project Gutenberg eBook #5001: Relativity").as_deref(),
            Some("Relativity")
        );
        assert_eq!(
            said("Microsoft Word - Thesis final").as_deref(),
            Some("Thesis final")
        );
        for junk in [
            "",
            "untitled",
            "Untitled",
            "about:blank",
            "Document1",
            "document",
            "https://example.com/a",
            "notes.docx",
            "paper_v2.pdf",
            "main.tex",
        ] {
            assert_eq!(said(junk), None, "{junk:?}");
        }
        // Words that only look like the junk are kept.
        assert_eq!(
            said("Documents of the Revolution").as_deref(),
            Some("Documents of the Revolution")
        );
        assert_eq!(
            said("A guide to HTML.html and more").as_deref(),
            Some("A guide to HTML.html and more")
        );
        assert_eq!(said_cleanly(None), None);
    }

    fn listed(title: &str, author: Option<&str>, touched: i64) -> Listed {
        Listed {
            id: DocId::from(title),
            title: title.into(),
            author: author.map(Into::into),
            format: "pdf".into(),
            notes: 0,
            file: None,
            state: BookState {
                progress: None,
                touched,
                chapter: None,
            },
        }
    }

    fn titles(books: &[Listed]) -> Vec<&str> {
        books.iter().map(|b| b.title.as_str()).collect()
    }

    #[test]
    fn books_are_listed_by_last_touched_title_or_author() {
        let mut books = vec![
            listed("optics", Some("Feynman"), 5),
            listed("Calculus", None, 9),
            listed("Analysis", Some("abbott"), 5),
            listed("Biology", Some("Feynman"), 1),
        ];
        sort(&mut books, Order::Recent);
        assert_eq!(
            titles(&books),
            ["Calculus", "Analysis", "optics", "Biology"]
        );
        sort(&mut books, Order::Title);
        assert_eq!(
            titles(&books),
            ["Analysis", "Biology", "Calculus", "optics"]
        );
        // Authors without regard to case, books without one last, each author's by title.
        sort(&mut books, Order::Author);
        assert_eq!(
            titles(&books),
            ["Analysis", "Biology", "optics", "Calculus"]
        );
    }

    #[test]
    fn the_order_is_kept_and_an_unknown_one_is_recent() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(Order::load(home.path()), Order::Recent);
        Order::Author.keep(home.path());
        assert_eq!(Order::load(home.path()), Order::Author);
        std::fs::write(home.path().join(ORDER_FILE), "shelf").unwrap();
        assert_eq!(Order::load(home.path()), Order::Recent);
    }

    #[test]
    fn a_book_added_is_copied_into_the_vault_with_its_record() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(&folder.path().join("vault")).unwrap();
        let file = folder.path().join("Some_Paper.pdf");
        std::fs::write(&file, b"%PDF-1.4").unwrap();
        let doc = add_book(&vault, &file).unwrap();
        assert_eq!(doc.title(), "Some Paper");
        assert_eq!(doc.path().as_str(), "library/some-paper/book.md");
        assert!(vault
            .root()
            .join("library/some-paper/Some_Paper.pdf")
            .exists());
        assert_eq!(doc.fields::<Book>().unwrap().format.as_deref(), Some("pdf"));
        // It is in the activity log, as the earlier app's journey had it.
        let log = vault.activity().unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].text, "Added Some Paper");
        assert_eq!(log[0].book.as_deref(), Some(doc.id().as_str()));
        // The same file again gets a folder of its own.
        let again = add_book(&vault, &file).unwrap();
        assert_eq!(again.path().as_str(), "library/some-paper-2/book.md");
    }

    #[test]
    fn a_file_that_is_not_a_book_is_refused() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(&folder.path().join("vault")).unwrap();
        let file = folder.path().join("notes.txt");
        std::fs::write(&file, b"hello").unwrap();
        assert!(add_book(&vault, &file).is_err());
    }
}
