//! Pictures in notes: adding one to `attachments/`, reading one back, and finding the picture a
//! link names. A picture is saved whole under a free name (`Pasted 2026-11-16 12.30.00.png`, or
//! the name it had) and announced, and history keeps it with the notes. Only pictures are ever
//! read from the folder, and only from that folder (`crate::attachments`). `embeds` lists what a
//! note shows in place: the pictures and figures it embeds.

use std::fs;

use crate::attachments::{picture_type, sniff, ATTACHMENTS, MAX_PICTURE_BYTES};
use crate::error::{Error, Result};
use crate::events::Change;
use crate::fsio::{self, stat_fingerprint};
use crate::kind::file_name_of;
use crate::path::VaultPath;

use super::{here, Vault};
use crate::doc::DocId;
use crate::kind::Kind;

/// Something a note shows in place of a link: `![[picture.png]]` or `![[figure:<id>]]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Embed {
    Picture {
        path: VaultPath,
        /// The width the note asked for (`![[picture.png|300]]`), in points.
        width: Option<u32>,
        /// What the picture shows, in the words the note gave (`![what it shows](…)`).
        alt: Option<String>,
    },
    Figure(DocId),
}

/// The longest a picture's name is, without its extension.
const NAME_CHARS: usize = 90;

/// How many numbered names (" 2", " 3"…) are tried before a name counts as taken.
const MAX_NUMBERED: u32 = 999;

/// The folder as a place.
fn folder() -> VaultPath {
    VaultPath::new(ATTACHMENTS).expect("the attachments folder is a plain name")
}

impl Vault {
    /// Saves a picture in `attachments/` and gives its place. `name` is what it was called
    /// (a dropped file's name; none for a pasted picture, which is named by the time). The
    /// bytes must be a PNG, JPEG, GIF or WebP picture of at most 12 MB, whatever the name
    /// says, and take that kind's extension.
    pub fn add_picture(&self, bytes: &[u8], name: Option<&str>) -> Result<VaultPath> {
        if bytes.len() > MAX_PICTURE_BYTES {
            return Err(Error::Name(
                "That picture is larger than 12 MB, so it was not added.".to_owned(),
            ));
        }
        let extension = sniff(bytes).ok_or_else(|| {
            Error::Name("Only PNG, JPEG, GIF and WebP pictures can be added.".to_owned())
        })?;
        let wanted = match name.map(str::trim).filter(|n| !n.is_empty()) {
            Some(name) => {
                // A dropped file's own extension is not part of the name it is given.
                let stem = match name.rsplit_once('.') {
                    Some((stem, ext)) if picture_type(&format!("x.{ext}")).is_some() => stem,
                    _ => name,
                };
                file_name_of(stem)
            }
            None => format!("Pasted {}", chrono::Utc::now().format("%Y-%m-%d %H.%M.%S")),
        };
        let base: String = wanted.chars().take(NAME_CHARS).collect();
        let inner = &self.inner;
        let mut state = inner.lock();
        let attachments = folder();
        let mut place = attachments.join(&format!("{base}.{extension}"))?;
        let mut n = 2;
        while place.on_disk(&inner.root).exists() {
            if n > MAX_NUMBERED {
                return Err(Error::Exists(place.to_string()));
            }
            place = attachments.join(&format!("{base} {n}.{extension}"))?;
            n += 1;
        }
        let full = inner.locate(&place)?;
        fsio::write_atomic(&full, bytes)?;
        if let Some(print) = stat_fingerprint(&full) {
            state.files.insert(place.clone(), print);
        }
        self.announce(vec![here(&place, Change::Created, None)]);
        Ok(place)
    }

    /// The pictures and figures a note embeds, in the order it does, each once. A link that
    /// reaches nothing, or something else, is left out.
    pub fn embeds(&self, id: &DocId) -> Vec<Embed> {
        let mut seen = Vec::new();
        for outlink in self.outlinks(id).into_iter().filter(|o| o.link.embed) {
            let embed = match (&outlink.picture, &outlink.to) {
                (Some(path), _) => {
                    let label = outlink.link.label.clone();
                    let width = label.as_deref().and_then(|l| l.parse::<u32>().ok());
                    Embed::Picture {
                        path: path.clone(),
                        width,
                        alt: label.filter(|_| width.is_none()),
                    }
                }
                (None, Some(to)) if self.get(to).is_some_and(|d| d.kind() == Kind::Figure) => {
                    Embed::Figure(to.clone())
                }
                _ => continue,
            };
            if !seen.contains(&embed) {
                seen.push(embed);
            }
        }
        seen
    }

