//! What a model is asked for: the JSON Schema of a sequence figure (strict: every field required,
//! nothing extra), the words that go with it, and one worked example.

use serde_json::{json, Value};

use crate::spec::Sequence;

/// The schema of `Sequence`, in the strict form structured-output calls take.
pub fn sequence_schema() -> Value {
    let text = json!({ "type": "string" });
    let number = json!({ "type": "number" });
    let tone =
        json!({ "type": "string", "enum": ["hi", "mid", "lo", "lit", "red", "green", "blue"] });
    let object = |kind: &str, fields: Vec<(&str, Value)>| {
        let mut properties = serde_json::Map::new();
        properties.insert("type".into(), json!({ "type": "string", "enum": [kind] }));
        properties.insert("id".into(), text.clone());
        for (name, schema) in fields {
            properties.insert(name.into(), schema);
        }
        let required: Vec<String> = properties.keys().cloned().collect();
        json!({ "type": "object", "properties": properties, "required": required, "additionalProperties": false })
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
    json!({
        "type": "object",
        "properties": {
            "title": text, "hint": text,
            "actors": { "type": "array", "items": {
                "type": "object",
                "properties": {
                    "id": text, "label": text, "sublabel": text,
                    "shape": { "type": "string", "enum": ["node", "client", "server", "database", "queue", "person", "process"] },
                },
                "required": ["id", "label", "sublabel", "shape"],
                "additionalProperties": false,
            }},
            "events": { "type": "array", "items": { "anyOf": events } },
            "time_label": text,
        },
        "required": ["title", "hint", "actors", "events", "time_label"],
        "additionalProperties": false,
    })
}

/// How to fill the schema in.
pub const GUIDE: &str = "\
You design one sequence figure: actors on vertical lifelines exchanging messages while time runs \
down the page. You give no positions, sizes or colours: the engine lays the figure out.

Rules:
- 2 to 6 actors, in the left-to-right order that keeps most arrows short. Labels of one or two words.
- ids are lowercase letters, digits and _, each used once.
- Times are numbers from 0 upward in one unit (say what it is in time_label, or leave it empty). \
A message leaves at `at` and lands at `arrive`, later than `at`. Leave about one unit between events \
on the same actor so their words do not collide.
- Message labels are a few words (\"PREPARE\", \"ack 2\"). A state's text is what the actor now holds \
(\"x = 1\", \"committed\"), under 20 characters.
- tone: mid for ordinary, hi for what matters, red for failure, green for success, blue for a second kind.
- lost: true for a message that never arrives. reply: true for an answer to an earlier message.
- narration: one sentence said when the event happens, in plain words. Empty when there is nothing to add.
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
