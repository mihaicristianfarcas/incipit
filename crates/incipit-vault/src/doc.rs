//! The one record type: a document is a Markdown file with properties (YAML frontmatter) and a
//! body. Every kind shares the same common properties (`id`, `kind`, `type`, `title`, `tags`,
//! `aliases`, `about`, `related`, `concepts`, `source`, `created`, `updated`); what a kind adds is
//! read and written by `fields`.
//!
//! A document keeps every property as it was read, known or not, in its order, so a learner's
//! own keys (and Obsidian's) survive any rewrite; the typed methods read and change single keys
//! in place. A document read from a file remembers how the file began (its byte order mark, its
//! `---` lines, its line endings) and writes back the text it was given exactly: while nothing
//! has changed the file's text comes back as it was, and when a property has changed only that
//! property's lines are written again (`amend`), so a learner's comments, quoting and blank lines
//! stay. A new key goes where the common ones are usually written, before `created` and
//! `updated`, and anyone's own keys never move.
//!
//! The id is the `id` property. A file without one (written by hand or in Obsidian) gets an id
//! made from its path, which is stable for as long as the file stays where it is; documents the
//! app makes always carry their id, so they keep it when moved.

use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::amend;
use crate::error::{Error, Result};
use crate::fields::Source;
use crate::frontmatter;
use crate::kind::{file_name_of, Kind};
use crate::path::VaultPath;
use crate::value::{Properties, Value};

/// A document's id: its `id` property, or one made from its path.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocId(String);

/// How many characters of an id are random: 12 base-32 characters are 60 bits, enough that
/// no two documents a person makes will ever share one (the store checks anyway).
const ID_CHARS: usize = 12;

const ID_ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

