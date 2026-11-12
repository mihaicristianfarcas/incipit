//! The activity log, `log/activity.jsonl`: one line for each thing the learner did, which the
//! journey, the reading place and the pace through a book are read from. It is the earlier app's
//! log with the same fields, except that the time is `at`, an RFC 3339 time as in the review log
//! (the earlier app's `ts` was milliseconds, and its lines are not read).
//!
//! Lines are only ever added at the end; a line that cannot be read is skipped.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::Result;
use crate::store::Vault;

/// The log's name: `log/activity.jsonl`.
pub const ACTIVITY: &str = "activity";

/// What kind of thing was done.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActivityType {
    /// A sitting with a book.
    Read,
    Review,
    /// A note was written.
    Note,
    Highlight,
    /// Flashcards were made.
    Card,
    /// A figure was made.
    Figure,
    Quiz,
    /// A concept was explained back.
    Explain,
    /// Links between notes or concepts were made or suggested.
    Connect,
    /// The tutor or the agent was asked.
    Ask,
    /// A chapter was mapped into concepts.
    Concept,
    /// A book was added.
    Book,
    /// The streak was paused (`data` holds the first and last day); not studying.
    Pause,
}

/// One thing the learner did.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Activity {
    /// When, in RFC 3339.
    pub at: String,
    #[serde(rename = "type")]
    pub kind: ActivityType,
    /// The book it was about, if any (its id).
    #[serde(default, rename = "bookId", skip_serializing_if = "Option::is_none")]
    pub book: Option<String>,
    /// The ids of the concepts it was about.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub concepts: Vec<String>,
    /// A short summary in plain words ("Read 12 min of Calculus Made Easy").
    pub text: String,
    /// More, by type (a sitting's minutes and position, a quiz's score).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Map<String, Value>>,
}

/// An RFC 3339 time, to the millisecond, from milliseconds since the epoch.
pub fn rfc3339(millis: i64) -> String {
    chrono::DateTime::from_timestamp_millis(millis)
        .unwrap_or_default()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

impl Activity {
    /// An entry made at `millis` (since the epoch) with a summary.
    pub fn new(kind: ActivityType, millis: i64, text: impl Into<String>) -> Activity {
        Activity {
            at: rfc3339(millis),
            kind,
            book: None,
            concepts: Vec::new(),
            text: text.into(),
            data: None,
        }
    }

    /// About a book.
    pub fn book(mut self, id: impl AsRef<str>) -> Activity {
        self.book = Some(id.as_ref().to_owned());
        self
    }

    /// About concepts.
    pub fn concepts<I: IntoIterator<Item = S>, S: Into<String>>(mut self, ids: I) -> Activity {
        self.concepts = ids.into_iter().map(Into::into).collect();
        self
    }

    /// With more: `data` must be a JSON object, anything else is left out.
    pub fn data(mut self, data: Value) -> Activity {
        self.data = match data {
            Value::Object(map) => Some(map),
            _ => None,
        };
        self
    }

    /// When, in milliseconds since the epoch; `None` for a time that is not RFC 3339.
    pub fn millis(&self) -> Option<i64> {
        chrono::DateTime::parse_from_rfc3339(&self.at)
            .ok()
            .map(|t| t.timestamp_millis())
    }

    /// A number in `data`.
    pub fn number(&self, key: &str) -> Option<f64> {
        self.data.as_ref()?.get(key)?.as_f64()
    }

    /// A text in `data`.
    pub fn text_of(&self, key: &str) -> Option<&str> {
        self.data.as_ref()?.get(key)?.as_str()
    }
}

impl Vault {
    /// Adds an entry to the activity log.
    pub fn log_activity(&self, entry: &Activity) -> Result<()> {
        self.append_log(ACTIVITY, entry)
    }

    /// Every entry of the activity log, oldest first; lines that cannot be read are skipped.
    pub fn activity(&self) -> Result<Vec<Activity>> {
        Ok(self.read_log::<Activity>(ACTIVITY)?.entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn vault() -> (tempfile::TempDir, Vault) {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(&folder.path().join("vault")).unwrap();
        (folder, vault)
    }

    #[test]
    fn an_entry_is_one_line_with_the_earlier_apps_fields_and_an_rfc_3339_time() {
        let entry = Activity::new(ActivityType::Read, 1_700_000_000_000, "Read 24 min of A")
            .book("b1")
            .concepts(["c1"])
            .data(json!({ "minutes": 24, "position": 3.5 }));
        let line = serde_json::to_value(&entry).unwrap();
        assert_eq!(
            line,
            json!({
                "at": "2023-11-14T22:13:20.000Z",
                "type": "read",
                "bookId": "b1",
                "concepts": ["c1"],
                "text": "Read 24 min of A",
                "data": { "minutes": 24, "position": 3.5 },
            })
        );
        assert_eq!(entry.millis(), Some(1_700_000_000_000));
        assert_eq!(entry.number("position"), Some(3.5));
    }

    #[test]
    fn what_is_empty_is_left_out_of_the_line() {
        let line = serde_json::to_value(Activity::new(ActivityType::Pause, 0, "Paused")).unwrap();
        assert_eq!(line.as_object().unwrap().len(), 3);
    }

    #[test]
    fn every_type_has_the_earlier_apps_name() {
        use ActivityType::*;
        let names = [
            (Read, "read"),
            (Review, "review"),
            (Note, "note"),
            (Highlight, "highlight"),
            (Card, "card"),
            (Figure, "figure"),
            (Quiz, "quiz"),
            (Explain, "explain"),
            (Connect, "connect"),
            (Ask, "ask"),
            (Concept, "concept"),
            (Book, "book"),
            (Pause, "pause"),
        ];
        for (kind, name) in names {
            assert_eq!(serde_json::to_value(kind).unwrap(), json!(name));
        }
    }

    #[test]
    fn entries_come_back_in_order_and_unreadable_lines_are_skipped() {
        let (_folder, vault) = vault();
        vault
            .log_activity(&Activity::new(ActivityType::Book, 1000, "Added A"))
            .unwrap();
        let path = vault.root().join("log/activity.jsonl");
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("{not json\n{\"ts\":5,\"type\":\"read\",\"text\":\"old\"}\n");
        std::fs::write(&path, text).unwrap();
        vault
            .log_activity(&Activity::new(ActivityType::Note, 2000, "Wrote: B"))
            .unwrap();
        let all = vault.activity().unwrap();
        let texts: Vec<_> = all.iter().map(|a| a.text.as_str()).collect();
        assert_eq!(texts, ["Added A", "Wrote: B"]);
    }

    #[test]
    fn a_vault_with_no_log_has_no_activity() {
        let (_folder, vault) = vault();
        assert!(vault.activity().unwrap().is_empty());
    }
}
