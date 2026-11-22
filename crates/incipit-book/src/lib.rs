//! Books, read without a browser. A PDF is drawn by pdfium, which also says where each character
//! is on the page (`pdf`), and `dark` turns a drawn page dark for a dark app; an EPUB's HTML is turned into plain blocks of styled text a window lays
//! out itself (`epub`). `anchor` ties a note to a passage and finds the passage again;
//! `worker` draws a PDF's pages on a thread of its own; `find` makes a book's text ready to be
//! searched as the earlier app's find matched it, and `index` keeps one index of a book's words
//! for finding a phrase in it and searching across the books.

pub mod anchor;
pub mod dark;
pub mod epub;
pub mod find;
pub mod index;
pub mod pdf;
pub mod worker;

pub use anchor::Anchor;
pub use dark::Dark;
pub use epub::{Block, Book, Kind, Link, Rich, Span, Target};
pub use index::{Excerpt, Passage, Source, TextIndex};
pub use pdf::{library_file, Cut, Entry, Glyph, Line, LinkTo, Page, PageLink, PageText, Pdf};
pub use worker::{PdfInfo, PdfWorker};
