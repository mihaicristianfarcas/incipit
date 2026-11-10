//! What a model is asked for to make a flow figure: the schema of its fields, the words that go
//! with it, and one worked example.

use serde_json::{json, Value};

use super::Flow;
use crate::schema::{boolean, fields, list, one_of, steps, text, tone};

/// The fields of `Flow` besides `kind`.
pub fn schema() -> Value {
    let node = fields(vec![
        ("id", text()),
        ("label", text()),
        (
            "shape",
            one_of(&["box", "round", "decision", "terminal", "circle"]),
        ),
        ("tone", tone()),
    ]);
    let edge = fields(vec![
        ("id", text()),
        ("from", text()),
        ("to", text()),
        ("label", text()),
        ("tone", tone()),
        ("dashed", boolean()),
    ]);
    fields(vec![
        ("title", text()),
        ("hint", text()),
        ("arrange", one_of(&["down", "right", "ring"])),
        ("nodes", list(node)),
        ("edges", list(edge)),
        ("steps", steps()),
    ])
}

pub const GUIDE: &str = "\
A flow figure: boxes (nodes) joined by arrows (edges). For processes, causes and effects, \
decisions, state machines, cycles and hierarchies.

Rules:
- arrange: \"down\" for a process, a hierarchy or a tree (layers from the top); \"right\" for a \
short pipeline or cause and effect (layers from the left); \"ring\" for a cycle of 3 to 8 nodes \
that comes back to its start, set clockwise from the top in the order you list them.
- 2 to 16 nodes. Each node sits one layer past the nodes that point to it, so the longest chain \
sets the number of layers: at most 6 for down, 4 for right. More side by side than about 5 does \
not fit across.
- A node's label is 1 to 5 words, at most about 40 characters; it wraps inside its shape. shape: \
box for a step or a thing, round for a state, decision for a question whose edges are its \
answers, terminal for a start or an end, circle for a node in a tree or network (one short word).
- An edge goes from one node id to another. Its label is a word or two saying what flows or why \
(\"yes\", \"heat\", \"on timeout\"), or empty. dashed: true for something optional or indirect. \
An edge back to an earlier node is fine: loops are drawn.
- List nodes in the order the flow goes; the engine orders each layer so arrows cross as little \
as they can.
- steps: build the figure up as a teacher would draw it, 3 to 8 steps. Each step's show lists \
the node and edge ids that appear at that step; anything no step shows is there from the start. \
focus lists the ids to look at during the step. Or give no steps.";

/// The scientific method as a loop: a question, a test, and back to the hypothesis when the
/// results do not support it.
pub fn example() -> Flow {
    serde_json::from_value(json!({
        "title": "The scientific method",
        "hint": "A hypothesis the results do not support goes back to be revised.",
        "arrange": "down",
        "nodes": [
            { "id": "question", "label": "Ask a question", "shape": "terminal", "tone": "mid" },
            { "id": "hypothesis", "label": "Form a hypothesis", "shape": "box", "tone": "hi" },
            { "id": "test", "label": "Test it with an experiment", "shape": "box", "tone": "mid" },
            { "id": "support", "label": "Do the results support it?", "shape": "decision", "tone": "mid" },
            { "id": "share", "label": "Share the conclusion", "shape": "terminal", "tone": "green" },
        ],
        "edges": [
            { "id": "e1", "from": "question", "to": "hypothesis", "label": "", "tone": "mid", "dashed": false },
            { "id": "e2", "from": "hypothesis", "to": "test", "label": "predicts", "tone": "mid", "dashed": false },
            { "id": "e3", "from": "test", "to": "support", "label": "data", "tone": "mid", "dashed": false },
            { "id": "yes", "from": "support", "to": "share", "label": "yes", "tone": "green", "dashed": false },
            { "id": "no", "from": "support", "to": "hypothesis", "label": "no: revise", "tone": "red", "dashed": true },
        ],
        "steps": [
            { "narration": "Science starts with a question about the world.", "show": ["question"], "focus": ["question"] },
            { "narration": "A hypothesis is a possible answer that could turn out wrong.",
              "show": ["hypothesis", "e1"], "focus": ["hypothesis"] },
            { "narration": "It predicts what an experiment will show, so we test it.",
              "show": ["test", "e2"], "focus": ["e2", "test"] },
            { "narration": "The data either support the hypothesis or they do not.",
              "show": ["support", "e3"], "focus": ["support"] },
            { "narration": "If they do, the conclusion is shared for others to check.",
              "show": ["share", "yes"], "focus": ["yes", "share"] },
            { "narration": "If not, the hypothesis is revised and tested again: the loop is the method.",
              "show": ["no"], "focus": ["no", "hypothesis"] },
        ],
    }))
    .expect("the example is a valid flow")
}
