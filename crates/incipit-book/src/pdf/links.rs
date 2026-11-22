//! The links on a PDF's pages: where a click on one falls, and where it leads (a page of the
//! same PDF and how far down it, or a page on the web). Only `http` and `https` addresses are
//! kept: a link to a file, a program or another PDF is not one the reader follows.

use pdfium_render::prelude::*;

use super::{down, Page};

/// A link on a page: the box a click on it falls in (x, y, width, height in points from the
/// page's top left) and where it leads.
#[derive(Clone, Debug, PartialEq)]
pub struct PageLink {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub to: LinkTo,
}

/// Where a link on a page leads.
#[derive(Clone, Debug, PartialEq)]
pub enum LinkTo {
    /// A page of the same PDF (from 0) and, when the link says, how far down it, as a share of
    /// its height from its top.
    Page { index: usize, y: Option<f32> },
    /// A page on the web, by its `http` or `https` address. Other addresses (files, programs,
    /// other PDFs) are not links the reader follows, and are left out.
    Web(String),
}

/// Where a destination in the document leads: its page and how far down it, as `contents`
/// reads an outline's line.
pub(super) fn destination_place(destination: &PdfDestination, pages: &PdfPages) -> Option<LinkTo> {
    let index = destination.page_index().ok()? as usize;
    let y = match destination.view_settings() {
        Ok(PdfDestinationViewSettings::SpecificCoordinatesAndZoom(_, Some(top), _)) => {
            let height = pages.page_size(index as _).ok()?.height().value;
            down(top.value, height)
        }
        _ => None,
    };
    Some(LinkTo::Page { index, y })
}

/// The links on a page `height` points high that lead somewhere the reader can go.
pub(super) fn links_of(page: &PdfPage, pages: &PdfPages, height: f32) -> Vec<PageLink> {
    page.links()
        .iter()
        .filter_map(|link| {
            let rect = link.rect().ok()?;
            let to = match link.destination() {
                Some(destination) => destination_place(&destination, pages)?,
                None => match link.action()? {
                    PdfAction::LocalDestination(action) => {
                        destination_place(&action.destination().ok()?, pages)?
                    }
                    PdfAction::Uri(action) => {
                        let address = action.uri().ok()?.trim().to_owned();
                        let lower = address.to_ascii_lowercase();
                        if !(lower.starts_with("http://") || lower.starts_with("https://")) {
                            return None;
                        }
                        LinkTo::Web(address)
                    }
                    _ => return None,
                },
            };
            let (left, top) = (rect.left().value, rect.top().value);
            Some(PageLink {
                x: left,
                y: height - top,
                w: rect.right().value - left,
                h: top - rect.bottom().value,
                to,
            })
        })
        .collect()
}

impl Page {
    /// The link at (x, y) in points, if there is one: the smallest whose box holds the point,
    /// for a link drawn inside another's box.
    pub fn link_at(&self, x: f32, y: f32) -> Option<&PageLink> {
        self.links
            .iter()
            .filter(|l| x >= l.x && x <= l.x + l.w && y >= l.y && y <= l.y + l.h)
            .min_by(|a, b| (a.w * a.h).total_cmp(&(b.w * b.h)))
    }
}
