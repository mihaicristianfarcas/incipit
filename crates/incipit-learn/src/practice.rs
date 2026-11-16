//! Practice, as the earlier app ran it: a worked example, then problems the learner answers
//! before seeing anything. Two parts, both pure:
//!
//! - [`same_answer`] says whether what the learner wrote is plainly the answer: the same words
//!   or symbols once case, spacing, `$…$`, a leading `x =` and the like are set aside, or the
//!   same number (3, -2.5, 3/4, 50%). It never calls an answer wrong: whatever it does not
//!   accept goes to a grader that reads it.
//! - [`Flow`] says which problem comes next (`advance`). Two right in a row finishes the practice. After
//!   three misses the easier problem is served, once. The problems run out otherwise. An answer
//!   the grader could not check is neither right nor a miss and does not break a run of right
//!   ones.

/// From this score an answer is right: the earlier app's.
pub const RIGHT: f64 = 0.7;

/// Right answers in a row that finish the practice.
pub const IN_A_ROW: usize = 2;

/// Misses after which the easier problem comes.
pub const MISSES_BEFORE_EASIER: usize = 3;

/// The relative difference below which two numbers are the same.
const SAME_NUMBER: f64 = 1e-9;

/// The answer reduced to what matters when comparing: lower case, no `$` around it, no `\( \)`,
/// the dashes as `-`, `×` and `·` as `*`, `÷` as `/`, no degree sign, no comma in a thousands
/// group (`1,234`), no `x =` before it, no spaces and no final full stop.
pub fn normal_answer(text: &str) -> String {
    let lower = text.trim().to_lowercase();
    let mut s = lower
        .trim_start_matches('$')
        .trim_end_matches('$')
        .to_owned();
    if let Some(rest) = s.strip_prefix("\\(") {
        s = rest.to_owned();
    }
    if let Some(rest) = s.strip_suffix("\\)") {
        s = rest.to_owned();
    }
    let s: String = s
        .chars()
        .filter(|&c| c != '°')
        .map(|c| match c {
            '−' | '–' | '—' => '-',
            '×' | '·' => '*',
            '÷' => '/',
            c => c,
        })
        .collect();
    let s = without_thousands_commas(&s);
    let s = without_variable(&s);
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    s.strip_suffix('.').map(str::to_owned).unwrap_or(s)
}

/// `1,234,567` as `1234567`: a comma between a digit and exactly three digits that end a word.
fn without_thousands_commas(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        let thousands = c == ','
            && i > 0
            && chars[i - 1].is_ascii_digit()
            && chars
                .get(i + 1..i + 4)
                .is_some_and(|d| d.iter().all(char::is_ascii_digit))
            && !chars.get(i + 4).is_some_and(|&next| word(next));
        if !thousands {
            out.push(c);
        }
    }
    out
}

/// `x = 5` as `5`: a single letter and an equals sign at the start.
fn without_variable(text: &str) -> String {
    let mut chars = text.chars();
    if chars.next().is_some_and(|c| c.is_ascii_lowercase()) {
        if let Some(after) = chars.as_str().trim_start().strip_prefix('=') {
            return after.trim_start().to_owned();
        }
    }
    text.to_owned()
}

/// The number an answer is, when it is one: `3`, `-2.5`, `3/4`, `50%`, `1e3`.
pub fn number_of(text: &str) -> Option<f64> {
    let normal = normal_answer(text);
    let (body, percent) = match normal.strip_suffix('%') {
        Some(body) => (body, true),
        None => (normal.as_str(), false),
    };
    let value = match body.split_once('/') {
        Some((top, bottom)) if plain_decimal(top) && plain_decimal(bottom) => {
            let bottom: f64 = bottom.parse().ok()?;
            (bottom != 0.0).then(|| top.parse::<f64>().ok().map(|top| top / bottom))??
        }
        Some(_) => return None,
        None if is_number(body) => body.parse::<f64>().ok()?,
        None => return None,
    };
    value
        .is_finite()
        .then_some(if percent { value / 100.0 } else { value })
}

/// An optional minus, digits, and optionally a point and digits (`-2`, `3.5`).
fn plain_decimal(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    let (whole, fraction) = match digits.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (digits, None),
    };
    !whole.is_empty()
        && whole.chars().all(|c| c.is_ascii_digit())
        && fraction.is_none_or(|f| !f.is_empty() && f.chars().all(|c| c.is_ascii_digit()))
}

/// A number as the earlier app read one: an optional minus, digits with an optional point (`5`,
/// `5.`, `.5`, `5.25`), and an optional exponent (`e3`, `e-3`).
fn is_number(text: &str) -> bool {
    let body = text.strip_prefix('-').unwrap_or(text);
    let (mantissa, exponent) = match body.split_once('e') {
        Some((mantissa, exponent)) => (mantissa, Some(exponent)),
        None => (body, None),
    };
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    let mantissa_ok = match mantissa.split_once('.') {
        Some((whole, fraction)) => {
            (digits(whole) && (fraction.is_empty() || digits(fraction)))
                || (whole.is_empty() && digits(fraction))
        }
        None => digits(mantissa),
    };
    mantissa_ok && exponent.is_none_or(|e| digits(e.strip_prefix('-').unwrap_or(e)))
}

