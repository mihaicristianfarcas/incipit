//! Pictures in notes, kept in `attachments/`: pasted or dropped into a note, they are saved here
//! under a free name and linked as `![[name.png]]`, as Obsidian does. Only pictures are ever
//! read from this folder: a file is a picture by what its name says (`picture_type`), and one
//! that is added must be one by what it holds (`sniff`), so the vault never keeps or reads
//! anything else under that name. The writing and reading are `Vault::add_picture` and
//! `Vault::picture`.

/// The folder pictures are kept in, from the vault's root.
pub const ATTACHMENTS: &str = "attachments";

/// The largest picture that is added: a photo of a page fits, a video does not.
pub const MAX_PICTURE_BYTES: usize = 12 * 1024 * 1024;

/// The content type of a picture file by its extension, or `None` when it is not one.
pub fn picture_type(name: &str) -> Option<&'static str> {
    let (_, extension) = name.rsplit_once('.')?;
    match extension.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "svg" => Some("image/svg+xml"),
        _ => None,
    }
}

/// The extension a picture's own bytes call for (`png`, `jpg`, `gif` or `webp`), or `None` when
/// they are none of those. SVG is not added: it is text that can say anything, and only
/// pictures the vault was given as pictures are drawn from it.
pub fn sniff(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        Some("png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

/// Whether a link's target names a picture (by its extension).
pub(crate) fn names_a_picture(target: &str) -> bool {
    picture_type(target).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_a_picture_by_its_extension_in_any_case() {
        assert_eq!(picture_type("Diagram.PNG"), Some("image/png"));
        assert_eq!(picture_type("photo.jpeg"), Some("image/jpeg"));
        assert_eq!(picture_type("a.b.webp"), Some("image/webp"));
        assert_eq!(picture_type("notes.md"), None);
        assert_eq!(picture_type("png"), None);
        assert_eq!(picture_type("script.js"), None);
    }

    #[test]
    fn bytes_are_a_picture_by_what_they_start_with() {
        assert_eq!(
            sniff(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0]),
            Some("png")
        );
        assert_eq!(sniff(&[0xff, 0xd8, 0xff, 0xe0]), Some("jpg"));
        assert_eq!(sniff(b"GIF89a...."), Some("gif"));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(sniff(b"<svg xmlns='http://www.w3.org/2000/svg'/>"), None);
        assert_eq!(sniff(b"#!/bin/sh\n"), None);
        assert_eq!(sniff(b""), None);
        assert_eq!(sniff(b"RIFF\0\0\0\0WAVE"), None);
    }
}
