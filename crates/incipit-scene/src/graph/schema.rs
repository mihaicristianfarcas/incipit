//! What a model is asked for to make a graph figure: the schema of its fields, the words that go
//! with it, and one worked example.

use serde_json::{json, Value};

use super::Graph;
use crate::schema::{boolean, fields, list, number, one_of, steps, text, texts, tone};

/// The fields of `Graph` besides `kind`.
pub fn schema() -> Value {
    let node = fields(vec![
        ("id", text()),
        ("label", text()),
        ("sublabel", text()),
        (
            "shape",
            one_of(&["circle", "box", "pill", "diamond", "database", "cell"]),
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
        ("arrow", boolean()),
    ]);
    let token = fields(vec![
        ("id", text()),
        ("path", texts()),
        ("at", number()),
        ("speed", number()),
        ("label", text()),
        ("tone", tone()),
    ]);
    let group = fields(vec![("id", text()), ("label", text()), ("nodes", texts())]);
    fields(vec![
        ("title", text()),
        ("hint", text()),
        ("layout", one_of(&["force", "circle", "row", "grid"])),
        ("nodes", list(node)),
        ("edges", list(edge)),
        ("tokens", list(token)),
        ("groups", list(group)),
        ("steps", steps()),
    ])
}

pub const GUIDE: &str = "\
A graph figure: a network of nodes and the links between them, with tokens that travel along \
the links. For computer networks, peers and servers, people and who knows whom, a hash ring, an \
array or a log, and anything that moves through a network: requests, packets, copies, goods, \
money. For a process, a hierarchy, a tree or a state machine, use a flow figure instead.

Rules:
- layout: \"force\" for a general network (linked nodes pulled together, the rest pushed \
apart); \"circle\" for nodes evenly round a ring, clockwise from the top in the order listed \
(gossip, a token ring, a hash ring); \"row\" for an array, a log or a queue, left to right in \
the order listed (shape cell for the slots of an array); \"grid\" for rows of like nodes.
- 2 to 16 nodes; 3 to 9 is typical. A label is a word or two, at most about 16 characters; a \
circle holds about 8. sublabel: a short value or state under the label (\"v3\", \"leader\", \
\"42\"), or empty. shape: circle for things, box for components, pill for states, diamond for \
decisions, database for stores of data, cell for the slots of an array.
- An edge joins two node ids. arrow: true when it has a direction. dashed: true for a weaker or \
later link. label: one or two words, only where the link's meaning is not plain, or empty. Two \
edges between the same pair bend apart by themselves; an edge from a node to itself loops over it.
- tokens: dots that travel through path (node ids, in order, at least two), leaving the first at \
`at` seconds and taking `speed` seconds for each hop (0.5 to 2 reads well); label a word or two, \
or empty. A graph with tokens plays on a clock that runs until the last arrives, and has no \
steps. Give tokens to show something moving through the network; otherwise give none.
- groups: a box round related nodes (a data centre, a partition, a region), with a short label. \
The box must not take in nodes outside the group, so with groups prefer row or grid and list \
each group's nodes together.
- steps (only without tokens): build the figure up, 3 to 8 steps. Each step's show lists the \
node, edge and group ids that appear at that step; anything no step shows is there from the \
start. focus lists the ids to look at during the step. Or give no steps.";

/// A write reaching every copy: the client's write goes to the leader, which sends it on to the
/// two followers, and the nearer one answers first.
pub fn example() -> Graph {
    serde_json::from_value(json!({
        "title": "A write reaches every copy",
        "hint": "The leader takes the write, then copies it to each follower.",
        "layout": "force",
        "nodes": [
            { "id": "client", "label": "Client", "sublabel": "", "shape": "circle", "tone": "mid" },
            { "id": "leader", "label": "Leader", "sublabel": "v2", "shape": "database", "tone": "hi" },
            { "id": "near", "label": "Follower A", "sublabel": "v2", "shape": "database", "tone": "mid" },
            { "id": "far", "label": "Follower B", "sublabel": "v1", "shape": "database", "tone": "mid" },
        ],
        "edges": [
            { "id": "write", "from": "client", "to": "leader", "label": "write", "tone": "mid", "dashed": false, "arrow": true },
            { "id": "to_near", "from": "leader", "to": "near", "label": "", "tone": "mid", "dashed": false, "arrow": true },
            { "id": "to_far", "from": "leader", "to": "far", "label": "slow link", "tone": "mid", "dashed": true, "arrow": true },
        ],
        "tokens": [
            { "id": "w", "path": ["client", "leader"], "at": 0, "speed": 1, "label": "x = 5", "tone": "lit" },
            { "id": "copy_a", "path": ["leader", "near"], "at": 1, "speed": 0.8, "label": "", "tone": "blue" },
            { "id": "copy_b", "path": ["leader", "far"], "at": 1, "speed": 2, "label": "", "tone": "blue" },
        ],
        "groups": [],
        "steps": [],
    }))
    .expect("the example is a valid graph")
}
