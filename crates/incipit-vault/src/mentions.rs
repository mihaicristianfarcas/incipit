//! Unlinked mentions: where a note names another document, by its title or one of its other
//! names, in plain words without linking it, and a way to turn the first such mention into a
//! link. As in the earlier app, a name is a mention only as a whole word (the characters on
//! either side are not letters or digits, and not the brackets and bars a link is written with),
//! in any case, and only names of three characters or more count, so "go" is never found
//! everywhere. A note that already links the document anywhere (its text or its properties) is
//! not listed, and neither is a line that already holds a link whose target starts with the
//! document's title.
//!
//! Matching is by characters, one by one, lowercased: no byte offsets of a lowercased copy, which
//! would be wrong where lowercasing changes a character's length.

use crate::doc::DocId;
use crate::error::Result;
use crate::kind::Kind;
use crate::store::Vault;

/// The least characters of a name for it to count as a mention.
const SHORTEST: usize = 3;

/// The most notes listed for one document.
const MOST: usize = 50;

/// The most characters of the line a mention is shown with.
const CONTEXT: usize = 240;

/// A note that names a document without linking it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mention {
    pub id: DocId,
    /// Its vault path.
    pub path: String,
    pub title: String,
    /// The first line that names the document, trimmed.
    pub context: String,
}

/// The names a document is mentioned by: its title and its other names that are long enough.
fn names_of(title: &str, aliases: &[String]) -> Vec<String> {
    std::iter::once(title.to_owned())
        .chain(aliases.iter().cloned())
        .filter(|name| name.chars().count() >= SHORTEST)
        .collect()
}

/// Whether a character may be right beside a mention: not a letter or a digit. Where the mention
/// begins also not `[` or `|`, and where it ends not `]` or `|`, which a link is written with.
fn open(c: char, link_mark: char) -> bool {
    !(c.is_alphanumeric() || c == link_mark || c == '|')
}

/// How many bytes of `text` the characters of `name` take, when `text` starts with `name` in any
/// case.
fn starts_with_name(text: &str, name: &str) -> Option<usize> {
    let mut chars = text.char_indices();
    for wanted in name.chars() {
        let (_, found) = chars.next()?;
        if !found.to_lowercase().eq(wanted.to_lowercase()) {
            return None;
        }
    }
    Some(chars.next().map_or(text.len(), |(at, _)| at))
}

/// The first place in `line` where one of `names` stands as a word: the start and end in bytes,
/// and the name as written there. At one place the names are tried in the order given.
fn find_in(line: &str, names: &[String]) -> Option<(usize, usize)> {
    let mut before: Option<char> = None;
    for (at, c) in line.char_indices() {
        if before.is_none_or(|b| open(b, '[')) {
            for name in names {
                let Some(length) = starts_with_name(&line[at..], name) else {
                    continue;
                };
                let end = at + length;
                if line[end..].chars().next().is_none_or(|a| open(a, ']')) {
                    return Some((at, end));
                }
            }
        }
        before = Some(c);
    }
    None
}

/// Whether a line has a wiki link in it somewhere.
fn has_wiki_link(line: &str) -> bool {
    let mut rest = line;
    while let Some(start) = rest.find("[[") {
        let after = &rest[start + 2..];
        match after.find(']') {
            Some(end) if after[end..].starts_with("]]") => return true,
            Some(end) => rest = &after[end..],
            None => return false,
        }
    }
    false
}

impl Vault {
    /// The notes that name document `id` in plain text without linking it, in the order of the
    /// vault's notes, at most fifty. None when it is not a document.
    pub fn unlinked_mentions(&self, id: &DocId) -> Vec<Mention> {
        let Some(doc) = self.get(id) else {
            return Vec::new();
        };
        let names = names_of(&doc.title(), &doc.aliases());
        if names.is_empty() {
            return Vec::new();
        }
        let linking: Vec<DocId> = self.backlinks(id).into_iter().map(|b| b.from).collect();
        let title = doc.title();
        let mut found = Vec::new();
        for other in self.list(Some(Kind::Note)) {
            if other.id() == id || linking.contains(other.id()) {
                continue;
            }
            let line = other.body().split('\n').find(|line| {
                find_in(line, &names).is_some() && !line.contains(&format!("[[{title}"))
            });
            if let Some(line) = line {
                found.push(Mention {
                    id: other.id().clone(),
                    path: other.path().to_string(),
                    title: other.title(),
                    context: line.trim().chars().take(CONTEXT).collect(),
                });
            }
        }
        found.truncate(MOST);
        found
    }

