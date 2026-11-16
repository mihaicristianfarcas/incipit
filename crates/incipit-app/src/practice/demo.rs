//! A made-up practice, for looking at the sheet without a model: INCIPIT_PRACTICE (see the
//! module above) opens it on the example, a problem, a problem missed, the easier problem or a finished
//! practice.

use incipit_learn::practice::Given;

use super::*;

impl PracticeView {
    /// INCIPIT_PRACTICE=demo, problem, missed, easier or done: a made-up practice, nothing asked
    /// of a model and nothing written.
    pub fn show_demo(&mut self, which: &str, cx: &mut Context<Self>) {
        self.open_on(
            About {
                concept: Some("Attention".into()),
                passage: Some(
                    "An attention function maps a query and a set of key-value pairs to an output."
                        .into(),
                ),
                ..About::default()
            },
            cx,
        );
        self.show_example(made_up(), cx);
        if let Some(open) = self.open.as_mut() {
            open.demo = true;
        }
        if which == "demo" {
            return;
        }
        self.begin(cx);
        let wrong = "That is the weight before the softmax.";
        let given = |score: f64| Given {
            score,
            unchecked: false,
        };
        match which {
            "missed" => self.answered("0.6", given(0.0), wrong, cx),
            "easier" | "done" => {
                // Three misses bring the easier problem; two right answers then finish it.
                for text in ["0.6", "0.7", "0.9"] {
                    self.answered(text, given(0.0), wrong, cx);
                    self.next(cx);
                }
                if which == "done" {
                    for text in ["1", "0.25"] {
                        self.answered(text, given(1.0), "", cx);
                        self.next(cx);
                    }
                }
            }
            _ => {}
        }
        cx.notify();
    }
}

/// A worked example and problems that were never written by a model.
pub(super) fn made_up() -> Practice {
    let problem = |question: &str, answer: &str| Problem {
        problem: question.into(),
        answer: answer.into(),
        accept: vec![format!("{answer}.0")],
        steps: vec![
            practice::Step {
                label: "Score each pair".into(),
                work: "Multiply each query by each key: $q \\cdot k$.".into(),
            },
            practice::Step {
                label: "Turn scores into weights".into(),
                work: "Apply the softmax, so the weights add up to one.".into(),
            },
        ],
    };
    Practice {
        title: "Weighing positions by their scores".into(),
        concept: "Attention".into(),
        example: problem(
            "Two positions score $1$ and $1$ against a query. What weight does each get?",
            "0.5",
        ),
        problems: vec![
            problem(
                "Three positions score $0$, $0$ and $0$. What weight does the first get?",
                "1/3",
            ),
            problem(
                "Two positions score $2$ and $2$. What weight does the second get?",
                "0.5",
            ),
            problem(
                "Four positions score the same. What weight does each get?",
                "0.25",
            ),
            problem(
                "Five positions score the same. What weight does each get?",
                "0.2",
            ),
        ],
        easier: problem("One position scores $3$. What is its weight?", "1"),
        source: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use incipit_learn::practice::{Flow, Next};

    #[test]
    fn the_demonstrations_sequence_ends_with_the_easier_problem_and_two_right_answers() {
        let made = made_up();
        let mut flow = Flow::new(made.problems.len());
        let miss = Given {
            score: 0.0,
            unchecked: false,
        };
        let right = Given {
            score: 1.0,
            unchecked: false,
        };
        for _ in 0..3 {
            flow.answer(miss);
            flow.advance();
        }
        assert!(flow.current().unwrap().easier);
        flow.answer(right);
        assert_eq!(flow.advance(), Next::Problem(3));
        flow.answer(right);
        assert_eq!(flow.advance(), Next::Done);
        assert_eq!(flow.streak(), 2);
        assert_eq!(flow.scored().len(), 5);
    }
}