/// Whether `given` is plainly the answer, or one of the other ways of writing it.
pub fn same_answer(given: &str, answer: &str, accept: &[String]) -> bool {
    let mine = normal_answer(given);
    if mine.is_empty() {
        return false;
    }
    let number = number_of(given);
    std::iter::once(answer)
        .chain(accept.iter().map(String::as_str))
        .any(|candidate| {
            if normal_answer(candidate) == mine {
                return true;
            }
            match (number, number_of(candidate)) {
                (Some(n), Some(m)) => (n - m).abs() <= SAME_NUMBER * m.abs().max(1.0),
                _ => false,
            }
        })
}

/// An answer given to a problem: how well, and whether it could be checked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Given {
    /// 0 to 1.
    pub score: f64,
    /// Skipped after the grader failed: not scored, neither right nor a miss.
    pub unchecked: bool,
}

impl Given {
    pub fn right(&self) -> bool {
        !self.unchecked && self.score >= RIGHT
    }
}

/// A problem served, and the answer to it once given.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Round {
    /// The easier problem, served after three misses.
    pub easier: bool,
    pub given: Option<Given>,
}

/// Which problem a practice serves next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    /// The problem at this place in the list.
    Problem(usize),
    /// The easier one.
    Easier,
    /// The practice is over.
    Done,
}

/// A practice with `problems` problems and an easier one in reserve, being worked through.
#[derive(Clone, Debug, PartialEq)]
pub struct Flow {
    problems: usize,
    served: Vec<Round>,
    done: bool,
}

impl Flow {
    /// A practice that serves its first problem.
    pub fn new(problems: usize) -> Flow {
        Flow {
            problems,
            served: vec![Round {
                easier: false,
                given: None,
            }],
            done: problems == 0,
        }
    }

    pub fn done(&self) -> bool {
        self.done
    }

    /// Every problem served so far, in order.
    pub fn served(&self) -> &[Round] {
        &self.served
    }

    /// The problem on screen.
    pub fn current(&self) -> Option<&Round> {
        self.served.last().filter(|_| !self.done)
    }

    /// The problems from the list served so far (not the easier one).
    pub fn used(&self) -> usize {
        self.served.iter().filter(|r| !r.easier).count()
    }

    /// What the problem on screen is answered with.
    pub fn answer(&mut self, given: Given) {
        if self.done {
            return;
        }
        if let Some(round) = self.served.last_mut().filter(|r| r.given.is_none()) {
            round.given = Some(given);
        }
    }

    /// The answers that were checked, in order.
    pub fn scored(&self) -> Vec<Given> {
        self.served
            .iter()
            .filter_map(|r| r.given)
            .filter(|g| !g.unchecked)
            .collect()
    }

    /// How many checked answers were not right.
    pub fn misses(&self) -> usize {
        self.scored().iter().filter(|g| !g.right()).count()
    }

    /// Right answers in a row, counting back from the latest checked one.
    pub fn streak(&self) -> usize {
        self.scored().iter().rev().take_while(|g| g.right()).count()
    }

    /// The points scored, to a tenth.
    pub fn score(&self) -> f64 {
        let sum: f64 = self.scored().iter().map(|g| g.score).sum();
        (sum * 10.0).round() / 10.0
    }

    /// Whether the easier problem is due: three misses, and it has not been served.
    pub fn steps_back(&self) -> bool {
        self.misses() >= MISSES_BEFORE_EASIER && !self.served.iter().any(|r| r.easier)
    }

    /// Whether the next step finishes the practice.
    pub fn ending(&self) -> bool {
        self.streak() >= IN_A_ROW || (!self.steps_back() && self.used() >= self.problems)
    }

    /// Goes on from the problem on screen, once answered: what comes next.
    pub fn advance(&mut self) -> Next {
        if self.current().is_none_or(|r| r.given.is_none()) {
            return self.here();
        }
        if self.ending() {
            self.done = true;
            return Next::Done;
        }
        if self.steps_back() {
            self.served.push(Round {
                easier: true,
                given: None,
            });
            return Next::Easier;
        }
        let index = self.used();
        self.served.push(Round {
            easier: false,
            given: None,
        });
        Next::Problem(index)
    }

