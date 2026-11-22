//! Opening a book: its file read off the window's thread (a PDF through its own pdfium thread,
//! an EPUB parsed whole, its pictures decoded once), then shown at the place it was left, with
//! its notes and concepts found again in it, and what was to happen once it was open (a page to
//! go to, a quiz, a chapter to map) done. A PDF's pages arrive as they are drawn (`take_pages`):
//! each is held with its text, settles the notes kept on it, and counts toward telling a PDF
//! with no text. Closing the book lets go of everything that was about it.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use futures::channel::mpsc::UnboundedReceiver;
use futures::StreamExt;
use gpui::{px, Context, ListOffset};
use incipit_book::{Block, Book, Entry, PdfInfo, PdfWorker};

use super::model::{rich, Doc, EpubDoc, Held, Opened, PdfDoc};
use super::{rows, Drawn, ReaderView, State, NO_TEXT};
use crate::learning::quiz::Mode;

impl ReaderView {
    /// Pages arrive from the PDF's thread for as long as the view lives.
    pub(super) async fn take_pages(
        this: gpui::WeakEntity<Self>,
        mut pages: UnboundedReceiver<Drawn>,
        cx: &mut gpui::AsyncApp,
    ) {
        while let Some((index, result)) = pages.next().await {
            let alive = this.update(cx, |this, cx| {
                let State::Open(Opened {
                    doc: Doc::Pdf(pdf), ..
                }) = &mut this.state
                else {
                    return;
                };
                pdf.asked.remove(&index);
                if let Ok((page, image, dark)) = result {
                    let text = page.text();
                    if pdf.count_text(&text) {
                        this.said = Some(NO_TEXT.to_owned());
                    }
                    pdf.pages.insert(
                        index,
                        Held {
                            page,
                            image,
                            text,
                            dark,
                            origin: Default::default(),
                        },
                    );
                    pdf.trim(this.list.logical_scroll_top().item_ix);
                    this.settle_part(index);
                }
                if this.demo && index == 0 {
                    this.demo = false;
                    this.show_demo();
                    this.try_demo(cx);
                }
                cx.notify();
            });
            if alive.is_err() {
                break;
            }
        }
    }

    /// Closes the book: the reader shows what it shows with no book, and what was to happen
    /// once the book opened (a page to go to, a quiz) is dropped with it.
    pub(super) fn close_book(&mut self, cx: &mut Context<Self>) {
        self.book = None;
        self.go_to = None;
        self.quiz_chapter = None;
        self.open = None;
        self.redrawing_for = None;
        // With no book to open, this lets go of the book's notes, answers and session.
        self.open_book(cx);
        cx.notify();
    }

