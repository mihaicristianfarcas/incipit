//! A PDF on a thread of its own. pdfium is slow enough to stall a window (a page takes a fifth of a
//! second in a debug build) and must be used from one thread at a time, so one named thread owns
//! the document: it is asked for pages, or for the text of pages without drawing them, over a
//! channel, and answers each through the callback that came with the request. When several
//! requests wait, the newest is answered first: it is the page on screen now.
//!
//! The test reads the sample paper `tools/setup` downloads, with the pdfium it puts in `lib/`:
//! `cargo test -p incipit-book -- --ignored`.

use std::ops::Range;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};

use anyhow::{anyhow, Result};

use crate::pdf::{Entry, Page, Pdf};

/// What is known of a PDF as soon as it is open.
#[derive(Clone, Debug)]
pub struct PdfInfo {
    /// Each page's width and height in points.
    pub sizes: Vec<(f32, f32)>,
    pub contents: Vec<Entry>,
}

type Reply = Box<dyn FnOnce(Result<Page>) + Send>;
type TextReply = Box<dyn FnOnce(Result<Vec<String>>) + Send>;

enum Request {
    /// A page, drawn.
    Page {
        index: usize,
        pixel_width: u32,
        reply: Reply,
    },
    /// The text of pages, none drawn.
    Text {
        pages: Range<usize>,
        reply: TextReply,
    },
}

/// The handle to the thread. Dropping it ends the thread once it has answered what it was asked.
pub struct PdfWorker {
    requests: Sender<Request>,
}

impl PdfWorker {
    /// Open `file` with the pdfium library in the folder `library`. Blocks until the document is
    /// open (or was refused), so call it off the window's thread.
    pub fn open(library: PathBuf, file: PathBuf) -> Result<(PdfWorker, PdfInfo)> {
        let (requests, waiting) = channel::<Request>();
        let (opened, open) = channel::<Result<PdfInfo>>();
        std::thread::Builder::new()
            .name("pdf".into())
            .spawn(move || {
                let pdf = match Pdf::open(&library, &file) {
                    Ok(pdf) => pdf,
                    Err(e) => {
                        let _ = opened.send(Err(e));
                        return;
                    }
                };
                let info = pdf.sizes().map(|sizes| PdfInfo {
                    sizes,
                    contents: pdf.contents(),
                });
                let failed = info.is_err();
                let _ = opened.send(info);
                if failed {
                    return;
                }
                // Blocks on the channel; ends when the handle is dropped.
                while let Ok(first) = waiting.recv() {
                    let mut pending = vec![first];
                    pending.extend(waiting.try_iter());
                    while let Some(request) = pending.pop() {
                        match request {
                            Request::Page {
                                index,
                                pixel_width,
                                reply,
                            } => reply(pdf.page(index, pixel_width)),
                            Request::Text { pages, reply } => {
                                reply(pages.map(|index| pdf.text(index)).collect())
                            }
                        }
                        pending.extend(waiting.try_iter());
                    }
                }
            })
            .map_err(|e| anyhow!("the PDF's thread did not start: {e}"))?;
        let info = open
            .recv()
            .map_err(|_| anyhow!("the PDF's thread ended before it opened"))??;
        Ok((PdfWorker { requests }, info))
    }

    /// Ask for page `index` drawn `pixel_width` pixels wide. `reply` runs on the PDF's thread.
    pub fn page(
        &self,
        index: usize,
        pixel_width: u32,
        reply: impl FnOnce(Result<Page>) + Send + 'static,
    ) {
        let _ = self.requests.send(Request::Page {
            index,
            pixel_width,
            reply: Box::new(reply),
        });
    }

    /// Ask for the text of `pages`, each page's as `Page::text` gives it, without drawing them.
    /// `reply` runs on the PDF's thread.
    pub fn text(
        &self,
        pages: Range<usize>,
        reply: impl FnOnce(Result<Vec<String>>) + Send + 'static,
    ) {
        let _ = self.requests.send(Request::Text {
            pages,
            reply: Box::new(reply),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
    }

    #[test]
    #[ignore = "requires pdfium in lib/ and the sample paper from tools/setup"]
    fn the_text_of_pages_is_the_text_of_the_pages_drawn() {
        let (worker, info) =
            PdfWorker::open(root().join("lib/lib"), root().join("samples/paper.pdf")).unwrap();
        assert!(info.sizes.len() > 3);
        let (send, receive) = channel();
        worker.text(1..3, move |texts| {
            let _ = send.send(texts);
        });
        let texts = receive.recv().unwrap().unwrap();
        assert_eq!(texts.len(), 2);
        let (send, receive) = channel();
        worker.page(2, 200, move |page| {
            let _ = send.send(page);
        });
        let drawn = receive.recv().unwrap().unwrap();
        assert_eq!(texts[1], drawn.text().into_iter().collect::<String>());
        assert!(texts[0].contains("attention"));
    }
}
