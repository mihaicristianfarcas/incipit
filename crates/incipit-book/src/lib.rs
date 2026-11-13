//! Books, read without a browser. A PDF is drawn by pdfium, which also says where each character
//! is on the page (`pdf`); an EPUB's HTML is turned into plain blocks of styled text a window lays
//! out itself (`epub`). `anchor` ties a note to a passage and finds the passage again;
//! `worker` draws a PDF's pages on a thread of its own; `find` finds every place a phrase
//! occurs in a book.

pub mod anchor;
pub mod epub;
pub mod find;
pub mod pdf;
pub mod worker;

pub use anchor::Anchor;
pub use epub::{Block, Book, Rich, Span};
pub use find::BookText;
pub use pdf::{Cut, Entry, Glyph, Line, Page, PageText, Pdf};
pub use worker::{PdfInfo, PdfWorker};
