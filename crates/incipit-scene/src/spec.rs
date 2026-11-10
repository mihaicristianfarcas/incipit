//! What a model writes for a figure: who is in it and what happens when. No positions, sizes or
//! colours beyond a tone: those are the engine's.

use serde::{Deserialize, Serialize};

use crate::draw::Tone;

/// Actors (processes, servers, people) exchanging messages over time. Time runs down the page.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Sequence {
    pub title: String,
    /// One line under the title saying what to watch for.
    pub hint: String,
    pub actors: Vec<Actor>,
    pub events: Vec<Event>,
    /// The time axis's unit ("ms", "round"); empty hides it.
    pub time_label: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Actor {
    pub id: String,
    pub label: String,
    pub sublabel: String,
    pub shape: ActorShape,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ActorShape {
    Node,
    Client,
    Server,
    Database,
    Queue,
    Person,
    Process,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Event {
    /// An arrow from one actor to another, leaving at `at` and landing at `arrive`.
    Message {
        id: String,
        from: String,
        to: String,
        at: f32,
        arrive: f32,
        label: String,
        tone: Tone,
        /// Never arrives: the arrow stops part of the way with a cross.
        lost: bool,
        /// An answer to an earlier message: drawn dashed.
        reply: bool,
        narration: String,
    },
    /// A small box on an actor's line saying what it now holds or is.
    State {
        id: String,
        actor: String,
        at: f32,
        text: String,
        tone: Tone,
        narration: String,
    },
    /// A stretch of an actor's line where it is busy.
    Span {
        id: String,
        actor: String,
        at: f32,
        until: f32,
        label: String,
        tone: Tone,
        narration: String,
    },
    Crash {
        id: String,
        actor: String,
        at: f32,
        label: String,
        narration: String,
    },
    Recover {
        id: String,
        actor: String,
        at: f32,
        label: String,
        narration: String,
    },
}

impl Event {
    pub fn id(&self) -> &str {
        match self {
            Event::Message { id, .. }
            | Event::State { id, .. }
            | Event::Span { id, .. }
            | Event::Crash { id, .. }
            | Event::Recover { id, .. } => id,
        }
    }

    pub fn at(&self) -> f32 {
        match self {
            Event::Message { at, .. }
            | Event::State { at, .. }
            | Event::Span { at, .. }
            | Event::Crash { at, .. }
            | Event::Recover { at, .. } => *at,
        }
    }

    /// When the event is over: a message's arrival, a span's end, else when it happens.
    pub fn until(&self) -> f32 {
        match self {
            Event::Message { at, arrive, .. } => arrive.max(*at),
            Event::Span { at, until, .. } => until.max(*at),
            other => other.at(),
        }
    }

    pub fn narration(&self) -> &str {
        match self {
            Event::Message { narration, .. }
            | Event::State { narration, .. }
            | Event::Span { narration, .. }
            | Event::Crash { narration, .. }
            | Event::Recover { narration, .. } => narration,
        }
    }

    /// The actors the event names.
    pub fn actors(&self) -> Vec<&str> {
        match self {
            Event::Message { from, to, .. } => vec![from, to],
            Event::State { actor, .. }
            | Event::Span { actor, .. }
            | Event::Crash { actor, .. }
            | Event::Recover { actor, .. } => vec![actor],
        }
    }
}

impl Sequence {
    /// The last moment anything happens.
    pub fn end(&self) -> f32 {
        self.events.iter().map(Event::until).fold(1.0, f32::max)
    }

    /// The moments worth stopping at, in order: the start, and each event's start and end.
    pub fn stops(&self) -> Vec<f32> {
        let mut stops = vec![0.0];
        for e in &self.events {
            stops.push(e.at().max(0.0));
            stops.push(e.until().max(0.0));
        }
        stops.retain(|t| t.is_finite());
        stops.sort_by(f32::total_cmp);
        stops.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
        stops
    }
}
