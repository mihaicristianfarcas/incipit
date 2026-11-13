//! A book's text index as a file: a short header, then the index's pieces in little-endian
//! numbers, most as variable-length integers (seven bits a byte) and positions as differences
//! from the one before, so a word takes a few bytes. Reading checks every count against what is
//! left, every position against its text and every term against the vocabulary, and says what
//! is wrong rather than panicking: the file is a cache, made again when it cannot be read.

use anyhow::{anyhow, bail, ensure, Result};

use super::{Indexed, Source, TextIndex, Word};
use crate::pdf::Entry;

/// What a file of this kind starts with, and the form's version: a file of another version is
/// made again.
const MAGIC: &[u8] = b"incipit text index\n";
const VERSION: u32 = 1;

pub(super) fn encode(index: &TextIndex) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&index.source.len.to_le_bytes());
    out.extend_from_slice(&index.source.modified.to_le_bytes());
    out.push(u8::from(index.pdf));
    number(&mut out, index.contents.len() as u64);
    for entry in &index.contents {
        text(&mut out, &entry.title);
        number(&mut out, entry.depth as u64);
        number(&mut out, entry.page as u64);
        match entry.y {
            Some(y) => {
                out.push(1);
                out.extend_from_slice(&y.to_le_bytes());
            }
            None => out.push(0),
        }
    }
    number(&mut out, index.vocabulary.len() as u64);
    for word in &index.vocabulary {
        text(&mut out, word);
    }
    number(&mut out, index.parts.len() as u64);
    for part in &index.parts {
        number(&mut out, u64::from(part.part));
        text(&mut out, &part.text);
        number(&mut out, part.runs.len() as u64);
        let mut last = 0;
        for (start, own) in &part.runs {
            number(&mut out, u64::from(start - last));
            number(&mut out, u64::from(*own));
            last = *start;
        }
        number(&mut out, part.lines.len() as u64);
        let mut last = 0;
        for (start, top) in &part.lines {
            number(&mut out, u64::from(start - last));
            out.extend_from_slice(&top.to_le_bytes());
            last = *start;
        }
        number(&mut out, part.words.len() as u64);
        let mut last = 0;
        for word in &part.words {
            number(&mut out, u64::from(word.term));
            number(&mut out, u64::from(word.start - last));
            number(&mut out, u64::from(word.end - word.start));
            last = word.end;
        }
    }
    out
}

pub(super) fn decode(bytes: &[u8]) -> Result<TextIndex> {
    let mut r = Reader { bytes, at: 0 };
    ensure!(
        r.take(MAGIC.len())? == MAGIC,
        "this is not a book's text index"
    );
    let version = u32::from_le_bytes(r.array()?);
    ensure!(
        version == VERSION,
        "the index is of version {version}, not {VERSION}"
    );
    let source = Source {
        len: u64::from_le_bytes(r.array()?),
        modified: u64::from_le_bytes(r.array()?),
    };
    let pdf = match r.byte()? {
        0 => false,
        1 => true,
        other => bail!("the index's kind of book is {other}"),
    };
    let count = r.count()?;
    let mut contents = Vec::with_capacity(count);
    for _ in 0..count {
        let title = r.text()?;
        let depth = r.index()?;
        let page = r.index()?;
        let y = match r.byte()? {
            0 => None,
            1 => Some(f32::from_le_bytes(r.array()?)),
            other => bail!("a line of the contents is marked {other}"),
        };
        contents.push(Entry {
            title,
            depth,
            page,
            y,
        });
    }
    let count = r.count()?;
    let mut vocabulary = Vec::with_capacity(count);
    for _ in 0..count {
        let word = r.text()?;
        ensure!(
            vocabulary
                .last()
                .is_none_or(|before: &String| *before < word),
            "the index's words are not in order"
        );
        vocabulary.push(word);
    }
    let count = r.count()?;
    let mut parts = Vec::with_capacity(count);
    for _ in 0..count {
        parts.push(r.part(vocabulary.len())?);
    }
    ensure!(r.at == bytes.len(), "the index has bytes after its end");
    Ok(TextIndex::assemble(
        source, pdf, contents, vocabulary, parts,
    ))
}

