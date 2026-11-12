//! One run as the agent's sheet shows it, with no window in it: the run as its log has it
//! (`incipit_vault::AgentRun`), the steps the agent reported on the way, the files it changed by
//! title, and what went wrong with what the learner did with it (an Undo that was refused). What
//! can be done with a run follows the earlier app's panel: Open the file its reply names once it
//! is in the vault, Undo a landed change, Apply anyway or Discard one that waits, Keep an answer
//! that changed nothing as a note, Try again after a failure; and, in Incipit, Stop one that
//! works.
//!
//! Also the made-up runs that show the sheet without a model (INCIPIT_AGENT=demo, review,
//! refused, thread, recent): their actions change no vault.

use incipit_llm::{Cancel, Event};
use incipit_vault::{rfc3339, AgentRun, FileChange, FileStatus};

use super::run::Status;
use super::runs::{self, ago};

/// How many of the agent's latest steps are shown: enough to see it move, few enough to leave
/// the room to its reply.
pub const STEPS_SHOWN: usize = 6;
/// The most steps kept of a run that goes on for long.
pub const STEPS_KEPT: usize = 200;

/// Why a change could not be undone, in words for the learner.
pub fn undo_problem(error: &incipit_vault::Error) -> String {
    match error {
        incipit_vault::Error::WouldConflict(files) => format!(
            "Undo would lose what was written since in {files}, so nothing was undone. Change \
             those lines back by hand."
        ),
        other => format!("This could not be undone: {other}."),
    }
}

/// Whether a file a reply names can be opened, as the earlier app's `vaultLink` decided: a
/// note, concept, card or figure, what belongs to a book, or the book itself.
pub fn openable(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    let markdown = path.ends_with(".md");
    match parts.as_slice() {
        ["notes" | "concepts" | "cards" | "figures", _, ..] => markdown,
        ["library", book, _, ..] => !book.is_empty(),
        _ => false,
    }
}

/// A file a run changed, as the sheet lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Touched {
    /// The document's title, or the file's name when it is not (or no longer) a document.
    pub title: String,
    pub path: String,
    pub what: &'static str,
}

fn touched(change: &FileChange, title: Option<String>) -> Touched {
    let name = change.path.rsplit('/').next().unwrap_or(&change.path);
    Touched {
        title: title
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| name.trim_end_matches(".md").to_owned()),
        path: change.path.clone(),
        what: match change.status {
            FileStatus::Added => "New",
            FileStatus::Modified => "Changed",
            FileStatus::Removed => "Removed",
        },
    }
}

/// The word for where a run stands, as the earlier app's panel said it.
pub fn status_word(status: Status) -> &'static str {
    match status {
        Status::Running => "Working",
        Status::Review => "Waiting for your OK",
        Status::Merged => "Done",
        Status::Done => "Answered",
        Status::Discarded => "Discarded",
        Status::Failed => "Didn't work",
        Status::Undone => "Undone",
        Status::Stopped => "Stopped",
    }
}

/// One request and what became of it.
#[derive(Debug)]
pub struct Turn {
    pub run: AgentRun,
    /// What the agent did, a line per step, oldest first.
    pub steps: Vec<String>,
    pub files: Vec<Touched>,
    /// What went wrong with what the learner did with it, in words for them.
    pub problems: Vec<String>,
    /// What stops it while it works.
    pub stop: Option<Cancel>,
    /// Undo, Apply anyway, Discard or Keep is being done.
    pub busy: bool,
    /// Made up: what is done with it changes no vault.
    pub made_up: bool,
}

impl Turn {
    /// A run as the log has it. `title` gives the title of the document at a path, as the
    /// vault has it now.
    pub fn new(run: AgentRun, title: impl Fn(&str) -> Option<String>) -> Turn {
        let mut turn = Turn {
            run: run.clone(),
            steps: Vec::new(),
            files: Vec::new(),
            problems: Vec::new(),
            stop: None,
            busy: false,
            made_up: false,
        };
        turn.finish(run, title);
        turn
    }

