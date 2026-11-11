//! The library: the vault's books, and adding one. A book added is copied into the vault, in a
//! folder of its own under `library/` with a record beside it, so the vault holds everything it
//! needs; choosing a book opens it in the reader. INCIPIT_PAGE=library opens the page;
//! INCIPIT_OPEN=words opens the first book whose title has those words, without clicking.

use std::path::{Path, PathBuf};

use gpui::{
    div, prelude::*, px, Context, Entity, EventEmitter, IntoElement, PathPromptOptions, Render,
    ScrollHandle, Subscription, Window,
};
use incipit_ui::{button, card, page, page_header, Type};
use incipit_vault::{Book, Doc, DocId, Kind, Vault, VaultPath};

use crate::reader::Shelved;
use crate::workspace::{Happened, Workspace};

struct Listed {
    id: DocId,
    title: String,
    author: Option<String>,
    format: String,
    notes: usize,
    file: Option<PathBuf>,
}

pub struct LibraryView {
    workspace: Entity<Workspace>,
    books: Vec<Listed>,
    problem: Option<String>,
    /// A title to open as soon as it is listed (INCIPIT_OPEN).
    wanted: Option<String>,
    scroll: ScrollHandle,
    _subscription: Subscription,
}

/// The book the learner chose to read.
pub struct Chosen(pub Shelved);

impl EventEmitter<Chosen> for LibraryView {}

/// The kind of book a file is, from its name: `pdf` or `epub`.
fn format_of(file: &Path) -> Option<&'static str> {
    match file.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "pdf" => Some("pdf"),
        "epub" => Some("epub"),
        _ => None,
    }
}

/// Copy a book's file into the vault and write its record. Blocks while the file is copied.
pub fn add_book(vault: &Vault, file: &Path) -> Result<Doc, String> {
    let format = format_of(file).ok_or("Only PDF and EPUB books can be added for now.")?;
    let name = file
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("The file has no name that can be kept.")?;
    let stem = file.file_stem().and_then(|n| n.to_str()).unwrap_or("Book");
    let title = match format {
        "epub" => incipit_book::Book::open(file)
            .ok()
            .map(|b| b.title)
            .filter(|t| !t.trim().is_empty()),
        _ => None,
    }
    .unwrap_or_else(|| stem.replace(['_', '-'], " "));
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
        file: Some(name.to_owned()),
        format: Some(format.to_owned()),
        ..Book::default()
    });
    vault.add(doc).map_err(|e| e.to_string())
}

impl LibraryView {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        let subscription =
            cx.subscribe(&workspace, |this: &mut Self, _, happened: &Happened, cx| {
                let books = match happened {
                    Happened::Opened => true,
                    Happened::Changed(event) => event.path.as_str().starts_with("library/"),
                };
                if books {
                    this.list(cx);
                    cx.notify();
                }
            });
        let mut view = LibraryView {
            workspace,
            books: Vec::new(),
            problem: None,
            wanted: std::env::var("INCIPIT_OPEN").ok(),
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
        self.books = vault
            .list(Some(Kind::Book))
            .into_iter()
            .map(|doc| {
                let book = doc.fields::<Book>().unwrap_or_default();
                let folder = doc.path().parent();
                let file = book.file.as_ref().zip(folder).and_then(|(name, folder)| {
                    let path = folder.join(name).ok()?;
                    Some(vault.root().join(path.as_str()))
                });
                let id = doc.id().clone();
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
                }
            })
            .collect();
        self.books.sort_by_key(|b| b.title.to_lowercase());
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
        let rows = self.books.iter().enumerate().map(|(index, book)| {
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
                .p(px(14.0))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .cursor_pointer()
                .hover(|c| c.bg(t.hover))
                .child(
                    div()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(book.title.clone()),
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
                .when(self.books.is_empty(), |body| {
                    body.child(div().py(px(30.0)).text_color(t.secondary).child(
                        "No books yet. Add a PDF or an EPUB and it is copied into your vault.",
                    ))
                })
                .child(div().flex().flex_col().gap(px(8.0)).children(rows));
        page("library", &self.scroll, 680.0, t, body).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