    /// Open the sample book of the kind chosen, off the window's thread.
    pub(super) fn open_book(&mut self, cx: &mut Context<Self>) {
        self.state = State::Opening;
        self.selection = None;
        self.menu = None;
        self.notes.clear();
        self.said = None;
        // Answers being written for the book that was open stay pending in the vault; they are
        // written again when it is opened.
        for (_, cancel) in self.answers.drain() {
            cancel.cancel();
        }
        self.composing = None;
        self.finding.reset();
        self.close_session(cx);
        // The conversation was about the book that was open: it stays kept in the vault.
        self.new_chat(cx);
        self.scale = self.first_scale();
        self.back.clear();
        self.marked = None;
        self.marked_at = None;
        self.asking_page = false;
        let epub = self.epub;
        let Some(book) = self.book.clone() else {
            self.state = State::Closed;
            return;
        };
        let (file, title) = (book.file.clone(), book.title.clone());
        cx.spawn(async move |this, cx| {
            let opened = cx
                .background_executor()
                .spawn(async move {
                    if epub {
                        open_epub(&file).map(Raw::Epub)
                    } else {
                        open_pdf(file).map(Raw::Pdf)
                    }
                })
                .await
                .map(|raw| raw.opened(title));
            let _ = this.update(cx, |this, cx| {
                if this.book.as_ref().map(|b| &b.file) != Some(&book.file) {
                    return;
                }
                match opened {
                    Ok(opened) => {
                        this.list.reset(opened.parts());
                        // A passage asked for comes first, then INCIPIT_PART, and else the
                        // book opens where it was left.
                        let part = this.go_to.take().or_else(|| {
                            std::env::var("INCIPIT_PART")
                                .ok()
                                .and_then(|p| p.parse().ok())
                        });
                        let left = match part {
                            Some(item_ix) => Some((item_ix, 0.0)),
                            None => this.left_at(!epub),
                        };
                        if let Some((item_ix, fraction)) = left {
                            this.list.scroll_to(ListOffset {
                                item_ix,
                                offset_in_item: px(0.0),
                            });
                            this.reading.fraction = (!epub && fraction > 0.0).then_some(fraction);
                        }
                        let parts = opened.parts();
                        this.state = State::Open(opened);
                        this.focus_book = true;
                        this.note_length(parts, cx);
                        this.report(cx);
                        this.kept_notes(cx);
                        this.concept_notes(cx);
                        this.finding.book_opened();
                        if let Some(entry) = this.map_on_open.take() {
                            this.contents = true;
                            this.map_chapter(entry, cx);
                        }
                        if let Some(chapter) = this.quiz_chapter.take() {
                            this.quiz_on(chapter, cx);
                        }
                        match this.quiz_on_open.take().as_deref() {
                            Some("check") => this.offer_quiz(Some(Mode::Check), cx),
                            Some("pretest") => this.offer_quiz(Some(Mode::Pretest), cx),
                            Some(_) => this.offer_quiz(None, cx),
                            None => {}
                        }
                        this.earlier_chats(cx);
                        match std::env::var("INCIPIT_ASK_BOOK").as_deref() {
                            Ok("demo") => this.chat_demo(),
                            Ok("1") | Err(_) => {}
                            Ok(question) => this.ask_book(question.to_owned(), cx),
                        }
                        if this.demo && epub {
                            this.demo = false;
                            this.show_demo();
                            this.try_demo(cx);
                        }
                    }
                    Err(e) => this.state = State::Failed(e),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

/// A book as the background thread hands it over. What the window adds (where each page was
/// painted) is not something a thread can pass on, so the PDF's document is made here.
enum Raw {
    Pdf((PdfWorker, PdfInfo)),
    Epub((String, Vec<Entry>, EpubDoc)),
}

impl Raw {
    fn opened(self, title: String) -> Opened {
        match self {
            Raw::Epub((_, contents, book)) => Opened {
                title,
                contents,
                doc: Doc::Epub(book),
            },
            Raw::Pdf((worker, info)) => Opened {
                title,
                contents: info.contents,
                doc: Doc::Pdf(PdfDoc {
                    worker,
                    sizes: info.sizes,
                    pages: HashMap::new(),
                    asked: HashSet::new(),
                    textless: Some(0),
                }),
            },
        }
    }
}

fn open_pdf(file: PathBuf) -> Result<(PdfWorker, PdfInfo), String> {
    PdfWorker::open(crate::resources::pdfium(), file).map_err(|e| format!("{e:#}"))
}

fn open_epub(file: &std::path::Path) -> Result<(String, Vec<Entry>, EpubDoc), String> {
    let book = Book::open(file).map_err(|e| format!("{e:#}"))?;
    let contents = book.contents;
    let mut pictures = HashMap::new();
    let blocks: Vec<_> = book
        .blocks
        .into_iter()
        .enumerate()
        .map(|(index, block)| {
            if let Block::Image { name, bytes } = &block {
                if let Some(format) = rows::format_of(name) {
                    pictures.insert(
                        index,
                        Arc::new(gpui::Image::from_bytes(format, bytes.clone())),
                    );
                }
            }
            let chars = rich(&block)
                .map(|r| r.text.chars().collect())
                .unwrap_or_default();
            (block, chars)
        })
        .collect();
    Ok((book.title, contents, EpubDoc { blocks, pictures }))
}
