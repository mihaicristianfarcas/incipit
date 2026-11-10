//! Books, read without a browser. A PDF is drawn by pdfium, which also says where each character
//! is on the page (`pdf`); an EPUB's HTML is turned into plain blocks of styled text a window lays
//! out itself (`epub`). `anchor` ties a note to a passage and finds the passage again.

pub mod anchor;
pub mod epub;
pub mod pdf;

pub use anchor::Anchor;
pub use epub::{Block, Book, Rich, Span};
pub use pdf::{Glyph, Page, Pdf};