    /// Something the agent reported while working. Its words on the way are left: it ends with
    /// its reply as JSON, which is read once it ends.
    pub fn hear(&mut self, event: Event) {
        match event {
            Event::Text(_) | Event::Link(_) => {}
            Event::Activity(step) => {
                let step = step.trim();
                if step.is_empty() || self.steps.last().is_some_and(|last| last == step) {
                    return;
                }
                if self.steps.len() == STEPS_KEPT {
                    self.steps.remove(0);
                }
                self.steps.push(step.to_owned());
            }
        }
    }

    /// The run as it stands now (it ended, landed, was discarded or kept).
    pub fn finish(&mut self, run: AgentRun, title: impl Fn(&str) -> Option<String>) {
        self.files = run
            .files
            .iter()
            .map(|change| touched(change, title(&change.path)))
            .collect();
        if run.status != Status::Running {
            self.stop = None;
        }
        self.run = run;
    }

    pub fn working(&self) -> bool {
        self.run.status == Status::Running
    }

    pub fn can_undo(&self) -> bool {
        self.run.status == Status::Merged && (self.run.change.is_some() || self.made_up)
    }

    pub fn waits(&self) -> bool {
        self.run.status == Status::Review
    }

    /// An answer that changed nothing, not kept yet.
    pub fn can_keep(&self) -> bool {
        self.run.status == Status::Done
            && !self.run.answer.trim().is_empty()
            && self.run.files.is_empty()
            && self.run.kept.is_none()
    }

    /// The file its reply names, when it is in the vault: once the change landed, or for an
    /// answer.
    pub fn to_open(&self) -> Option<&str> {
        matches!(self.run.status, Status::Merged | Status::Done)
            .then_some(self.run.open.as_deref())
            .flatten()
            .filter(|path| openable(path))
    }

    /// The note its answer was kept in, when it can be opened.
    pub fn kept(&self) -> Option<&str> {
        self.run.kept.as_deref().filter(|path| openable(path))
    }

    /// The steps the sheet shows: the latest few.
    pub fn shown_steps(&self) -> &[String] {
        &self.steps[self.steps.len().saturating_sub(STEPS_SHOWN)..]
    }

    /// How it stands, where the learner was, when it was asked and how long it took, in a line.
    pub fn meta(&self, now: i64) -> String {
        let mut parts = vec![status_word(self.run.status).to_owned()];
        parts.extend(self.run.place.clone());
        parts.push(ago(&self.run.at, now));
        if let (Some(ms), false) = (self.run.ms, self.working()) {
            parts.push(format!("{} s", (ms as f64 / 1000.0).round()));
        }
        parts.retain(|p| !p.is_empty());
        parts.join(" · ")
    }

    /// Where the run stands, in a line, when there is something to say beyond its reply.
    pub fn standing(&self) -> Option<&'static str> {
        match self.run.status {
            Status::Running => Some("Working…"),
            Status::Undone => Some("Undone: what this request changed was taken back."),
            Status::Stopped => Some("Stopped. No file was changed."),
            Status::Done => Some("No file was changed."),
            Status::Review => Some(
                "Waiting for your OK: some files it changed can no longer be read. Apply them \
                 anyway, or discard them.",
            ),
            Status::Discarded => Some("Discarded: nothing of this request reached your vault."),
            Status::Failed if !self.files.is_empty() => {
                Some("Nothing reached your vault. It would have changed these files:")
            }
            Status::Failed | Status::Merged => None,
        }
    }
}

/// A run that never happened, asked `minutes` before `now`.
fn made_up_run(task: &str, now: i64, minutes: i64, status: Status) -> AgentRun {
    let mut run = runs::begin(task, None, "", 0, rfc3339(now - minutes * 60_000))
        .expect("a made-up request is not empty and nothing else is going");
    run.status = status;
    run
}

fn change(path: &str, status: FileStatus) -> FileChange {
    FileChange {
        path: path.to_owned(),
        status,
    }
}

/// The titles the made-up runs' files have.
pub fn made_up_title(path: &str) -> Option<String> {
    match path {
        "cards/What attention weighs.md" => Some("What attention weighs".into()),
        "notes/Attention.md" => Some("Attention".into()),
        _ => None,
    }
}

