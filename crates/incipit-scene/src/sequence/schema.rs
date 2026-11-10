//! What a model is asked for to make a sequence figure: the JSON Schema of its fields (strict: every
//! field required, nothing extra), the words that go with it, and one worked example.

use serde_json::{json, Value};

use super::spec::Sequence;
use crate::schema::{fields, tone, variant};

/// The fields of `Sequence` besides `kind`, in the strict form structured-output calls take.
pub fn schema() -> Value {
    let text = json!({ "type": "string" });
    let number = json!({ "type": "number" });
    let tone = tone();
    let object = |kind: &str, mut fields: Vec<(&str, Value)>| {
        fields.insert(0, ("id", text.clone()));
        variant(kind, fields)
    };
    let events = json!([
        object(
            "message",
            vec![
                ("from", text.clone()),
                ("to", text.clone()),
                ("at", number.clone()),
                ("arrive", number.clone()),
                ("label", text.clone()),
                ("tone", tone.clone()),
                ("lost", json!({ "type": "boolean" })),
                ("reply", json!({ "type": "boolean" })),
                ("narration", text.clone()),
            ]
        ),
        object(
            "state",
            vec![
                ("actor", text.clone()),
                ("at", number.clone()),
                ("text", text.clone()),
                ("tone", tone.clone()),
                ("narration", text.clone()),
            ]
        ),
        object(
            "span",
            vec![
                ("actor", text.clone()),
                ("at", number.clone()),
                ("until", number.clone()),
                ("label", text.clone()),
                ("tone", tone.clone()),
                ("narration", text.clone()),
            ]
        ),
        object(
            "crash",
            vec![
                ("actor", text.clone()),
                ("at", number.clone()),
                ("label", text.clone()),
                ("narration", text.clone()),
            ]
        ),
        object(
            "recover",
            vec![
                ("actor", text.clone()),
                ("at", number.clone()),
                ("label", text.clone()),
                ("narration", text.clone()),
            ]
        ),
    ]);
    let actor = fields(vec![
        ("id", text.clone()),
        ("label", text.clone()),
        ("sublabel", text.clone()),
        (
            "shape",
            json!({ "type": "string", "enum": ["node", "client", "server", "database", "queue", "person", "process"] }),
        ),
    ]);
    fields(vec![
        ("title", text.clone()),
        ("hint", text.clone()),
        ("actors", json!({ "type": "array", "items": actor })),
        (
            "events",
            json!({ "type": "array", "items": { "anyOf": events } }),
        ),
        ("time_label", text),
    ])
}

/// How to fill the schema in.
pub const GUIDE: &str = "\
A sequence figure: actors on vertical lifelines exchanging messages while time runs down the page.

Rules:
- 2 to 6 actors, in the left-to-right order that keeps most arrows short. Labels of one or two words.
- Times are numbers from 0 upward in one unit (say what it is in time_label, or leave it empty). \
A message leaves at `at` and lands at `arrive`, later than `at`. Leave about one unit between events \
on the same actor so their words do not collide.
- Message labels are a few words (\"PREPARE\", \"ack 2\"). A state's text is what the actor now holds \
(\"x = 1\", \"committed\"), under 20 characters.
- lost: true for a message that never arrives. reply: true for an answer to an earlier message.
- narration: one sentence said when the event happens. Empty when there is nothing to add.
- Show one idea in at most 14 events.";

/// A small clean figure: a request, a write, and the answers.
pub fn example() -> Sequence {
    serde_json::from_value(json!({
        "title": "A write with one replica",
        "hint": "The client hears back only after the replica has the write.",
        "time_label": "",
        "actors": [
            { "id": "c", "label": "Client", "sublabel": "", "shape": "client" },
            { "id": "a", "label": "Leader", "sublabel": "node A", "shape": "server" },
            { "id": "b", "label": "Follower", "sublabel": "node B", "shape": "database" },
        ],
        "events": [
            { "type": "message", "id": "put", "from": "c", "to": "a", "at": 0.5, "arrive": 1.5, "label": "put x = 1",
              "tone": "hi", "lost": false, "reply": false, "narration": "The client sends its write to the leader." },
            { "type": "state", "id": "sa", "actor": "a", "at": 2.0, "text": "x = 1", "tone": "mid",
              "narration": "The leader writes it to its own log." },
            { "type": "message", "id": "rep", "from": "a", "to": "b", "at": 2.5, "arrive": 3.5, "label": "replicate",
              "tone": "mid", "lost": false, "reply": false, "narration": "Then it sends the write to the follower." },
            { "type": "state", "id": "sb", "actor": "b", "at": 4.0, "text": "x = 1", "tone": "mid", "narration": "" },
            { "type": "message", "id": "ack", "from": "b", "to": "a", "at": 4.5, "arrive": 5.5, "label": "ack",
              "tone": "mid", "lost": false, "reply": true, "narration": "The follower confirms it has the write." },
            { "type": "message", "id": "ok", "from": "a", "to": "c", "at": 6.0, "arrive": 7.0, "label": "ok",
              "tone": "green", "lost": false, "reply": true, "narration": "Only now does the client hear that the write is safe." },
        ],
    }))
    .expect("the example is a valid sequence")
}
