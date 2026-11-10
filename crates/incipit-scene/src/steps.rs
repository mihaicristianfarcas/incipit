//! Steps shared by the kinds that build a figure up: each step says one sentence, may bring
//! elements in (`show`) and lights some (`focus`). An element no step shows is there from the
//! start; one a step shows appears at that step and stays. Positions never depend on the step, so
//! nothing jumps when the learner steps on: the layout places every element, then leaves out
//! what is not shown yet.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::figure::{Sample, State};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub narration: String,
    /// Elements that appear at this step.
    pub show: Vec<String>,
    /// Elements drawn as the accent during this step.
    pub focus: Vec<String>,
}

/// The steps as they stand at one step.
#[derive(Clone, Copy)]
pub struct At<'a> {
    steps: &'a [Step],
    at: usize,
}

impl<'a> At<'a> {
    pub fn new(steps: &'a [Step], state: &State) -> Self {
        At {
            steps,
            at: state.step.min(steps.len().saturating_sub(1)),
        }
    }

    /// Whether `id` is on the figure at this step.
    pub fn shown(&self, id: &str) -> bool {
        match self
            .steps
            .iter()
            .position(|s| s.show.iter().any(|x| x == id))
        {
            Some(i) => i <= self.at,
            None => true,
        }
    }

    /// Whether `id` came in at this very step.
    pub fn fresh(&self, id: &str) -> bool {
        self.steps
            .get(self.at)
            .is_some_and(|s| s.show.iter().any(|x| x == id))
    }

    pub fn lit(&self, id: &str) -> bool {
        self.steps
            .get(self.at)
            .is_some_and(|s| s.focus.iter().any(|x| x == id))
    }

    pub fn narration(&self) -> String {
        self.steps
            .get(self.at)
            .map(|s| s.narration.clone())
            .unwrap_or_default()
    }
}

/// Errors in steps that name elements: unknown ids, an element shown twice, a focus on what is not
/// shown yet, a step with nothing to say.
pub fn check(steps: &[Step], known: &BTreeSet<&str>, errors: &mut Vec<String>) {
    let mut shown: BTreeSet<&str> = BTreeSet::new();
    let introduced: BTreeSet<&str> = steps
        .iter()
        .flat_map(|s| s.show.iter().map(String::as_str))
        .collect();
    if steps.len() > 16 {
        errors.push(format!(
            "a figure holds at most 16 steps, this one has {}: show one idea",
            steps.len()
        ));
    }
    for (i, step) in steps.iter().enumerate() {
        let n = i + 1;
        if step.narration.trim().is_empty() {
            errors.push(format!("step {n} has no narration"));
        }
        for id in &step.show {
            crate::check::known_id(&format!("step {n}'s show"), id, known, errors);
            if !shown.insert(id) {
                errors.push(format!(
                    "step {n} shows \"{id}\", which an earlier step already showed"
                ));
            }
        }
        for id in &step.focus {
            crate::check::known_id(&format!("step {n}'s focus"), id, known, errors);
            if introduced.contains(id.as_str()) && !shown.contains(id.as_str()) {
                errors.push(format!(
                    "step {n} focuses \"{id}\", which is not shown until a later step"
                ));
            }
        }
    }
}

/// One sample for each step, or the one state when there are none.
pub fn samples(steps: usize) -> Vec<Sample> {
    if steps == 0 {
        return vec![Sample {
            state: State::default(),
            place: String::new(),
        }];
    }
    (0..steps)
        .map(|i| Sample {
            state: State {
                step: i,
                ..State::default()
            },
            place: format!("at step {}", i + 1),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(show: &[&str], focus: &[&str]) -> Step {
        Step {
            narration: "Something happens.".into(),
            show: show.iter().map(|s| s.to_string()).collect(),
            focus: focus.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn an_element_appears_at_its_step_and_stays() {
        let steps = vec![step(&[], &["a"]), step(&["b"], &["b"]), step(&[], &[])];
        let at = |i| {
            At::new(
                &steps,
                &State {
                    step: i,
                    ..State::default()
                },
            )
        };
        assert!(at(0).shown("a") && !at(0).shown("b"));
        assert!(at(1).shown("b") && at(1).fresh("b") && at(1).lit("b"));
        assert!(at(2).shown("b") && !at(2).fresh("b"));
    }

    #[test]
    fn a_focus_before_its_element_appears_is_refused() {
        let steps = vec![step(&[], &["b"]), step(&["b"], &[])];
        let known = BTreeSet::from(["a", "b"]);
        let mut errors = Vec::new();
        check(&steps, &known, &mut errors);
        assert!(
            errors.iter().any(|e| e.contains("not shown until")),
            "{errors:?}"
        );
    }
}
