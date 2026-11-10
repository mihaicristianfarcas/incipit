//! What the links in a vault point at: every link resolved to a document, the backlinks of each
//! document, and the edits that carry links along when documents move.
//!
//! A Wiki link's target resolves, in order, to: a document at that path from the root (`.md`
//! optional), or one whose path ends with it (`[[Physics/Waves]]`); one with that file name (the
//! one in the linking document's folder first, then the shortest path); one with that id; one
//! with that title (cards, books); one with that alias. A Markdown link's target is a path
//! relative to the linking document, or else from the root. Case does not matter. Links in
//! properties count too (`about: ["[[Derivative]]"]`), as they do in Obsidian.
//!
//! The graph is rebuilt from the documents when it is asked for after a change: parsing every
//! body is a few tens of milliseconds for thousands of notes, and it is never stale.

use std::collections::{HashMap, HashSet};

use crate::doc::{Doc, DocId};
use crate::links::{self, Link, LinkStyle};
use crate::path::VaultPath;
use crate::value::Value;

/// Where in a document a link is written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    Body,
    /// In the value of this property.
    Property(String),
}

/// A link into a document.
#[derive(Clone, Debug, PartialEq)]
pub struct Backlink {
    pub from: DocId,
    pub link: Link,
    pub place: Place,
}

/// A link out of a document, and the document it reaches (`None`: nothing has that name yet).
#[derive(Clone, Debug, PartialEq)]
pub struct Outlink {
    pub link: Link,
    pub place: Place,
    pub to: Option<DocId>,
}