/// Runs that never happened, to look at the sheet without a model, newest first: a landed
/// change (`demo`), one waiting for the learner's OK (`review`), one refused for changing the
/// app's own file (`refused`), a conversation of an answer and a follow-up that landed
/// (`thread`), and four conversations, one of them working (`recent`).
pub fn made_up(which: &str, now: i64) -> Vec<Turn> {
    let turn = |mut run: AgentRun, steps: &[&str]| {
        run.ms = (run.status != Status::Running).then_some(48_000);
        let mut turn = Turn::new(run, made_up_title);
        turn.made_up = true;
        for step in steps {
            turn.hear(Event::Activity((*step).to_owned()));
        }
        turn
    };
    let steps = [
        "Reading notes/Attention.md",
        "Writing cards/What attention weighs.md",
        "Editing notes/Attention.md",
    ];
    let mut cards = made_up_run(
        "Make flashcards from my note on attention",
        now,
        2,
        Status::Merged,
    );
    cards.place = Some("Attention".into());
    cards.answer = "I wrote **one flashcard** from the note and linked it there:\n\n\
        - `cards/What attention weighs.md`: what the weights in attention say.\n\
        - `notes/Attention.md`: a link to the new card under *Cards*."
        .into();
    cards.open = Some("cards/What attention weighs.md".into());
    cards.files = vec![
        change("cards/What attention weighs.md", FileStatus::Added),
        change("notes/Attention.md", FileStatus::Modified),
    ];
    match which {
        "review" => {
            let mut run = cards;
            run.status = Status::Review;
            run.files.truncate(1);
            run.review = vec![
                "Can't be read: cards/What attention weighs.md: the properties at \
                               the top are not closed with a line of ---"
                    .into(),
            ];
            vec![turn(run, &steps)]
        }
        "refused" => {
            let mut run = cards;
            run.status = Status::Failed;
            run.files = vec![change("log/reviews.jsonl", FileStatus::Modified)];
            run.error =
                Some("log/reviews.jsonl: the app keeps this file; agents may not change it".into());
            vec![turn(run, &steps)]
        }
        "thread" => {
            let mut first = made_up_run(
                "What does my note say attention weighs?",
                now,
                6,
                Status::Done,
            );
            first.place = Some("Attention".into());
            first.answer = "That each word's weight is how well its key matches the query, \
                softened so the weights add up to one ([[Attention]])."
                .into();
            first.open = Some("notes/Attention.md".into());
            let mut follow = cards;
            follow.thread = Some(first.id.clone());
            follow.task = "Make a flashcard of that".into();
            vec![
                turn(follow, &steps),
                turn(first, &["Reading notes/Attention.md"]),
            ]
        }
        "recent" => {
            let going = made_up_run("Summarise chapter 3 into a note", now, 1, Status::Running);
            let mut answered = made_up_run(
                "What have I written about consensus?",
                now,
                50,
                Status::Done,
            );
            answered.answer = "Two notes mention it.".into();
            let mut undone = made_up_run("Tidy the tags on my notes", now, 60 * 26, Status::Undone);
            undone.files = vec![change("notes/Attention.md", FileStatus::Modified)];
            vec![
                turn(going, &["Reading library/calculus/book.md"]),
                turn(cards, &steps),
                turn(answered, &[]),
                turn(undone, &[]),
            ]
        }
        _ => vec![turn(cards, &steps)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_794_000_000_000;

    fn no_title(_: &str) -> Option<String> {
        None
    }

    fn ended(status: Status, answer: &str, files: &[(&str, FileStatus)]) -> AgentRun {
        let mut run = made_up_run("x", NOW, 1, status);
        run.answer = answer.to_owned();
        run.files = files.iter().map(|(path, s)| change(path, *s)).collect();
        run.change = (status == Status::Merged).then(|| "change".to_owned());
        run
    }

    fn going() -> Turn {
        Turn::new(made_up_run("x", NOW, 0, Status::Running), no_title)
    }

    #[test]
    fn steps_are_kept_in_order_without_repeats_and_the_latest_are_shown() {
        let mut turn = going();
        turn.hear(Event::Activity("  Reading a.md ".into()));
        turn.hear(Event::Activity("Reading a.md".into()));
        turn.hear(Event::Activity(" ".into()));
        turn.hear(Event::Link("https://example.com".into()));
        turn.hear(Event::Text("{\"reply\": \"I ad".into()));
        assert_eq!(turn.steps, ["Reading a.md"]);
        for n in 0..STEPS_KEPT + 5 {
            turn.hear(Event::Activity(format!("Step {n}")));
        }
        assert_eq!(turn.steps.len(), STEPS_KEPT);
        let shown = turn.shown_steps();
        assert_eq!(shown.len(), STEPS_SHOWN);
        assert_eq!(shown.last().unwrap(), &format!("Step {}", STEPS_KEPT + 4));
        assert_eq!(turn.standing(), Some("Working…"));
        assert_eq!(turn.meta(NOW), "Working · just now");
    }

    #[test]
    fn a_landed_run_offers_undo_and_says_how_it_went_in_a_line() {
        let mut turn = going();
        turn.stop = Some(Cancel::new());
        let mut run = ended(
            Status::Merged,
            "I added one card.",
            &[("cards/New.md", FileStatus::Added)],
        );
        run.place = Some("Waves".into());
        run.ms = Some(12_400);
        turn.finish(run, no_title);
        assert!(!turn.working());
        assert!(turn.stop.is_none());
        assert!(turn.can_undo());
        assert!(!turn.can_keep());
        assert_eq!(turn.standing(), None);
        assert_eq!(
            turn.meta(NOW + 3 * 60_000),
            "Done · Waves · 4 min ago · 12 s"
        );
    }

    #[test]
    fn changed_files_are_listed_by_title_or_else_by_file_name() {
        let mut turn = going();
        let files = [
            ("notes/Waves.md", FileStatus::Modified),
            ("cards/old card.md", FileStatus::Removed),
            ("notes/Tides.md", FileStatus::Added),
        ];
        turn.finish(ended(Status::Merged, "Done.", &files), |path| {
            (path == "notes/Waves.md").then(|| "Waves and tides".to_owned())
        });
        let listed: Vec<(&str, &str, &str)> = turn
            .files
            .iter()
            .map(|f| (f.title.as_str(), f.path.as_str(), f.what))
            .collect();
        assert_eq!(
            listed,
            [
                ("Waves and tides", "notes/Waves.md", "Changed"),
                ("old card", "cards/old card.md", "Removed"),
                ("Tides", "notes/Tides.md", "New"),
            ]
        );
    }

    #[test]
    fn an_answer_that_changed_nothing_can_be_kept_once_and_offers_no_undo() {
        let mut turn = going();
        turn.finish(ended(Status::Done, "Nothing to do.", &[]), no_title);
        assert!(!turn.can_undo());
        assert!(turn.can_keep());
        assert_eq!(turn.standing(), Some("No file was changed."));
        let mut kept = turn.run.clone();
        kept.kept = Some("notes/x.md".into());
        turn.finish(kept, no_title);
        assert!(!turn.can_keep());
        assert_eq!(turn.kept(), Some("notes/x.md"));
        turn.finish(ended(Status::Done, " ", &[]), no_title);
        assert!(!turn.can_keep(), "no answer, nothing to keep");
    }

    #[test]
    fn a_stopped_run_changed_nothing() {
        let mut turn = going();
        turn.finish(ended(Status::Stopped, "", &[]), no_title);
        assert!(turn.problems.is_empty());
        assert!(!turn.can_undo());
        assert_eq!(turn.standing(), Some("Stopped. No file was changed."));
    }

    #[test]
    fn a_failed_run_says_what_it_would_have_changed() {
        let mut clashed = going();
        let mut run = ended(
            Status::Failed,
            "Done.",
            &[("notes/A.md", FileStatus::Modified)],
        );
        run.error = Some("It clashes with edits made while the agent worked.".into());
        clashed.finish(run, no_title);
        assert!(!clashed.can_undo());
        assert_eq!(
            clashed.standing(),
            Some("Nothing reached your vault. It would have changed these files:")
        );
        let mut failed = going();
        failed.finish(ended(Status::Failed, "", &[]), no_title);
        assert_eq!(failed.standing(), None);
        assert!(failed.meta(NOW).starts_with("Didn't work"));
    }

    #[test]
    fn a_change_waiting_for_the_learners_ok_offers_no_undo() {
        let mut turn = going();
        let files = [("notes/Waves.md", FileStatus::Added)];
        let mut waiting = ended(Status::Review, "I wrote a note.", &files);
        waiting.change = Some("change".into());
        waiting.review = vec!["Can't be read: notes/Waves.md: broken".into()];
        turn.finish(waiting, no_title);
        assert!(turn.waits());
        assert!(!turn.can_undo());
        assert!(turn.standing().unwrap().starts_with("Waiting for your OK"));
    }

    #[test]
    fn a_file_a_reply_names_is_opened_where_the_earlier_app_opened_it() {
        for path in [
            "notes/Waves.md",
            "notes/Physics/Waves.md",
            "concepts/Wave.md",
            "cards/What a wave is.md",
            "figures/A wave.md",
            "library/calculus/annotations/a1.md",
            "library/calculus/book.md",
            "library/calculus/calculus.pdf",
        ] {
            assert!(openable(path), "{path}");
        }
        for path in [
            "notes/data.json",
            "log/reviews.jsonl",
            "README.md",
            "notes/",
            "library/x",
        ] {
            assert!(!openable(path), "{path}");
        }

        let mut turn = going();
        let mut answered = ended(Status::Done, "It is in your note.", &[]);
        answered.open = Some("notes/Waves.md".into());
        turn.run.open = answered.open.clone();
        assert_eq!(turn.to_open(), None, "not while it works");
        turn.finish(answered.clone(), no_title);
        assert_eq!(turn.to_open(), Some("notes/Waves.md"));
        answered.status = Status::Review;
        turn.finish(answered.clone(), no_title);
        assert_eq!(turn.to_open(), None, "not before it is in the vault");
        answered.open = Some("log/reviews.jsonl".into());
        answered.status = Status::Merged;
        turn.finish(answered, no_title);
        assert_eq!(turn.to_open(), None);
    }

    #[test]
    fn after_undo_nothing_further_is_offered() {
        let mut turn = going();
        let files = [("notes/A.md", FileStatus::Modified)];
        turn.finish(ended(Status::Merged, "Done.", &files), no_title);
        // A refused undo leaves the run as it was, with why.
        turn.problems.push("it clashed".into());
        assert!(turn.can_undo(), "a refused undo can be tried again");
        let mut undone = turn.run.clone();
        undone.status = Status::Undone;
        turn.finish(undone, no_title);
        assert!(!turn.can_undo());
        assert_eq!(
            turn.standing(),
            Some("Undone: what this request changed was taken back.")
        );
    }

    #[test]
    fn an_undo_that_would_lose_later_edits_names_the_files() {
        let clash = incipit_vault::Error::WouldConflict("notes/A.md".into());
        let words = undo_problem(&clash);
        assert!(words.contains("notes/A.md"), "{words}");
        assert!(words.contains("nothing was undone"), "{words}");
    }

    #[test]
    fn the_made_up_runs_need_no_vault() {
        let demo = made_up("demo", NOW);
        assert_eq!(demo.len(), 1);
        assert_eq!(demo[0].steps.len(), 3);
        assert_eq!(demo[0].files.len(), 2);
        assert_eq!(demo[0].files[0].title, "What attention weighs");
        assert!(demo[0].can_undo());
        assert!(demo[0].run.change.is_none(), "its undo touches no vault");
        assert_eq!(demo[0].to_open(), Some("cards/What attention weighs.md"));

        let review = made_up("review", NOW);
        assert!(review[0].waits());
        assert_eq!(review[0].run.review.len(), 1);
        let refused = made_up("refused", NOW);
        assert_eq!(refused[0].run.status, Status::Failed);
        assert!(refused[0]
            .run
            .error
            .as_deref()
            .unwrap()
            .contains("agents may not change it"));
        assert!(!refused[0].can_undo());

        let thread = made_up("thread", NOW);
        assert_eq!(thread.len(), 2);
        assert_eq!(
            thread[0].run.thread.as_deref(),
            Some(thread[1].run.id.as_str())
        );
        assert!(thread[1].can_keep());
        let recent = made_up("recent", NOW);
        let conversations =
            runs::conversations(&recent.iter().map(|t| t.run.clone()).collect::<Vec<_>>());
        assert_eq!(conversations.len(), 4);
        assert!(recent[0].working());
    }
}