/// A number in seven bits a byte, the high bit set on all but the last.
fn number(out: &mut Vec<u8>, mut n: u64) {
    loop {
        let low = (n & 0x7f) as u8;
        n >>= 7;
        if n == 0 {
            out.push(low);
            return;
        }
        out.push(low | 0x80);
    }
}

fn text(out: &mut Vec<u8>, text: &str) {
    number(out, text.len() as u64);
    out.extend_from_slice(text.as_bytes());
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let end = self
            .at
            .checked_add(n)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| anyhow!("the index ends early"))?;
        let taken = &self.bytes[self.at..end];
        self.at = end;
        Ok(taken)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut out = [0; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn number(&mut self) -> Result<u64> {
        let mut n: u64 = 0;
        for shift in (0..64).step_by(7) {
            let byte = self.byte()?;
            n |= u64::from(byte & 0x7f)
                .checked_shl(shift)
                .filter(|part| part >> shift == u64::from(byte & 0x7f))
                .ok_or_else(|| anyhow!("a number in the index is too large"))?;
            if byte & 0x80 == 0 {
                return Ok(n);
            }
        }
        bail!("a number in the index is too long")
    }

    fn index(&mut self) -> Result<usize> {
        usize::try_from(self.number()?).map_err(|_| anyhow!("a number in the index is too large"))
    }

    fn small(&mut self) -> Result<u32> {
        u32::try_from(self.number()?).map_err(|_| anyhow!("a position in the index is too large"))
    }

    /// A count of things that each take a byte at least: never more than the bytes left, so a
    /// broken count cannot ask for a vast allocation.
    fn count(&mut self) -> Result<usize> {
        let count = self.index()?;
        ensure!(
            count <= self.bytes.len() - self.at,
            "a count in the index is larger than the index"
        );
        Ok(count)
    }

    fn text(&mut self) -> Result<String> {
        let len = self.index()?;
        let bytes = self.take(len)?.to_vec();
        String::from_utf8(bytes).map_err(|_| anyhow!("a text in the index is not UTF-8"))
    }

    fn part(&mut self, terms: usize) -> Result<Indexed> {
        let part = self.small()?;
        let text = self.text()?;
        let place = |at: u32| -> Result<u32> {
            ensure!(
                text.is_char_boundary(at as usize),
                "a position in the index is not in its text"
            );
            Ok(at)
        };
        let count = self.count()?;
        let mut runs = Vec::with_capacity(count);
        let mut last = 0u32;
        for _ in 0..count {
            let start = place(add(last, self.small()?)?)?;
            runs.push((start, self.small()?));
            last = start;
        }
        let count = self.count()?;
        let mut lines = Vec::with_capacity(count);
        let mut last = 0u32;
        for _ in 0..count {
            let start = place(add(last, self.small()?)?)?;
            lines.push((start, f32::from_le_bytes(self.array()?)));
            last = start;
        }
        let count = self.count()?;
        let mut words = Vec::with_capacity(count);
        let mut last = 0u32;
        for _ in 0..count {
            let term = self.small()?;
            ensure!(
                (term as usize) < terms,
                "a word of the index is not in its vocabulary"
            );
            let start = place(add(last, self.small()?)?)?;
            let end = place(add(start, self.small()?)?)?;
            ensure!(end > start, "a word of the index is empty");
            words.push(Word { term, start, end });
            last = end;
        }
        Ok(Indexed {
            part,
            text,
            runs,
            lines,
            words,
            chapters: Vec::new(),
        })
    }
}

fn add(a: u32, b: u32) -> Result<u32> {
    a.checked_add(b)
        .ok_or_else(|| anyhow!("a position in the index is too large"))
}