/// The key a name is matched by: lowercase, spaces folded, without `.md`.
fn key(name: &str) -> String {
    let name = name.trim();
    let name = name.strip_suffix(".md").unwrap_or(name);
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[derive(Default)]
struct Names {
    /// Path key (`notes/physics/waves`) to document.
    paths: HashMap<String, DocId>,
    stems: HashMap<String, Vec<DocId>>,
    ids: HashSet<DocId>,
    titles: HashMap<String, Vec<DocId>>,
    aliases: HashMap<String, Vec<DocId>>,
    where_is: HashMap<DocId, VaultPath>,
}

pub(crate) struct Graph {
    names: Names,
    outgoing: HashMap<DocId, Vec<Outlink>>,
    incoming: HashMap<DocId, Vec<Backlink>>,
}

/// Every link in a document: in its body, then in its properties.
fn links_of(doc: &Doc) -> Vec<(Link, Place)> {
    let mut out: Vec<(Link, Place)> = links::parse(doc.body())
        .into_iter()
        .map(|l| (l, Place::Body))
        .collect();
    for (key, value) in doc.properties().iter() {
        let mut texts = Vec::new();
        value.texts(&mut texts);
        for text in texts {
            for link in links::parse(text) {
                out.push((link, Place::Property(key.to_owned())));
            }
        }
    }
    out
}

/// A path written relative to a folder, with `..` and `.` worked out; `None` if it climbs out.
fn join_relative(folder: Option<&VaultPath>, target: &str) -> Option<String> {
    let mut parts: Vec<&str> = folder.map(|f| f.parts().collect()).unwrap_or_default();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// `to` as a path relative to the folder of `from`.
fn relative(from: &VaultPath, to: &VaultPath) -> String {
    let mut from_dirs: Vec<&str> = from.parts().collect();
    from_dirs.pop();
    let to_parts: Vec<&str> = to.parts().collect();
    let common = from_dirs
        .iter()
        .zip(&to_parts)
        .take_while(|(a, b)| a == b)
        .count();
    let mut out: Vec<&str> = vec![".."; from_dirs.len() - common];
    out.extend(&to_parts[common..]);
    out.join("/")
}

impl Graph {
    pub(crate) fn build<'a>(docs: impl Iterator<Item = &'a Doc> + Clone) -> Graph {
        let mut names = Names::default();
        for doc in docs.clone() {
            let id = doc.id().clone();
            names.paths.insert(key(doc.path().as_str()), id.clone());
            names
                .stems
                .entry(key(doc.path().stem()))
                .or_default()
                .push(id.clone());
            names.ids.insert(id.clone());
            if !doc.kind().title_is_file_name() {
                names
                    .titles
                    .entry(key(&doc.title()))
                    .or_default()
                    .push(id.clone());
            }
            for alias in doc.aliases() {
                names
                    .aliases
                    .entry(key(&alias))
                    .or_default()
                    .push(id.clone());
            }
            names.where_is.insert(id, doc.path().clone());
        }
        let mut graph = Graph {
            names,
            outgoing: HashMap::new(),
            incoming: HashMap::new(),
        };
        for doc in docs {
            let mut out = Vec::new();
            for (link, place) in links_of(doc) {
                let to = graph.resolve(&link, doc.path());
                if let Some(to) = to.as_ref().filter(|to| *to != doc.id()) {
                    graph
                        .incoming
                        .entry(to.clone())
                        .or_default()
                        .push(Backlink {
                            from: doc.id().clone(),
                            link: link.clone(),
                            place: place.clone(),
                        });
                }
                out.push(Outlink { link, place, to });
            }
            graph.outgoing.insert(doc.id().clone(), out);
        }
        graph
    }

    /// Of several documents with one name, the one a link from `from` means: the one in the
    /// same folder, else the one with the shortest path, else the first by path.
    fn closest(&self, candidates: &[DocId], from: &VaultPath) -> Option<DocId> {
        let folder = from.parent();
        candidates
            .iter()
            .min_by_key(|id| {
                let path = &self.names.where_is[*id];
                (path.parent() != folder, path.as_str().len(), path.clone())
            })
            .cloned()
    }

    /// The document a link written in the document at `from` points at.
    pub(crate) fn resolve(&self, link: &Link, from: &VaultPath) -> Option<DocId> {
        let names = &self.names;
        if link.target.is_empty() {
            // `[[#Heading]]`: a place in the same document.
            return names.paths.get(&key(from.as_str())).cloned();
        }
        if link.style == LinkStyle::Markdown {
            let joined = join_relative(from.parent().as_ref(), &link.target);
            let from_root = join_relative(None, &link.target);
            return [joined, from_root]
                .into_iter()
                .flatten()
                .find_map(|p| names.paths.get(&key(&p)).cloned());
        }
        let target = key(&link.target);
        if let Some(id) = names.paths.get(&target) {
            return Some(id.clone());
        }
        if let Some(id) = names.paths.get(&format!("{target}/book")) {
            return Some(id.clone());
        }
        if target.contains('/') {
            let suffix = format!("/{target}");
            let ends: Vec<DocId> = names
                .paths
                .iter()
                .filter(|(path, _)| path.ends_with(&suffix))
                .map(|(_, id)| id.clone())
                .collect();
            return self.closest(&ends, from);
        }
        if let Some(found) = names.stems.get(&target) {
            return self.closest(found, from);
        }
        let as_id = DocId::from(link.target.trim());
        if names.ids.contains(&as_id) {
            return Some(as_id);
        }
        if let Some(found) = names.titles.get(&target) {
            return self.closest(found, from);
        }
        names
            .aliases
            .get(&target)
            .and_then(|found| self.closest(found, from))
    }

    pub(crate) fn backlinks(&self, id: &DocId) -> Vec<Backlink> {
        self.incoming.get(id).cloned().unwrap_or_default()
    }

    pub(crate) fn outlinks(&self, id: &DocId) -> Vec<Outlink> {
        self.outgoing.get(id).cloned().unwrap_or_default()
    }

    /// Links that point at nothing yet, with the document each is written in.
    pub(crate) fn unresolved(&self) -> Vec<(DocId, Link)> {
        let mut out: Vec<(DocId, Link)> = self
            .outgoing
            .iter()
            .flat_map(|(from, links)| {
                links
                    .iter()
                    .filter(|l| l.to.is_none())
                    .map(move |l| (from.clone(), l.link.clone()))
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.span.start.cmp(&b.1.span.start)));
        out
    }

    /// The documents whose text must change when documents move (`moves`: id to new path), with
    /// their new body and properties: links to a moved document follow it, and a moved
    /// document's relative Markdown links still reach what they reached.
    pub(crate) fn carry_links<'a>(
        &self,
        docs: impl Iterator<Item = &'a Doc>,
        moves: &HashMap<DocId, VaultPath>,
    ) -> Vec<Doc> {
        let new_path = |id: &DocId| moves.get(id).or_else(|| self.names.where_is.get(id));
        let mut changed = Vec::new();
        for doc in docs {
            let from_old = doc.path().clone();
            let from_new = moves.get(doc.id()).cloned().unwrap_or(from_old.clone());
            let moved_self = from_new != from_old;
            let mut target_for = |link: &Link| -> Option<String> {
                let to = self.resolve(link, &from_old)?;
                let to_moved = moves.contains_key(&to);
                let relative_md =
                    link.style == LinkStyle::Markdown && !link.target.starts_with('/');
                if !to_moved && !(moved_self && relative_md) {
                    return None;
                }
                let to_path = new_path(&to)?;
                Some(match link.style {
                    LinkStyle::Markdown => relative(&from_new, to_path),
                    LinkStyle::Wiki if link.target.is_empty() => return None,
                    LinkStyle::Wiki => {
                        let stem_taken = self
                            .names
                            .stems
                            .get(&key(to_path.stem()))
                            .is_some_and(|ids| ids.iter().any(|other| *other != to));
                        if link.target.contains('/') || stem_taken {
                            to_path
                                .as_str()
                                .strip_suffix(".md")
                                .unwrap_or(to_path.as_str())
                                .to_owned()
                        } else {
                            to_path.stem().to_owned()
                        }
                    }
                })
            };
            let body = links::retarget(doc.body(), &mut target_for);
            let mut properties = doc.properties().clone();
            let mut properties_changed = false;
            let keys: Vec<String> = properties.keys().map(str::to_owned).collect();
            for key in keys {
                let old = properties.get(&key).cloned().unwrap_or(Value::Null);
                let new = old.map_texts(&mut |text| links::retarget(text, &mut target_for));
                if new != old {
                    properties.set(&key, new);
                    properties_changed = true;
                }
            }
            if body != doc.body() || properties_changed {
                let mut edited = doc.clone();
                edited.set_body(body);
                if properties_changed {
                    *edited.properties_mut() = properties;
                }
                changed.push(edited);
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(path: &str, text: &str) -> Doc {
        Doc::parse(VaultPath::new(path).unwrap(), text).unwrap()
    }

    fn graph(docs: &[Doc]) -> Graph {
        Graph::build(docs.iter())
    }

    fn resolve(graph: &Graph, from: &str, text: &str) -> Option<String> {
        let link = &links::parse(text)[0];
        let to = graph.resolve(link, &VaultPath::new(from).unwrap())?;
        Some(graph.names.where_is[&to].to_string())
    }

    #[test]
    fn a_link_resolves_by_path_name_title_and_alias() {
        let docs = [
            doc(
                "notes/Physics/Waves.md",
                "---\naliases: [Oscillation]\n---\n",
            ),
            doc("notes/Chemistry/Waves.md", ""),
            doc("cards/c1.md", "---\ntitle: The speed of light\n---\n"),
            doc("library/sicp/book.md", "---\ntitle: SICP\n---\n"),
        ];
        let g = graph(&docs);
        let from = "notes/Physics/Index.md";
        assert_eq!(
            resolve(&g, from, "[[notes/Chemistry/Waves]]").as_deref(),
            Some("notes/Chemistry/Waves.md")
        );
        assert_eq!(
            resolve(&g, from, "[[Chemistry/Waves]]").as_deref(),
            Some("notes/Chemistry/Waves.md")
        );
        assert_eq!(
            resolve(&g, from, "[[waves]]").as_deref(),
            Some("notes/Physics/Waves.md")
        );
        assert_eq!(
            resolve(&g, from, "[[The Speed of Light]]").as_deref(),
            Some("cards/c1.md")
        );
        assert_eq!(
            resolve(&g, from, "[[Oscillation]]").as_deref(),
            Some("notes/Physics/Waves.md")
        );
        assert_eq!(
            resolve(&g, from, "[[library/sicp]]").as_deref(),
            Some("library/sicp/book.md")
        );
        assert_eq!(
            resolve(&g, from, "[x](../Chemistry/Waves.md)").as_deref(),
            Some("notes/Chemistry/Waves.md")
        );
        assert_eq!(resolve(&g, from, "[[Nothing yet]]"), None);
    }

    #[test]
    fn backlinks_come_from_bodies_and_properties() {
        let docs = [
            doc("concepts/Derivative.md", ""),
            doc("notes/A.md", "See [[Derivative]].\n"),
            doc("notes/B.md", "---\nabout: [\"[[Derivative]]\"]\n---\n"),
            doc("notes/C.md", "[[Missing]]\n"),
        ];
        let g = graph(&docs);
        let back = g.backlinks(docs[0].id());
        assert_eq!(back.len(), 2);
        assert!(back
            .iter()
            .any(|b| b.place == Place::Property("about".into())));
        assert_eq!(g.unresolved().len(), 1);
    }

    #[test]
    fn moving_a_document_carries_the_links_to_it() {
        let docs = [
            doc("notes/Old.md", "Self: [[#Top]]\n[up](Other.md)\n"),
            doc(
                "notes/Other.md",
                "[[Old|shown]] [[notes/Old]] [md](Old.md#Part) ![[Old]]\n",
            ),
            doc(
                "notes/Deep/Third.md",
                "---\nrelated: [\"[[Old]]\"]\n---\n[x](../Old.md)\n",
            ),
        ];
        let g = graph(&docs);
        let moves = HashMap::from([(
            docs[0].id().clone(),
            VaultPath::new("notes/Archive/New name.md").unwrap(),
        )]);
        let changed = g.carry_links(docs.iter(), &moves);
        let by_path: HashMap<&str, &Doc> = changed.iter().map(|d| (d.path().as_str(), d)).collect();
        assert_eq!(
            by_path["notes/Other.md"].body(),
            "[[New name|shown]] [[notes/Archive/New name]] [md](Archive/New%20name.md#Part) ![[New name]]\n"
        );
        assert_eq!(
            by_path["notes/Deep/Third.md"].body(),
            "[x](../Archive/New%20name.md)\n"
        );
        assert_eq!(
            by_path["notes/Deep/Third.md"].related(),
            vec!["[[New name]]"]
        );
        // The moved note's own relative link still reaches its neighbour.
        assert_eq!(
            by_path["notes/Old.md"].body(),
            "Self: [[#Top]]\n[up](../Other.md)\n"
        );
    }
}