impl DocId {
    /// A new random id.
    pub fn new() -> DocId {
        // `RandomState` is seeded from the system's randomness; a counter makes each id in a
        // process different even from the same seed.
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
        hasher.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default(),
        );
        let mut bits = hasher.finish();
        let mut id = String::with_capacity(ID_CHARS);
        for _ in 0..ID_CHARS {
            id.push(ID_ALPHABET[(bits & 31) as usize] as char);
            bits >>= 5;
        }
        DocId(id)
    }

    /// The id of a file that has none of its own: `p` and a hash of its path (FNV-1a, which is
    /// the same on every system and in every version of Rust).
    pub fn from_path(path: &VaultPath) -> DocId {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in path.as_str().bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        DocId(format!("p{:011x}", hash >> 20))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for DocId {
    fn default() -> Self {
        DocId::new()
    }
}

impl From<&str> for DocId {
    fn from(id: &str) -> Self {
        DocId(id.to_owned())
    }
}

impl fmt::Display for DocId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for DocId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

/// The common properties in the order they are written, then a kind's own (which go before
/// `created`).
pub(crate) const COMMON: [&str; 12] = [
    "id", "kind", "type", "title", "tags", "aliases", "about", "related", "concepts", "source",
    "created", "updated",
];

/// The time now, as properties keep it (RFC 3339, in UTC, to the second).
pub(crate) fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// The time now to the millisecond, for ordering things done within one second.
pub(crate) fn now_precise() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// How a file began when it was read: what a write needs to leave alone what did not change.
#[derive(Clone, Debug, PartialEq)]
struct AsRead {
    /// Whether the text began with a byte order mark.
    bom: bool,
    /// The `---` lines before and after the properties, as written, when there was a block.
    fences: Option<(String, String)>,
    /// The properties' own text, as written; empty when there was no block.
    front: String,
    /// The line ending the file used, for the lines a write adds.
    eol: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Doc {
    id: DocId,
    kind: Kind,
    path: VaultPath,
    properties: Properties,
    body: String,
    /// What the file was like when it was read; `None` for a document made here.
    as_read: Option<AsRead>,
    /// Whether the path was chosen (read from disk, or given); a new document's is made from its
    /// content when it is added.
    placed: bool,
}

impl Doc {
    /// A new document of a kind, with a new id and the time now.
    pub fn new(kind: Kind) -> Doc {
        let id = DocId::new();
        let mut properties = Properties::new();
        properties.set("id", id.as_str());
        properties.set("kind", kind.name());
        if let Some(first) = kind.types().first().filter(|_| kind != Kind::Note) {
            properties.set("type", *first);
        }
        let at = now();
        properties.set("created", at.as_str());
        properties.set("updated", at);
        let folder = match kind {
            Kind::Note => "notes",
            Kind::Concept => "concepts",
            Kind::Card => "cards",
            Kind::Figure => "figures",
            Kind::Annotation | Kind::Session | Kind::Book => "library",
        };
        let path = VaultPath::new(&format!("{folder}/Untitled.md"))
            .expect("a kind's folder and “Untitled.md” are a valid path");
        Doc {
            id,
            kind,
            path,
            properties,
            body: String::new(),
            as_read: None,
            placed: false,
        }
    }

    /// A new document of a kind, at a place of the caller's choosing.
    pub fn new_at(kind: Kind, path: VaultPath) -> Doc {
        let mut doc = Doc::new(kind);
        doc.path = path;
        doc.placed = true;
        doc
    }

    /// Reads a document from its file's text.
    pub fn parse(path: VaultPath, text: &str) -> Result<Doc> {
        let split = frontmatter::split(text);
        let properties = match split.front {
            Some(front) => frontmatter::parse(front).map_err(|message| Error::Frontmatter {
                path: path.to_string(),
                message,
            })?,
            None => Properties::new(),
        };
        let folder_kind = Kind::of_path(&path);
        let kind = properties
            .text("kind")
            .and_then(|name| Kind::from_name(&name))
            .unwrap_or(folder_kind);
        let id = properties
            .text("id")
            .map(|id| DocId(id.trim().to_owned()))
            .filter(|id| !id.0.is_empty())
            .unwrap_or_else(|| DocId::from_path(&path));
        let as_read = AsRead {
            bom: split.bom,
            fences: split.fences.map(|(a, b)| (a.to_owned(), b.to_owned())),
            front: split.front.unwrap_or_default().to_owned(),
            eol: frontmatter::line_ending(text),
        };
        Ok(Doc {
            id,
            kind,
            path,
            properties,
            body: split.body.to_owned(),
            as_read: Some(as_read),
            placed: true,
        })
    }

    /// The file's text. For a document read from a file that is the file's own text with what
    /// changed written in: the body as it is, the properties' lines that changed, and no line
    /// ending added where the file had none.
    pub fn to_text(&self) -> String {
        let Some(read) = &self.as_read else {
            return frontmatter::compose(&frontmatter::write(&self.properties), &self.body);
        };
        let bom = if read.bom { "\u{feff}" } else { "" };
        let front = amend::amend(&read.front, &self.properties, read.eol);
        match &read.fences {
            Some((open, close)) => format!("{bom}{open}{front}{close}{}", self.body),
            None if front.is_empty() => format!("{bom}{}", self.body),
            // The file had no properties and has some now.
            None => format!("{bom}---{eol}{front}---{eol}{}", self.body, eol = read.eol),
        }
    }

    pub fn id(&self) -> &DocId {
        &self.id
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn path(&self) -> &VaultPath {
        &self.path
    }

    pub(crate) fn is_placed(&self) -> bool {
        self.placed
    }

    /// Moves the document to another place (written there by `Vault::update`, which carries the
    /// links to it along).
    pub fn set_path(&mut self, path: VaultPath) {
        self.path = path;
        self.placed = true;
    }

    pub(crate) fn set_id(&mut self, id: DocId) {
        self.id = id;
    }

    pub fn body(&self) -> &str {
        &self.body
    }

    pub fn set_body(&mut self, body: impl Into<String>) {
        self.body = body.into();
    }

    /// Every property as written, the learner's own included.
    pub fn properties(&self) -> &Properties {
        &self.properties
    }

    /// Changes properties directly (anyone's own keys; the typed methods are for known ones).
    pub fn properties_mut(&mut self) -> &mut Properties {
        &mut self.properties
    }

    /// Sets a known property, or removes it when `value` is `None` or empty, keeping the usual
    /// order for a key that is new.
    pub(crate) fn set_property(&mut self, key: &str, value: Option<Value>) {
        let value = value.filter(|v| match v {
            Value::Null => false,
            Value::Text(text) => !text.is_empty(),
            Value::List(items) => !items.is_empty(),
            Value::Map(map) => !map.is_empty(),
            _ => true,
        });
        if self.properties.get(key) == value.as_ref() {
            return;
        }
        match value {
            None => {
                self.properties_mut().remove(key);
            }
            Some(value) => {
                let after: &[&str] = match COMMON.iter().position(|k| *k == key) {
                    Some(at) => &COMMON[at + 1..],
                    None => &["created", "updated"],
                };
                self.properties_mut().set_before(key, value, after);
            }
        }
    }

    /// The title: the file name for notes and concepts, else the `title` property, else one made
    /// from what the document holds.
    pub fn title(&self) -> String {
        if self.kind.title_is_file_name() {
            return self.path.stem().to_owned();
        }
        if let Some(title) = self.properties.text("title") {
            return title;
        }
        crate::fields::made_title(self).unwrap_or_else(|| self.path.stem().to_owned())
    }

    /// Changes the title. For notes and concepts that renames the file (in the same folder).
    pub fn set_title(&mut self, title: &str) {
        if self.kind.title_is_file_name() {
            let name = format!("{}.md", file_name_of(title));
            if let Ok(path) = self.path.with_name(&name) {
                self.path = path;
            }
        } else {
            self.set_property("title", Some(Value::from(title.trim())));
        }
    }

    /// The sort of this kind it is (`summary`, `cloze`, `explain`…); `None` for kinds without
    /// sorts. A sort the kind does not have reads as its first.
    pub fn doc_type(&self) -> Option<&'static str> {
        let types = self.kind.types();
        let said = self.properties.text("type").or_else(|| {
            // An unknown kind names a sort of the folder's kind ("kind: explain").
            self.properties
                .text("kind")
                .filter(|k| Kind::from_name(k).is_none())
        });
        let said = said.as_deref().unwrap_or("");
        types
            .iter()
            .copied()
            .find(|t| *t == said)
            .or(types.first().copied())
    }

    pub fn set_doc_type(&mut self, doc_type: &str) {
        if self.kind.types().contains(&doc_type) {
            let shown = !(self.kind == Kind::Note && doc_type == "note");
            self.set_property("type", shown.then(|| Value::from(doc_type)));
        }
    }

    pub fn tags(&self) -> Vec<String> {
        self.properties.texts("tags")
    }

    pub fn set_tags(&mut self, tags: Vec<String>) {
        self.set_property("tags", Some(Value::from(tags)));
    }

    /// Every tag the document has, as Obsidian counts them: the `tags` property (a leading `#`
    /// is left off), then the `#tags` written in the text, each once, whatever its case. The
    /// `tags` property alone is `tags()`, which is what `set_tags` changes.
    pub fn all_tags(&self) -> Vec<String> {
        let mut tags = Vec::new();
        for tag in self.tags() {
            crate::tags::add(&mut tags, &tag);
        }
        for tag in crate::tags::inline(&self.body) {
            crate::tags::add(&mut tags, &tag);
        }
        tags
    }

    pub fn aliases(&self) -> Vec<String> {
        self.properties.texts("aliases")
    }

    pub fn set_aliases(&mut self, aliases: Vec<String>) {
        self.set_property("aliases", Some(Value::from(aliases)));
    }

    /// What it is about: links to books, concepts or notes (`"[[Derivative]]"`).
    pub fn about(&self) -> Vec<String> {
        self.properties.texts("about")
    }

    pub fn set_about(&mut self, about: Vec<String>) {
        self.set_property("about", Some(Value::from(about)));
    }

    /// Other links: related ideas, the note made from it.
    pub fn related(&self) -> Vec<String> {
        self.properties.texts("related")
    }

    pub fn set_related(&mut self, related: Vec<String>) {
        self.set_property("related", Some(Value::from(related)));
    }

    /// The concepts it is about or tests (a card's, an explanation's): links to them
    /// (`"[[Derivative]]"`); a concept's bare name or id is read the same way.
    pub fn concepts(&self) -> Vec<String> {
        self.properties.texts("concepts")
    }

    pub fn set_concepts(&mut self, concepts: Vec<String>) {
        self.set_property("concepts", Some(Value::from(concepts)));
    }

    /// Where in a book it comes from.
    pub fn source(&self) -> Option<Source> {
        self.properties.get("source").and_then(Source::from_value)
    }

    /// Changes where it comes from; the source's other keys (a reader's own anchor data) stay.
    pub fn set_source(&mut self, source: Option<&Source>) {
        let value = source.map(|s| s.to_value(self.properties.get("source")));
        self.set_property("source", value);
    }

    /// When it was made, as written (RFC 3339 when the app wrote it).
    pub fn created(&self) -> Option<String> {
        self.properties.text("created")
    }

    pub fn updated(&self) -> Option<String> {
        self.properties.text("updated")
    }

    /// Marks it changed now. Only documents that already say when they were made get the mark,
    /// so a file written by hand keeps the properties it had.
    pub(crate) fn touch(&mut self) {
        if self.properties.contains("created") || self.properties.contains("updated") {
            self.set_property("updated", Some(Value::from(now())));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(p: &str) -> VaultPath {
        VaultPath::new(p).unwrap()
    }

    #[test]
    fn unknown_frontmatter_keys_survive_a_rewrite() {
        let text = "---\nmood: curious\ntags: [physics]\nrating: 4\nnested:\n  a: 1\n---\nBody\n";
        let mut doc = Doc::parse(path("notes/Waves.md"), text).unwrap();
        doc.set_tags(vec!["physics".into(), "light".into()]);
        doc.set_body("A new body\n");
        let back = Doc::parse(path("notes/Waves.md"), &doc.to_text()).unwrap();
        assert_eq!(back.properties().text("mood").as_deref(), Some("curious"));
        assert_eq!(back.properties().get("rating"), Some(&Value::Int(4)));
        assert!(back.properties().get("nested").unwrap().as_map().is_some());
        assert_eq!(back.tags(), vec!["physics", "light"]);
        assert_eq!(
            back.properties().keys().collect::<Vec<_>>(),
            ["mood", "tags", "rating", "nested"]
        );
    }

    #[test]
    fn untouched_properties_are_written_back_exactly_as_read() {
        let text = "---\n# my comment\ntitle:   'Spaced'\n---\nBody\n";
        let mut doc = Doc::parse(path("cards/a.md"), text).unwrap();
        doc.set_body("Other body\n");
        assert_eq!(
            doc.to_text(),
            "---\n# my comment\ntitle:   'Spaced'\n---\nOther body\n"
        );
    }

    #[test]
    fn a_file_read_is_written_back_as_it_was_whatever_its_endings_and_fences() {
        for text in [
            "---\n# my comment\ntitle:   'Spaced'\n---\nBody\n",
            "---\r\ntags: [a]\r\n---\r\nBody\r\nTwo\r\n",
            "\u{feff}---\r\ntags: [a]\r\n---  \r\nBody",
            "\u{feff}No properties, a mark, and no final line ending",
            "---\n---\nAn empty block\n",
            "---\ntags: [a]\n---",
            "---\ntags: [a]\n---\n\n\n  Body with spaces   \n\n",
            "",
        ] {
            let doc = Doc::parse(path("notes/Waves.md"), text).unwrap();
            assert_eq!(doc.to_text(), text);
        }
    }

    #[test]
    fn a_change_to_the_body_or_a_property_is_all_that_a_write_changes() {
        let text =
            "\u{feff}---\r\n# a plugin's\r\ntags: [a]   # kept\r\nrating: 4\r\n---\r\nBody\r\nTwo";
        let mut doc = Doc::parse(path("notes/Waves.md"), text).unwrap();
        doc.set_body("Body\r\nThree");
        assert_eq!(
            doc.to_text(),
            text.replace("Two", "Three"),
            "no line ending is added where the file had none"
        );
        doc.set_tags(vec!["a".into(), "b".into()]);
        doc.set_related(vec!["[[Other]]".into()]);
        assert_eq!(
            doc.to_text(),
            "\u{feff}---\r\n# a plugin's\r\ntags: [a, b]   # kept\r\nrating: 4\r\nrelated:\r\n  - \"[[Other]]\"\r\n---\r\nBody\r\nThree"
        );
        // Taking the change back writes the file's own text again.
        doc.set_tags(vec!["a".into()]);
        doc.set_related(Vec::new());
        doc.set_body("Body\r\nTwo");
        assert_eq!(doc.to_text(), text);
    }

    #[test]
    fn the_first_property_of_a_file_that_had_none_is_a_block_in_the_files_own_endings() {
        let mut doc = Doc::parse(path("notes/Waves.md"), "\u{feff}One\r\nTwo\r\n").unwrap();
        doc.set_tags(vec!["a".into()]);
        assert_eq!(
            doc.to_text(),
            "\u{feff}---\r\ntags:\r\n  - a\r\n---\r\nOne\r\nTwo\r\n"
        );
        let mut doc = Doc::parse(path("notes/Waves.md"), "One\n").unwrap();
        doc.set_tags(vec!["a".into()]);
        assert_eq!(doc.to_text(), "---\ntags:\n  - a\n---\nOne\n");
    }

    #[test]
    fn a_tag_property_may_be_written_with_its_hash_and_is_counted_once_with_the_text() {
        let text = "---\ntags: [\"#physics\", Waves]\n---\nAbout #waves and #light.\n";
        let doc = Doc::parse(path("Waves.md"), text).unwrap();
        assert_eq!(doc.tags(), ["#physics", "Waves"]);
        assert_eq!(doc.all_tags(), ["physics", "Waves", "light"]);
    }

    #[test]
    fn a_file_without_properties_gets_its_kind_from_its_folder_and_an_id_from_its_path() {
        let doc = Doc::parse(path("concepts/Derivative.md"), "How fast it changes.\n").unwrap();
        assert_eq!(doc.kind(), Kind::Concept);
        assert_eq!(doc.title(), "Derivative");
        assert_eq!(doc.id(), &DocId::from_path(doc.path()));
        assert_eq!(doc.to_text(), "How fast it changes.\n");
    }

    #[test]
    fn malformed_frontmatter_is_an_error_naming_the_file() {
        let error = Doc::parse(path("notes/Bad.md"), "---\ntitle: [oops\n---\nBody\n").unwrap_err();
        match error {
            Error::Frontmatter { path, .. } => assert_eq!(path, "notes/Bad.md"),
            other => panic!("expected a frontmatter error, got {other:?}"),
        }
    }

    #[test]
    fn a_new_document_says_its_id_kind_and_times() {
        let mut doc = Doc::new(Kind::Card);
        doc.set_tags(vec!["waves".into()]);
        let back = Doc::parse(path("cards/x.md"), &doc.to_text()).unwrap();
        assert_eq!(back.id(), doc.id());
        assert_eq!(back.kind(), Kind::Card);
        assert_eq!(back.doc_type(), Some("basic"));
        assert_eq!(
            back.properties().keys().collect::<Vec<_>>(),
            ["id", "kind", "type", "tags", "created", "updated"]
        );
    }

    #[test]
    fn an_unknown_kind_names_a_sort_of_the_folders_kind() {
        let doc = Doc::parse(
            path("library/sicp/annotations/p1.md"),
            "---\nkind: explain\n---\n",
        )
        .unwrap();
        assert_eq!(doc.kind(), Kind::Annotation);
        assert_eq!(doc.doc_type(), Some("explain"));
    }

    #[test]
    fn a_notes_title_is_its_file_name() {
        let mut doc = Doc::new_at(Kind::Note, path("notes/Physics/Waves.md"));
        assert_eq!(doc.title(), "Waves");
        doc.set_title("Waves: and light");
        assert_eq!(doc.path().as_str(), "notes/Physics/Waves and light.md");
    }

    #[test]
    fn new_ids_differ_and_path_ids_are_stable() {
        let ids: std::collections::HashSet<DocId> = (0..1000).map(|_| DocId::new()).collect();
        assert_eq!(ids.len(), 1000);
        let a = DocId::from_path(&path("notes/a.md"));
        assert_eq!(a, DocId::from_path(&path("notes/a.md")));
        assert_ne!(a, DocId::from_path(&path("notes/b.md")));
    }
}
