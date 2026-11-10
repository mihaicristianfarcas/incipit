//! A PDF on a thread of its own. pdfium is slow enough to stall a window (a page takes a fifth of a
//! second in a debug build) and must be used from one thread at a time, so one named thread owns
//! the document: it is asked for pages over a channel and answers each through the callback that
//! came with the request. When several requests wait, the newest is drawn first: it is the page
//! on screen now.

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

struct Request {
    index: usize,
    pixel_width: u32,
    reply: Reply,
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
                        (request.reply)(pdf.page(request.index, request.pixel_width));
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
        let _ = self.requests.send(Request {
            index,
            pixel_width,
            reply: Box::new(reply),
        });
    }
}