    /// A picture's bytes, read from `attachments/`. Refused for anything else: a path outside
    /// the folder, or a file whose name is not a picture's.
    pub fn picture(&self, path: &VaultPath) -> Result<Vec<u8>> {
        if !path.is_in(&folder()) || picture_type(path.name()).is_none() {
            return Err(Error::InvalidPath {
                path: path.to_string(),
                reason: "only pictures in the attachments folder are read",
            });
        }
        let full = self.inner.locate(path)?;
        let size = fs::metadata(&full)
            .map_err(|e| Error::io("read", &full, e))?
            .len();
        if size > MAX_READ_BYTES {
            return Err(Error::Name(format!("{path} is too large to show.")));
        }
        fs::read(&full).map_err(|e| Error::io("read", &full, e))
    }
}

/// The most a picture read for showing may be: what history keeps, so a picture added by
/// another program is shown if the vault keeps it.
const MAX_READ_BYTES: u64 = 64 * 1024 * 1024;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Doc;
    use crate::events::Origin;
    use crate::kind::Kind;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3];

    fn new_vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(&dir.path().join("vault")).unwrap();
        (dir, vault)
    }

    fn note(vault: &Vault, title: &str, body: &str) -> Doc {
        let mut doc = Doc::new(Kind::Note);
        doc.set_title(title);
        doc.set_body(body);
        vault.add(doc).unwrap()
    }

    #[test]
    fn a_pasted_picture_is_kept_under_a_name_made_from_the_time_and_announced() {
        let (_dir, vault) = new_vault();
        let events = vault.subscribe();
        let place = vault.add_picture(PNG, None).unwrap();
        assert!(
            place.as_str().starts_with("attachments/Pasted 20"),
            "{place}"
        );
        assert!(place.as_str().ends_with(".png"));
        assert_eq!(vault.picture(&place).unwrap(), PNG);
        let event = events.try_recv().unwrap();
        assert_eq!(event.path, place);
        assert_eq!(event.change, Change::Created);
        assert_eq!(event.doc, None);
        assert_eq!(event.origin, Origin::Here);
    }

    #[test]
    fn a_dropped_picture_keeps_its_name_and_a_second_with_it_is_numbered() {
        let (_dir, vault) = new_vault();
        let first = vault.add_picture(PNG, Some("Wave diagram.png")).unwrap();
        let second = vault.add_picture(PNG, Some("Wave diagram.png")).unwrap();
        assert_eq!(first.as_str(), "attachments/Wave diagram.png");
        assert_eq!(second.as_str(), "attachments/Wave diagram 2.png");
        // The bytes decide the extension, and a name that could not be a file's is made one.
        let third = vault.add_picture(PNG, Some("what/is: this?.jpg")).unwrap();
        assert_eq!(third.as_str(), "attachments/what is this.png");
    }

    #[test]
    fn what_is_not_a_picture_or_is_too_large_is_refused_and_nothing_is_written() {
        let (_dir, vault) = new_vault();
        for bad in [
            b"#!/bin/sh\necho hi\n".as_slice(),
            b"<svg xmlns='http://www.w3.org/2000/svg'/>",
            b"",
        ] {
            assert!(vault.add_picture(bad, Some("x.png")).is_err());
        }
        let mut huge = PNG.to_vec();
        huge.resize(MAX_PICTURE_BYTES + 1, 0);
        assert!(vault.add_picture(&huge, None).is_err());
        assert!(!vault.root().join("attachments").exists());
    }

    #[test]
    fn only_pictures_in_the_attachments_folder_are_read() {
        let (_dir, vault) = new_vault();
        let place = vault.add_picture(PNG, Some("p.png")).unwrap();
        assert!(vault.picture(&place).is_ok());
        std::fs::write(vault.root().join("attachments/secret.txt"), "x").unwrap();
        std::fs::create_dir_all(vault.root().join("library")).unwrap();
        std::fs::write(vault.root().join("library/book.png"), PNG).unwrap();
        for not in ["attachments/secret.txt", "library/book.png", "notes/n.md"] {
            let path = VaultPath::new(not).unwrap();
            assert!(
                matches!(vault.picture(&path), Err(Error::InvalidPath { .. })),
                "{not}"
            );
        }
        let missing = VaultPath::new("attachments/missing.png").unwrap();
        assert!(vault.picture(&missing).is_err());
    }

    #[test]
    fn links_to_pictures_and_figures_resolve_and_the_rest_stays_unresolved() {
        let (_dir, vault) = new_vault();
        vault.add_picture(PNG, Some("p.png")).unwrap();
        let mut figure = Doc::new(Kind::Figure);
        figure.set_title("Standing wave");
        let figure = vault.add(figure).unwrap();
        let body = format!(
            "![[p.png|300]]\n![a picture](../attachments/p.png)\n![[attachments/p.png]]\n\
             ![[figure:{}]]\n![[figure:nope]]\n![[missing.png]]\n![[Elsewhere/p.png]]\n",
            figure.id()
        );
        let waves = note(&vault, "Waves", &body);
        let outlinks = vault.outlinks(waves.id());
        let seen: Vec<(String, Option<String>, bool)> = outlinks
            .iter()
            .map(|o| {
                (
                    o.link.target.clone(),
                    o.picture.as_ref().map(|p| p.to_string()),
                    o.to.as_ref() == Some(figure.id()),
                )
            })
            .collect();
        let pic = Some("attachments/p.png".to_owned());
        assert_eq!(seen[0], ("p.png".into(), pic.clone(), false));
        assert_eq!(seen[1], ("../attachments/p.png".into(), pic.clone(), false));
        assert_eq!(seen[2], ("attachments/p.png".into(), pic, false));
        assert_eq!(seen[3].0, format!("figure:{}", figure.id()));
        assert!(seen[3].2 && seen[3].1.is_none());
        assert_eq!(seen[4], ("figure:nope".into(), None, false));
        assert_eq!(seen[5], ("missing.png".into(), None, false));
        assert_eq!(seen[6], ("Elsewhere/p.png".into(), None, false));
        let unresolved: Vec<String> = vault
            .unresolved()
            .into_iter()
            .map(|(_, link)| link.target)
            .collect();
        assert_eq!(
            unresolved,
            ["figure:nope", "missing.png", "Elsewhere/p.png"]
        );
        assert_eq!(vault.backlinks(figure.id()).len(), 1);
        assert_eq!(
            vault.resolve_picture("p.png", waves.id()).unwrap().as_str(),
            "attachments/p.png"
        );
        assert_eq!(vault.resolve_picture("missing.png", waves.id()), None);
        assert_eq!(vault.resolve("figure:nope", waves.id()), None);
        assert_eq!(
            vault.resolve(&format!("Figure:{}", figure.id()), waves.id()),
            Some(figure.id().clone())
        );
    }

    #[test]
    fn a_note_embeds_the_pictures_and_figures_it_names_once_each_in_order() {
        let (_dir, vault) = new_vault();
        vault.add_picture(PNG, Some("p.png")).unwrap();
        vault.add_picture(PNG, Some("q.png")).unwrap();
        let mut figure = Doc::new(Kind::Figure);
        figure.set_title("Standing wave");
        let figure = vault.add(figure).unwrap();
        let other = note(&vault, "Other", "Some words.\n");
        let body = format!(
            "![[q.png|300]]\n![[figure:{id}]]\n![the first](attachments/p.png)\n![[q.png]]\n\
             [[p.png]] ![[figure:{id}]] ![[Other]] ![[missing.png]]\n",
            id = figure.id()
        );
        let waves = note(&vault, "Waves", &body);
        let q = VaultPath::new("attachments/q.png").unwrap();
        let p = VaultPath::new("attachments/p.png").unwrap();
        assert_eq!(
            vault.embeds(waves.id()),
            [
                Embed::Picture {
                    path: q.clone(),
                    width: Some(300),
                    alt: None
                },
                Embed::Figure(figure.id().clone()),
                Embed::Picture {
                    path: p,
                    width: None,
                    alt: Some("the first".into())
                },
                Embed::Picture {
                    path: q,
                    width: None,
                    alt: None
                },
            ]
        );
        assert_eq!(vault.embeds(other.id()), []);
    }

    #[test]
    fn a_figure_link_is_left_as_written_when_the_figure_moves() {
        let (_dir, vault) = new_vault();
        let mut figure = Doc::new(Kind::Figure);
        figure.set_title("Standing wave");
        let figure = vault.add(figure).unwrap();
        let body = format!("![[figure:{}]]\n", figure.id());
        let waves = note(&vault, "Waves", &body);
        let mut moved = figure.clone();
        moved.set_path(VaultPath::new("figures/Physics/Standing wave.md").unwrap());
        vault.update(&moved).unwrap();
        assert_eq!(vault.get(waves.id()).unwrap().body(), body);
        assert_eq!(vault.backlinks(figure.id()).len(), 1);
    }
}