    /// Turns the first plain mention of document `target` (by its title or another name, the
    /// longest first) in the note `note` into a link: `[[Title]]` when it was written as the
    /// title is, else `[[Title|as written]]`. A line that already links something that starts
    /// with the title is passed by. Returns whether a link was made; the note is written only
    /// then.
    pub fn link_mention(&self, target: &DocId, note: &DocId) -> Result<bool> {
        let (Some(target), Some(mut note)) = (self.get(target), self.get(note)) else {
            return Err(crate::error::Error::NoDoc(
                "the note or what it mentions".into(),
            ));
        };
        let title = target.title();
        let mut names = names_of(&title, &target.aliases());
        names.sort_by_key(|name| std::cmp::Reverse(name.chars().count()));
        let own = format!("[[{}", title.to_lowercase());
        let mut lines: Vec<String> = note.body().split('\n').map(str::to_owned).collect();
        for line in &mut lines {
            if has_wiki_link(line) && line.to_lowercase().contains(&own) {
                continue;
            }
            let Some((start, end)) = find_in(line, &names) else {
                continue;
            };
            let written = line[start..end].to_owned();
            let link = if written == title {
                format!("[[{title}]]")
            } else {
                format!("[[{title}|{written}]]")
            };
            line.replace_range(start..end, &link);
            let body = lines.join("\n");
            note.set_body(body);
            self.update(&note)?;
            return Ok(true);
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Doc;

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        (dir, vault)
    }

    fn note(vault: &Vault, title: &str, body: &str) -> Doc {
        let mut doc = Doc::new(Kind::Note);
        doc.set_title(title);
        doc.set_body(body);
        vault.add(doc).unwrap()
    }

    #[test]
    fn a_name_is_a_mention_only_as_a_whole_word_in_any_case() {
        let names = vec!["Quorums".to_owned(), "Intersecting sets".to_owned()];
        assert_eq!(find_in("Read about quorums today", &names), Some((11, 18)));
        assert_eq!(find_in("QUORUMS", &names), Some((0, 7)));
        assert_eq!(find_in("quorums, then more", &names), Some((0, 7)));
        assert_eq!(find_in("(quorums)", &names), Some((1, 8)));
        // Inside a longer word, or written as part of a link, it is not one.
        assert_eq!(find_in("Quorumsy", &names), None);
        assert_eq!(find_in("preQuorums", &names), None);
        assert_eq!(find_in("[[Quorums]]", &names), None);
        assert_eq!(find_in("[[Raft|Quorums]]", &names), None);
        assert_eq!(find_in("[[Raft|a quorums b]]", &names), Some((9, 16)));
        assert_eq!(find_in("[[Raft|a quorums]]", &names), None);
        assert_eq!(find_in("intersecting SETS.", &names), Some((0, 17)));
        // Accents and other scripts are letters too.
        let greek = vec!["Ελλάδα".to_owned()];
        assert_eq!(find_in("η ελλάδα είναι", &greek), Some((3, 15)));
        assert_eq!(find_in("ηΕλλάδα", &greek), None);
    }

    #[test]
    fn the_notes_that_name_a_document_without_linking_it_are_listed() {
        let (_dir, vault) = vault();
        let mut quorums = Doc::new(Kind::Note);
        quorums.set_title("Quorums");
        quorums.set_aliases(vec!["Majority sets".to_owned(), "Qu".to_owned()]);
        let quorums = vault.add(quorums).unwrap();
        let plain = note(
            &vault,
            "Raft",
            "Intro.\nRaft needs a majority sets of nodes.\n",
        );
        note(&vault, "Linked", "We use [[Quorums]] and quorums again.\n");
        let mut by_property = Doc::new(Kind::Note);
        by_property.set_title("By property");
        by_property.set_body("quorums everywhere\n");
        by_property.set_related(vec!["[[Quorums]]".to_owned()]);
        vault.add(by_property).unwrap();
        note(
            &vault,
            "Elsewhere",
            "Qu is too short; quorum is not the word.\n",
        );
        note(&vault, "On the line", "Quorums and [[Quorums|them]]\n");
        let long = format!("{}Quorums{}", "a ".repeat(200), " b".repeat(200));
        let far = note(&vault, "Far", &long);
        let mentions = vault.unlinked_mentions(quorums.id());
        let titles: Vec<&str> = mentions.iter().map(|m| m.title.as_str()).collect();
        assert_eq!(titles, ["Far", "Raft"]);
        let raft = mentions.iter().find(|m| m.id == *plain.id()).unwrap();
        assert_eq!(raft.context, "Raft needs a majority sets of nodes.");
        assert_eq!(raft.path, plain.path().to_string());
        // A long line is cut for showing.
        let far = mentions.iter().find(|m| m.id == *far.id()).unwrap();
        assert_eq!(far.context.chars().count(), CONTEXT);
        // The document is not its own mention, and nothing is listed for one that is not there.
        assert!(!titles.contains(&"Quorums"));
        assert_eq!(vault.unlinked_mentions(&DocId::from("nothing")), []);
    }

    #[test]
    fn at_most_fifty_notes_are_listed() {
        let (_dir, vault) = vault();
        let target = note(&vault, "Entropy", "");
        for i in 0..55 {
            note(&vault, &format!("Note {i:02}"), "On entropy.\n");
        }
        assert_eq!(vault.unlinked_mentions(target.id()).len(), 50);
    }

    #[test]
    fn linking_a_mention_writes_the_title_and_keeps_the_words_as_written() {
        let (_dir, vault) = vault();
        let mut target = Doc::new(Kind::Note);
        target.set_title("Quorums");
        target.set_aliases(vec!["Majority sets".to_owned()]);
        let target = vault.add(target).unwrap();
        let raft = note(
            &vault,
            "Raft",
            "A line with [[Other]] and quorums too.\nAbout quorums here.\nAnd quorums, again.\n",
        );
        // The first line that has no link to a title like it is the one that changes.
        assert!(vault.link_mention(target.id(), raft.id()).unwrap());
        let body = vault.get(raft.id()).unwrap().body().to_owned();
        assert_eq!(
            body,
            "A line with [[Other]] and [[Quorums|quorums]] too.\nAbout quorums here.\nAnd quorums, again.\n"
        );
        // Written as the title is, it is a plain link; the longest name wins at a place.
        assert!(vault.link_mention(target.id(), raft.id()).unwrap());
        let body = vault.get(raft.id()).unwrap().body().to_owned();
        assert!(body.contains("About [[Quorums|quorums]] here."), "{body}");
        let sets = note(&vault, "Sets", "The Majority sets rule.\nQuorums\n");
        assert!(vault.link_mention(target.id(), sets.id()).unwrap());
        assert_eq!(
            vault.get(sets.id()).unwrap().body(),
            "The [[Quorums|Majority sets]] rule.\nQuorums\n"
        );
        assert!(vault.link_mention(target.id(), sets.id()).unwrap());
        assert_eq!(
            vault.get(sets.id()).unwrap().body(),
            "The [[Quorums|Majority sets]] rule.\n[[Quorums]]\n"
        );
        // Nothing left to link leaves the note as it is, and is said.
        let before = vault.get(sets.id()).unwrap();
        assert!(!vault.link_mention(target.id(), sets.id()).unwrap());
        assert_eq!(vault.get(sets.id()).unwrap().to_text(), before.to_text());
        // After linking, the note is no longer listed as a mention.
        assert!(vault
            .unlinked_mentions(target.id())
            .iter()
            .all(|m| m.id != *sets.id()));
        assert!(vault
            .link_mention(&DocId::from("nothing"), sets.id())
            .is_err());
    }

    #[test]
    fn a_line_that_links_the_title_somewhere_is_passed_by() {
        let (_dir, vault) = vault();
        let target = note(&vault, "Entropy", "");
        let other = note(
            &vault,
            "Heat",
            "See [[entropy#Definition]] and entropy.\nPlain entropy here.\n",
        );
        assert!(vault.link_mention(target.id(), other.id()).unwrap());
        assert_eq!(
            vault.get(other.id()).unwrap().body(),
            "See [[entropy#Definition]] and entropy.\nPlain [[Entropy|entropy]] here.\n"
        );
    }
}