    /// The problem on screen, as a [`Next`].
    pub fn here(&self) -> Next {
        match self.served.last() {
            _ if self.done => Next::Done,
            Some(round) if round.easier => Next::Easier,
            Some(_) => Next::Problem(self.used() - 1),
            None => Next::Done,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accept(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn spacing_case_dollars_and_a_leading_variable_are_set_aside() {
        for given in ["x = 5", " $5$ ", "X=5.", "$$ 5 $$", "\\(5\\)", "5."] {
            assert!(same_answer(given, "5", &[]), "{given:?}");
        }
        assert!(!same_answer(
            "Newton's second law",
            "newtons second law",
            &[]
        ));
        assert!(same_answer("A  Large  Dog", "a large dog", &[]));
        assert!(same_answer("2×3", "2*3", &[]));
        assert!(same_answer("6 ÷ 3", "6/3", &[]));
        assert!(same_answer("−4", "-4", &[]));
        assert!(same_answer("90°", "90", &[]));
        assert!(!same_answer("", "5", &[]));
        assert!(!same_answer("6", "5", &[]));
    }

    #[test]
    fn a_thousands_comma_is_set_aside_but_a_list_is_not() {
        assert_eq!(normal_answer("1,234,567"), "1234567");
        assert_eq!(normal_answer("1,2345"), "1,2345");
        assert_eq!(normal_answer("1,234a"), "1,234a");
        assert_eq!(normal_answer("a,bcd"), "a,bcd");
        assert!(same_answer("1,000", "1000", &[]));
    }

    #[test]
    fn numbers_are_the_same_in_any_of_their_writings() {
        assert_eq!(number_of("3"), Some(3.0));
        assert_eq!(number_of("-2.5"), Some(-2.5));
        assert_eq!(number_of("3/4"), Some(0.75));
        assert_eq!(number_of("50%"), Some(0.5));
        assert_eq!(number_of(".5"), Some(0.5));
        assert_eq!(number_of("5."), Some(5.0));
        assert_eq!(number_of("1e3"), Some(1000.0));
        assert_eq!(number_of("2e-2"), Some(0.02));
        assert_eq!(number_of("1/0"), None);
        assert_eq!(number_of("x"), None);
        assert_eq!(number_of("3/4/5"), None);
        assert_eq!(number_of("1.2.3"), None);
        assert_eq!(number_of("e5"), None);
        assert!(same_answer("0.75", "3/4", &[]));
        assert!(same_answer("75%", "0.75", &[]));
        assert!(same_answer("0.5", "x", &accept(&["1/2"])));
        assert!(same_answer("1000000000.0000000001", "1000000000", &[]));
        assert!(!same_answer("0.76", "3/4", &[]));
    }

    #[test]
    fn another_way_of_writing_the_answer_is_accepted_when_the_problem_lists_it() {
        assert!(same_answer(
            "v = d / t",
            "speed = distance / time",
            &accept(&["v=d/t"])
        ));
        assert!(!same_answer("speed", "velocity", &[]));
    }

    fn given(score: f64) -> Given {
        Given {
            score,
            unchecked: false,
        }
    }

    const SKIPPED: Given = Given {
        score: 0.0,
        unchecked: true,
    };

    #[test]
    fn two_right_in_a_row_finish_the_practice() {
        let mut flow = Flow::new(4);
        assert_eq!(flow.here(), Next::Problem(0));
        flow.answer(given(1.0));
        assert_eq!(flow.streak(), 1);
        assert_eq!(flow.advance(), Next::Problem(1));
        flow.answer(given(0.7));
        assert_eq!(flow.streak(), 2);
        assert!(flow.ending());
        assert_eq!(flow.advance(), Next::Done);
        assert!(flow.done());
        assert_eq!(flow.score(), 1.7);
    }

    #[test]
    fn an_unchecked_answer_neither_adds_to_a_run_nor_breaks_it() {
        let mut flow = Flow::new(5);
        flow.answer(given(1.0));
        flow.advance();
        flow.answer(SKIPPED);
        assert_eq!(flow.streak(), 1);
        assert_eq!(flow.misses(), 0);
        flow.advance();
        flow.answer(given(1.0));
        assert_eq!(flow.streak(), 2);
        assert!(flow.ending());
        assert_eq!(flow.scored().len(), 2);
    }

    #[test]
    fn three_misses_bring_the_easier_problem_once() {
        let mut flow = Flow::new(5);
        for _ in 0..2 {
            flow.answer(given(0.0));
            assert!(matches!(flow.advance(), Next::Problem(_)));
        }
        flow.answer(given(0.3));
        assert!(flow.steps_back());
        assert!(!flow.ending());
        assert_eq!(flow.advance(), Next::Easier);
        assert_eq!(flow.here(), Next::Easier);
        assert!(flow.current().unwrap().easier);
        // Missed again: no second easier one; the problems go on where they were.
        flow.answer(given(0.0));
        assert!(!flow.steps_back());
        assert_eq!(flow.advance(), Next::Problem(3));
    }

    #[test]
    fn the_problems_running_out_finishes_it() {
        let mut flow = Flow::new(2);
        flow.answer(given(0.0));
        assert_eq!(flow.advance(), Next::Problem(1));
        flow.answer(given(1.0));
        assert!(flow.ending());
        assert_eq!(flow.advance(), Next::Done);
        // Nothing more is taken.
        flow.answer(given(1.0));
        assert_eq!(flow.score(), 1.0);
        assert_eq!(flow.advance(), Next::Done);
        assert!(flow.current().is_none());
    }

    #[test]
    fn nothing_moves_on_before_the_problem_is_answered() {
        let mut flow = Flow::new(3);
        assert_eq!(flow.advance(), Next::Problem(0));
        assert_eq!(flow.served().len(), 1);
        assert_eq!(Flow::new(0).advance(), Next::Done);
    }
}
