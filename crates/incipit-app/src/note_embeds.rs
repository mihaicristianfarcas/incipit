//! What a note shows in place of its embeds, under its text: the pictures of `attachments/`
//! (`![[picture.png]]`, `![[picture.png|300]]`, `![alt](attachments/picture.png)`) and the
//! figures (`![[figure:<id>]]`). The note is written as Markdown in a field, so these are
//! listed below it, in the order the note embeds them, each once.
//!
//! A picture is read from the vault off the window's thread and kept for as long as the vault
//! is open; the window draws it from there. A figure is drawn still, at its first moment, by
//! the figure engine (`incipit-scene`) with this window's text widths, so it never needs a
//! model; it is not played here (the Figure page plays a kept figure). A picture that cannot be
//! read, or a figure whose spec the engine does not accept, is named and not drawn.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{div, img, prelude::*, px, AnyElement, Image, Window};
use incipit_scene::{layout, Figure};
use incipit_ui::{Theme, Type};
use incipit_vault::{Doc, DocId, Embed, Vault, VaultPath};

use crate::reader::rows::format_of;
use crate::stage::{stage, Shaped};

/// The widest a picture or a figure is drawn: the note's own column.
const WIDTH: f32 = 680.0;

/// The pictures read so far, by their place in the vault.
#[derive(Default)]
pub struct Embeds {
    pub items: Vec<Embed>,
    pictures: HashMap<VaultPath, Arc<Image>>,
    /// Pictures being read, so one is not read twice at once.
    reading: Vec<VaultPath>,
}

/// The figure a figure document holds, if the engine reads its spec.
pub fn figure_of(doc: &Doc) -> Option<Figure> {
    let spec = doc.fields::<incipit_vault::Figure>()?.spec?;
    serde_json::from_value(spec).ok()
}

impl Embeds {
    /// Forgets everything: another vault opened.
    pub fn clear(&mut self) {
        *self = Embeds::default();
    }

    /// What the note embeds now, and the pictures among them not yet read, to read.
    pub fn read(&mut self, vault: &Vault, note: Option<&DocId>) -> Vec<VaultPath> {
        self.items = note.map(|id| vault.embeds(id)).unwrap_or_default();
        let wanted: Vec<VaultPath> = self
            .items
            .iter()
            .filter_map(|item| match item {
                Embed::Picture { path, .. } => Some(path.clone()),
                Embed::Figure(_) => None,
            })
            .filter(|path| !self.pictures.contains_key(path) && !self.reading.contains(path))
            .collect();
        self.reading.extend(wanted.iter().cloned());
        wanted
    }

    /// A picture's bytes, read: kept for drawing, when its name says what kind it is.
    pub fn keep(&mut self, path: &VaultPath, bytes: Option<Vec<u8>>) {
        self.reading.retain(|p| p != path);
        if let (Some(format), Some(bytes)) = (format_of(path.name()), bytes) {
            self.pictures
                .insert(path.clone(), Arc::new(Image::from_bytes(format, bytes)));
        }
    }

    /// Whether the note embeds anything to draw.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The embeds, one under another.
    pub fn view(&self, vault: &Vault, t: Theme, window: &Window) -> AnyElement {
        let measure = Shaped(window.text_system().clone());
        let caption = |text: String| {
            div()
                .text_size(px(Type::META))
                .text_color(t.tertiary)
                .child(text)
        };
        let items = self.items.iter().enumerate().map(|(ix, item)| match item {
            Embed::Picture { path, width, alt } => {
                let said = alt.clone().unwrap_or_else(|| path.name().to_owned());
                let drawn = match self.pictures.get(path) {
                    Some(image) => {
                        let picture = img(image.clone()).max_w(px(WIDTH));
                        match width {
                            Some(w) => picture.w(px((*w as f32).min(WIDTH))),
                            None => picture,
                        }
                        .into_any_element()
                    }
                    None if self.reading.contains(path) => div().into_any_element(),
                    None => div()
                        .text_color(t.tertiary)
                        .child(format!("{} could not be shown.", path.name()))
                        .into_any_element(),
                };
                div()
                    .id(("note-embed", ix))
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(drawn)
                    .child(caption(format!("Picture: {said}")))
                    .into_any_element()
            }
            Embed::Figure(id) => {
                let doc = vault.get(id);
                let title = doc.as_ref().map(|d| d.title()).unwrap_or_default();
                let figure = doc.as_ref().and_then(figure_of);
                let drawn = match figure {
                    Some(figure) => {
                        let frame = layout(&figure, &figure.start(), &measure);
                        stage(frame, WIDTH, t, Rc::new(Cell::new(gpui::Point::default())))
                            .into_any_element()
                    }
                    None => div()
                        .text_color(t.tertiary)
                        .child(format!("The figure {title} could not be drawn."))
                        .into_any_element(),
                };
                div()
                    .id(("note-embed", ix))
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(drawn)
                    .child(caption(format!("Figure: {title}")))
                    .into_any_element()
            }
        });
        div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(
                div()
                    .text_size(px(Type::META))
                    .text_color(t.secondary)
                    .child("Pictures and figures in this note"),
            )
            .children(items)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use incipit_vault::Kind;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3];

    fn vault_with_figure() -> (tempfile::TempDir, Vault, Doc) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(&dir.path().join("vault")).unwrap();
        let mut figure = Doc::new(Kind::Figure);
        figure.set_title("A sequence");
        figure.set_fields(&incipit_vault::Figure {
            spec: Some(
                serde_json::to_value(incipit_scene::schema::example(incipit_scene::Kind::ALL[0]))
                    .unwrap(),
            ),
            ..incipit_vault::Figure::default()
        });
        let figure = vault.add(figure).unwrap();
        (dir, vault, figure)
    }

    #[test]
    fn a_kept_figure_is_read_by_the_engine_and_a_broken_spec_is_not() {
        let (_dir, vault, figure) = vault_with_figure();
        assert!(figure_of(&figure).is_some());
        let mut broken = figure.clone();
        broken.set_fields(&incipit_vault::Figure {
            spec: Some(serde_json::json!({"kind": "nonsense"})),
            ..incipit_vault::Figure::default()
        });
        assert!(figure_of(&broken).is_none());
        drop(vault);
    }

    #[test]
    fn each_picture_is_asked_for_once_until_it_is_read() {
        let (_dir, vault, figure) = vault_with_figure();
        vault.add_picture(PNG, Some("p.png")).unwrap();
        let mut note = Doc::new(Kind::Note);
        note.set_title("Waves");
        note.set_body(format!("![[p.png]] ![[figure:{}]]\n", figure.id()));
        let note = vault.add(note).unwrap();
        let mut embeds = Embeds::default();
        let wanted = embeds.read(&vault, Some(note.id()));
        assert_eq!(wanted.len(), 1);
        assert_eq!(embeds.items.len(), 2);
        // Asked again while being read: not asked twice.
        assert!(embeds.read(&vault, Some(note.id())).is_empty());
        embeds.keep(&wanted[0], vault.picture(&wanted[0]).ok());
        assert!(embeds.read(&vault, Some(note.id())).is_empty());
        assert!(embeds.pictures.contains_key(&wanted[0]));
        // A note that is no longer open embeds nothing.
        embeds.read(&vault, None);
        assert!(embeds.is_empty());
    }
}
