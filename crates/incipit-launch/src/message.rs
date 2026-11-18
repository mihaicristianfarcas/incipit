//! What a second launch sends to the program running: the files it was given, as absolute paths,
//! and nothing else. The form is a fixed first line, then each path ended by a NUL, which no path
//! holds on any system. The bytes come from another process, so reading them trusts nothing: a
//! message that is too long, does not begin with the first line, or has a path that is empty, too
//! long or not ended is dropped whole, and a path that is not absolute is left out (it was meant
//! from another working folder). Whether each path is a file the library can add is the
//! receiver's to decide.
//!
//! A path is its bytes on macOS and Linux, where a file name can be any bytes, and UTF-8 on
//! Windows; a path a message cannot hold (longer than `MAX_PATH`, or on Windows not Unicode) is
//! not sent. A launch given more files than one message holds sends several.

use std::path::{Path, PathBuf};

/// The first line of every message: what it is and the form's version.
pub const FIRST_LINE: &[u8] = b"incipit-open 1\n";

/// The most files in one message. A person opens a few books at a time; a launch given more
/// sends several messages.
pub const MAX_FILES: usize = 32;

/// The longest path a message holds, in bytes: Linux's own limit for a path (`PATH_MAX`).
pub const MAX_PATH: usize = 4096;

/// The longest message: the first line and `MAX_FILES` paths of `MAX_PATH` bytes, each ended.
pub const MAX_MESSAGE: usize = FIRST_LINE.len() + MAX_FILES * (MAX_PATH + 1);

/// Ends each path.
const END: u8 = 0;

/// The messages that carry `files`, in order: one with none when there are none, since a launch
/// with no file still asks the program running to come forward.
pub fn encode(files: &[PathBuf]) -> Vec<Vec<u8>> {
    let paths: Vec<Vec<u8>> = files
        .iter()
        .filter_map(|file| bytes_of(file))
        .filter(|bytes| !bytes.is_empty() && bytes.len() <= MAX_PATH)
        .collect();
    if paths.is_empty() {
        return vec![FIRST_LINE.to_vec()];
    }
    paths
        .chunks(MAX_FILES)
        .map(|chunk| {
            let mut message = FIRST_LINE.to_vec();
            for path in chunk {
                message.extend_from_slice(path);
                message.push(END);
            }
            message
        })
        .collect()
}

/// The absolute paths a message names, in order, or `None` when it is not a message of this form.
pub fn decode(message: &[u8]) -> Option<Vec<PathBuf>> {
    if message.len() > MAX_MESSAGE {
        return None;
    }
    let body = message.strip_prefix(FIRST_LINE)?;
    if body.is_empty() {
        return Some(Vec::new());
    }
    let body = body.strip_suffix(&[END])?;
    let parts: Vec<&[u8]> = body.split(|b| *b == END).collect();
    if parts.len() > MAX_FILES {
        return None;
    }
    let mut paths = Vec::with_capacity(parts.len());
    for part in parts {
        if part.is_empty() || part.len() > MAX_PATH {
            return None;
        }
        let path = path_of(part)?;
        if path.is_absolute() {
            paths.push(path);
        }
    }
    Some(paths)
}

#[cfg(unix)]
fn bytes_of(path: &Path) -> Option<Vec<u8>> {
    use std::os::unix::ffi::OsStrExt;
    Some(path.as_os_str().as_bytes().to_vec())
}

#[cfg(unix)]
fn path_of(bytes: &[u8]) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
}

#[cfg(not(unix))]
fn bytes_of(path: &Path) -> Option<Vec<u8>> {
    path.to_str().map(|text| text.as_bytes().to_vec())
}

