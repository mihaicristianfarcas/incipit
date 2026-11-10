//! The sequence figure: who tells whom what, over time. Actors stand on vertical lifelines and
//! messages travel between them while a clock runs down the page; states, busy spans, crashes and
//! recoveries sit on the lifelines. It is the one kind driven by a clock rather than steps.

mod check;
mod layout;
mod schema;
mod spec;

pub use layout::layout;
pub use schema::{example, schema, GUIDE};
pub use spec::{Actor, ActorShape, Event, Sequence};

use crate::draw::{Frame, Measure};
use crate::figure::{Pace, Sample, Spec, State};

impl Spec for Sequence {
    fn title(&self) -> &str {
        &self.title
    }

    fn hint(&self) -> &str {
        &self.hint
    }

    fn pace(&self) -> Pace {
        Pace::Clock {
            end: self.end(),
            stops: self.stops(),
        }
    }

    /// At the end, so the whole exchange is in view before it is played.
    fn start(&self) -> State {
        State {
            time: self.end(),
            ..State::default()
        }
    }

    fn layout(&self, state: &State, m: &dyn Measure) -> Frame {
        layout(self, state.time, state.focus.as_deref(), m)
    }

    fn structure(&self) -> Vec<String> {
        check::structure(self)
    }

    fn samples(&self) -> Vec<Sample> {
        check::moments(self)
            .into_iter()
            .map(|time| Sample {
                state: State {
                    time,
                    ..State::default()
                },
                place: format!("at time {}", crate::ticks::number(time)),
            })
            .collect()
    }
}