#[cfg(not(unix))]
fn path_of(bytes: &[u8]) -> Option<PathBuf> {
    std::str::from_utf8(bytes).ok().map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An absolute path on the system the test runs on.
    fn absolute(name: &str) -> PathBuf {
        std::env::temp_dir().join(name)
    }

    fn message(paths: &[&[u8]]) -> Vec<u8> {
        let mut bytes = FIRST_LINE.to_vec();
        for path in paths {
            bytes.extend_from_slice(path);
            bytes.push(END);
        }
        bytes
    }

    fn bytes(path: &Path) -> Vec<u8> {
        bytes_of(path).unwrap()
    }

    #[test]
    fn files_are_read_back_as_they_were_sent_in_order() {
        let files = vec![
            absolute("Stories of Light.pdf"),
            absolute("Électricité et magnétisme.epub"),
            absolute("line\nbreak.pdf"),
        ];
        let sent = encode(&files);
        assert_eq!(sent.len(), 1);
        assert_eq!(decode(&sent[0]).unwrap(), files);
    }

    #[test]
    fn a_launch_with_no_file_still_sends_one_message_with_none() {
        let sent = encode(&[]);
        assert_eq!(sent, vec![FIRST_LINE.to_vec()]);
        assert_eq!(decode(&sent[0]).unwrap(), Vec::<PathBuf>::new());
    }

    #[test]
    fn more_files_than_one_message_holds_are_sent_in_several() {
        let files: Vec<PathBuf> = (0..MAX_FILES * 2 + 1)
            .map(|n| absolute(&format!("book {n}.pdf")))
            .collect();
        let sent = encode(&files);
        assert_eq!(sent.len(), 3);
        let back: Vec<PathBuf> = sent.iter().flat_map(|m| decode(m).unwrap()).collect();
        assert_eq!(back, files);
        assert!(sent.iter().all(|m| m.len() <= MAX_MESSAGE));
    }

    #[test]
    fn a_path_a_message_cannot_hold_is_not_sent() {
        let long = absolute(&"a".repeat(MAX_PATH));
        let kept = absolute("kept.pdf");
        let sent = encode(&[long, PathBuf::new(), kept.clone()]);
        assert_eq!(decode(&sent[0]).unwrap(), [kept]);
    }

    #[test]
    fn the_longest_message_is_read_and_one_byte_more_is_dropped() {
        let path = vec![b'/'; MAX_PATH];
        let parts: Vec<&[u8]> = vec![&path; MAX_FILES];
        let longest = message(&parts);
        assert_eq!(longest.len(), MAX_MESSAGE);
        let read = decode(&longest).unwrap();
        if cfg!(unix) {
            assert_eq!(read.len(), MAX_FILES);
        }
        let mut over = longest;
        over.push(b'/');
        assert_eq!(decode(&over), None);
    }

    #[test]
    fn too_many_files_or_a_path_too_long_drops_the_message() {
        let one = bytes(&absolute("a.pdf"));
        let many: Vec<&[u8]> = vec![&one; MAX_FILES + 1];
        assert_eq!(decode(&message(&many)), None);
        let long = vec![b'/'; MAX_PATH + 1];
        assert_eq!(decode(&message(&[&long])), None);
    }

    #[test]
    fn malformed_messages_are_dropped_whole() {
        let one = bytes(&absolute("a.pdf"));
        // Not this form at all, or another version of it.
        assert_eq!(decode(b""), None);
        assert_eq!(decode(b"GET / HTTP/1.1\r\n\r\n"), None);
        assert_eq!(decode(b"incipit-open 2\n"), None);
        assert_eq!(decode(&one), None);
        // A path not ended, and an empty one between two.
        let mut cut = message(&[&one]);
        cut.pop();
        assert_eq!(decode(&cut), None);
        assert_eq!(decode(&message(&[&one, b"", &one])), None);
        // Random bytes after the first line.
        let mut noise = FIRST_LINE.to_vec();
        noise.extend((0..=255u8).cycle().take(5000));
        assert_eq!(decode(&noise), None);
    }

    #[test]
    fn a_path_that_is_not_absolute_is_left_out() {
        let one = bytes(&absolute("a.pdf"));
        let paths = decode(&message(&[b"relative/b.pdf", &one, b".."])).unwrap();
        assert_eq!(paths, [absolute("a.pdf")]);
    }

    #[cfg(unix)]
    #[test]
    fn a_file_name_that_is_not_unicode_is_carried_as_its_bytes() {
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::OsStr::from_bytes(b"/books/caf\xe9.pdf");
        let files = vec![PathBuf::from(name)];
        assert_eq!(decode(&encode(&files)[0]).unwrap(), files);
    }

    #[cfg(windows)]
    #[test]
    fn a_path_that_is_not_utf8_drops_the_message_on_windows() {
        assert_eq!(decode(&message(&[b"C:\\books\\caf\xe9.pdf"])), None);
    }
}
