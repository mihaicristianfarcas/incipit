//! The vault agent: a sheet over the window, opened from any page with a key, where the learner
//! says what should be done in their vault and the model they chose does it, as an agent with a
//! shell. The sheet says what the agent will know of where the learner is (the note or concept
//! open, the book and the place in it, the selection; `here.rs`), which the learner can leave
//! out. While it works the sheet shows what it is doing; when it ends, its reply, the files it
//! changed, Open for the main one, and Undo, or, when a file it changed can no longer be read,
//! why, with Apply anyway and Discard.
//!
//! The run itself has no window in it (`run.rs`): the agent works in a working copy of the vault
//! of its own, under the provider crate's OS sandbox, which lets it write there and nowhere else
//! and read the vault. The learner's words are the request. Where they are, and what the agent
//! reads in the files, is material, and the instructions (`prompt.rs`), which hold no text from
//! any file, say so. The words it writes on the way are not shown: it ends with a reply in JSON.
//!
//! One request is one change in the vault's history, made of the agent's work alone. When the
//! agent ends, its change lands after whatever the learner wrote meanwhile, is read into the
//! index and announced, so open pages follow; one that changes the app's own files or clashes
//! with the learner's edits, or an agent that failed or was stopped, lands nothing. Undo takes that change out and keeps what
//! was written before and after it; it is refused when later edits touched the same lines. The
//! run and the history calls block, so they run off the window's thread and the sheet awaits
//! them.
//!
//! Closing the sheet does not stop the agent: it goes on, and the key shows it again. Stop does.
//!
//! Without clicking: INCIPIT_AGENT=demo shows a made-up finished run, `review` one waiting for
//! the learner's OK and `refused` one refused for changing the review log, and none calls a
//! model; INCIPIT_AGENT with other words runs them once the vault is open, with the learner's
//! login.

use futures::StreamExt;
use gpui::{
    div, prelude::*, px, AnyElement, Context, Entity, EventEmitter, FontWeight, IntoElement,
    KeyDownEvent, Render, ScrollHandle, Subscription, Window,
};
use incipit_llm::{Cancel, Event, Update};
use incipit_ui::{button, field, sheet, spinner, FieldEvent, IconName, TextField, Theme, Type};
use incipit_vault::{FileChange, FileStatus, Vault, VaultPath};

use crate::models::Models;
use crate::workspace::{Happened, Workspace};
use here::Here;
use run::{Job, Outcome, Status};

pub mod here;
pub mod prompt;
pub mod run;

/// What the learner is told when no vault is open.
const NO_VAULT: &str = "No vault is open. Choose its folder in Settings first.";
/// The sheet's width, and the most height a run takes before it scrolls: with the field and the
/// buttons, the sheet fits the smallest window.
const WIDTH: f32 = 560.0;
const RUN: f32 = 300.0;
/// How many of the agent's latest steps are shown: enough to see it move, few enough to leave
/// the room to its closing words.
const STEPS_SHOWN: usize = 6;
/// The most steps kept of a run that goes on for long.
const STEPS_KEPT: usize = 200;
/// Whether a file a reply names can be opened, as the earlier app's `vaultLink` decided: a
/// note, concept, card or figure, what belongs to a book, or the book itself.
fn openable(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    let markdown = path.ends_with(".md");
    match parts.as_slice() {
        ["notes" | "concepts" | "cards" | "figures", _, ..] => markdown,
        ["library", book, _, ..] => !book.is_empty(),
        _ => false,
    }
}

/// Why a change could not be undone, in words for the learner.
fn undo_problem(error: &incipit_vault::Error) -> String {
    match error {
        incipit_vault::Error::WouldConflict(files) => format!(
            "Undo would lose what was written since in {files}, so nothing was undone. Change \
             those lines back by hand."
        ),
        other => format!("This could not be undone: {other}."),
    }
}

/// A file a run changed, as the sheet lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Touched {
    /// The document's title, or the file's name when it is not (or no longer) a document.
    title: String,
    path: String,
    what: &'static str,
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

/// One request and what became of it.
#[derive(Debug)]
struct Run {
    request: String,
    /// What the agent did, a line per step, oldest first.
    steps: Vec<String>,
    /// Its words as far as they have come, and its closing words once it ended.
    words: String,
    /// How it ended; `None` while it works.
    status: Option<Status>,
    /// What went wrong, in words for the learner.
    problems: Vec<String>,
    files: Vec<Touched>,
    /// The main file it made or changed, to open from its reply.
    open: Option<String>,
    /// The run's change in the vault's history, by change id, which Undo takes out.
    change: Option<String>,
    /// What waits for the learner's OK, a line per file that can no longer be read.
    review: Vec<String>,
    /// The run's end while its change waits for the learner's OK.
    waiting: Option<Outcome>,
}

impl Run {
    fn new(request: &str) -> Run {
        Run {
            request: request.to_owned(),
            steps: Vec::new(),
            words: String::new(),
            status: None,
            problems: Vec::new(),
            files: Vec::new(),
            open: None,
            change: None,
            review: Vec::new(),
            waiting: None,
        }
    }

    /// Something the agent reported while working. Its words on the way are left: it ends with
    /// its reply as JSON, which is read once it ends.
    fn hear(&mut self, event: Event) {
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

    /// The run ended with `outcome`. `title` gives the title of the document at a path, as the
    /// vault has it now.
    fn finish(&mut self, outcome: Outcome, title: impl Fn(&str) -> Option<String>) {
        if !outcome.answer.trim().is_empty() {
            self.words = outcome.answer.clone();
        }
        self.problems.extend(outcome.error.clone());
        self.files = outcome
            .files
            .iter()
            .map(|change| touched(change, title(&change.path)))
            .collect();
        self.open = outcome.open.clone().filter(|path| openable(path));
        self.change = outcome.change.clone();
        self.review = outcome.review.clone();
        self.status = Some(outcome.status);
        self.waiting = (outcome.status == Status::Review).then_some(outcome);
    }

    fn working(&self) -> bool {
        self.status.is_none()
    }

    fn can_undo(&self) -> bool {
        self.status == Some(Status::Merged)
    }

    fn waits(&self) -> bool {
        self.status == Some(Status::Review)
    }

    /// The file its reply names, when it is in the vault: once the change landed, or for an
    /// answer.
    fn to_open(&self) -> Option<&str> {
        matches!(self.status, Some(Status::Merged | Status::Done))
            .then_some(self.open.as_deref())
            .flatten()
    }

    /// Undo ended.
    fn undone(&mut self, result: Result<(), String>) {
        match result {
            Ok(()) => self.status = Some(Status::Undone),
            Err(problem) => self.problems.push(problem),
        }
    }

    /// The steps the sheet shows: the latest few.
    fn shown_steps(&self) -> &[String] {
        &self.steps[self.steps.len().saturating_sub(STEPS_SHOWN)..]
    }

    /// Where the run stands, in a line, when there is something to say beyond its words.
    fn standing(&self) -> Option<&'static str> {
        match self.status {
            None => Some("Working…"),
            Some(Status::Undone) => Some("Undone: what this request changed was taken back."),
            Some(Status::Stopped) => Some("Stopped. No file was changed."),
            Some(Status::Done) => Some("No file was changed."),
            Some(Status::Review) => Some(
                "Waiting for your OK: some files it changed can no longer be read. Apply them \
                 anyway, or discard them.",
            ),
            Some(Status::Discarded) => {
                Some("Discarded: nothing of this request reached your vault.")
            }
            Some(Status::Failed) if !self.files.is_empty() => {
                Some("Nothing reached your vault. It would have changed these files:")
            }
            Some(Status::Failed | Status::Merged) => None,
        }
    }

    /// A finished run that never happened, to look at the sheet without a model: landed
    /// (`demo`), waiting for the learner's OK (`review`) or refused for changing the app's own
    /// file (`refused`).
    fn made_up_as(which: &str) -> Run {
        let mut run = Run::made_up();
        let change = |path: &str, status| FileChange {
            path: path.to_owned(),
            status,
        };
        match which {
            "review" => {
                let files = vec![change("cards/What attention weighs.md", FileStatus::Added)];
                run.finish(
                    Outcome {
                        status: Status::Review,
                        answer: run.words.clone(),
                        open: None,
                        files,
                        change: None,
                        error: None,
                        review: vec!["Can't be read: cards/What attention weighs.md: the \
                                      properties at the top are not closed with a line of ---"
                            .to_owned()],
                    },
                    |_| Some("What attention weighs".to_owned()),
                );
            }
            "refused" => {
                let files = vec![change("log/reviews.jsonl", FileStatus::Modified)];
                run.finish(
                    Outcome {
                        status: Status::Failed,
                        answer: run.words.clone(),
                        open: None,
                        files,
                        change: None,
                        error: Some(
                            "log/reviews.jsonl: the app keeps this file; agents may not change it"
                                .to_owned(),
                        ),
                        review: Vec::new(),
                    },
                    |_| None,
                );
            }
            _ => {}
        }
        run
    }

    fn made_up() -> Run {
        let mut run = Run::new("Make flashcards from my note on attention");
        for step in [
            "Reading notes/Attention.md",
            "Writing cards/What attention weighs.md",
            "Editing notes/Attention.md",
        ] {
            run.hear(Event::Activity(step.to_owned()));
        }
        run.status = Some(Status::Merged);
        run.words = "I wrote **one flashcard** from the note and linked it there:\n\n\
            - `cards/What attention weighs.md`: what the weights in attention say.\n\
            - `notes/Attention.md`: a link to the new card under *Cards*."
            .to_owned();
        run.files = vec![
            Touched {
                title: "What attention weighs".to_owned(),
                path: "cards/What attention weighs.md".to_owned(),
                what: "New",
            },
            Touched {
                title: "Attention".to_owned(),
                path: "notes/Attention.md".to_owned(),
                what: "Changed",
            },
        ];
        run
    }
}

/// The learner closed the sheet.
pub struct Closed;

/// The learner asked to open the file a reply names, by its path in the vault the run worked in.
pub struct OpenFile(pub String);

pub struct AgentView {
    workspace: Entity<Workspace>,
    models: Entity<Models>,
    words: Entity<TextField>,
    run: Option<Run>,
    /// The vault the run works in, which Undo goes back to even if another was opened since.
    ran_in: Option<Vault>,
    stop: Option<Cancel>,
    busy: bool,
    /// Why nothing was started, in words for the learner.
    problem: Option<String>,
    /// Words to run as soon as a vault is open.
    waiting: Option<String>,
    /// Where the learner was as the sheet opened, unless they left it out.
    here: Here,
    scroll: ScrollHandle,
    _heard: [Subscription; 2],
}

impl EventEmitter<Closed> for AgentView {}
impl EventEmitter<OpenFile> for AgentView {}

impl AgentView {
    pub fn new(
        workspace: Entity<Workspace>,
        models: Entity<Models>,
        cx: &mut Context<Self>,
    ) -> Self {
        let words =
            cx.new(|cx| TextField::new(cx).placeholder("What should be done in your vault?"));
        let typed = cx.subscribe(
            &words,
            |this: &mut Self, _, event: &FieldEvent, cx| match event {
                FieldEvent::Changed => {}
                FieldEvent::Submit => this.run(cx),
                FieldEvent::Cancel => cx.emit(Closed),
            },
        );
        let opened = cx.subscribe(&workspace, |this: &mut Self, _, happened, cx| {
            if matches!(happened, Happened::Opened) && this.waiting.is_some() {
                this.run(cx);
            }
        });
        AgentView {
            workspace,
            models,
            words,
            run: None,
            ran_in: None,
            stop: None,
            busy: false,
            problem: None,
            waiting: None,
            here: Here::default(),
            scroll: ScrollHandle::new(),
            _heard: [typed, opened],
        }
    }

    /// Take the keyboard: the sheet was opened where the learner is, `here`. A run that is
    /// going on, or the last one, is still there.
    pub fn start(&mut self, here: Here, window: &mut Window, cx: &mut Context<Self>) {
        self.here = here;
        TextField::focus(&self.words, window, cx);
        cx.notify();
    }

    /// The learner leaves out where they are: the agent works on the whole vault.
    fn leave_out_here(&mut self, cx: &mut Context<Self>) {
        self.here = self.here.page_only();
        cx.notify();
    }

    /// INCIPIT_AGENT=demo, review or refused: a made-up finished run, with no model called.
    /// Its Undo, Apply anyway and Discard change nothing in the vault.
    pub fn show_demo(&mut self, which: &str, cx: &mut Context<Self>) {
        self.run = Some(Run::made_up_as(which));
        cx.notify();
    }

    /// INCIPIT_AGENT=words: run them, now or as soon as the vault is open.
    pub fn run_words(&mut self, words: &str, cx: &mut Context<Self>) {
        self.words.update(cx, |field, cx| field.set_text(words, cx));
        if self.workspace.read(cx).vault.is_some() {
            self.run(cx);
        } else {
            self.waiting = Some(words.to_owned());
        }
    }

    fn working(&self) -> bool {
        self.run.as_ref().is_some_and(Run::working)
    }

    /// Have the agent do what is in the field.
    fn run(&mut self, cx: &mut Context<Self>) {
        self.waiting = None;
        let request = self.words.read(cx).text().trim().to_string();
        if request.is_empty() || self.working() || self.busy {
            return;
        }
        let Some(vault) = self.workspace.read(cx).vault.clone() else {
            self.problem = Some(NO_VAULT.to_owned());
            cx.notify();
            return;
        };
        let job = Job {
            runs: run::runs_folder(&crate::workspace::home(), &vault),
            id: run::new_id(),
            task: request.clone(),
            vault: vault.clone(),
        };
        let described = here::describe(&vault, &self.here);
        let asking = self.models.read(cx).act(
            prompt::request_for(&request, &described.lines, &[]),
            move |provider, request, cancel, on| {
                Ok(run::execute(provider, request, &job, cancel, on))
            },
        );
        let mut asking = match asking {
            Ok(asking) => asking,
            Err(problem) => {
                self.refuse(&request, problem, cx);
                return;
            }
        };
        self.problem = None;
        self.words.update(cx, |field, cx| field.set_text("", cx));
        self.run = Some(Run::new(&request));
        self.ran_in = Some(vault.clone());
        self.stop = Some(asking.cancel.clone());
        cx.notify();
        cx.spawn(async move |this, cx| {
            // A channel that closes without an end is a run that was dropped: as if stopped.
            let mut outcome = Outcome::stopped();
            while let Some(update) = asking.updates.next().await {
                match update {
                    Update::Event(event) => {
                        let heard = this.update(cx, |this, cx| {
                            if let Some(run) = &mut this.run {
                                run.hear(event);
                            }
                            this.scroll.scroll_to_bottom();
                            cx.notify();
                        });
                        if heard.is_err() {
                            asking.cancel.cancel();
                        }
                    }
                    Update::Done(result) => {
                        if let Ok(ended) = result {
                            outcome = ended;
                        }
                        break;
                    }
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.stop = None;
                if let Some(run) = &mut this.run {
                    let title = |path: &str| {
                        let path = VaultPath::new(path).ok()?;
                        vault.at(&path).map(|doc| doc.title())
                    };
                    run.finish(outcome, title);
                }
                this.scroll.scroll_to_bottom();
                cx.notify();
            });
        })
        .detach();
    }

    /// Nothing was started: the learner's words go back to the field, with why.
    fn refuse(&mut self, request: &str, problem: String, cx: &mut Context<Self>) {
        self.run = None;
        self.problem = Some(problem);
        self.words
            .update(cx, |field, cx| field.set_text(request, cx));
        cx.notify();
    }

    /// Stop the agent. Nothing it changed reaches the vault.
    fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(stop) = &self.stop {
            stop.cancel();
        }
        cx.notify();
    }

    /// Take the run's change out of the vault, and nothing else.
    fn undo(&mut self, cx: &mut Context<Self>) {
        let Some(run) = self.run.as_mut().filter(|run| run.can_undo()) else {
            return;
        };
        if self.busy {
            return;
        }
        let (Some(change), Some(vault)) = (run.change.clone(), self.ran_in.clone()) else {
            // The made-up run changed nothing.
            run.undone(Ok(()));
            cx.notify();
            return;
        };
        self.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { vault.drop_change(&change) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                if let Some(run) = &mut this.run {
                    run.undone(result.map_err(|e| undo_problem(&e)));
                }
                this.scroll.scroll_to_bottom();
                cx.notify();
            });
        })
        .detach();
    }

    /// The learner's OK for a change that waited for it: it lands as any other.
    fn apply(&mut self, cx: &mut Context<Self>) {
        self.settle(true, cx);
    }

    /// The learner turned down a change that waited for their OK: it is left out.
    fn discard(&mut self, cx: &mut Context<Self>) {
        self.settle(false, cx);
    }

    fn settle(&mut self, apply: bool, cx: &mut Context<Self>) {
        let Some(run) = self.run.as_mut().filter(|run| run.waits()) else {
            return;
        };
        if self.busy {
            return;
        }
        let (Some(waiting), Some(vault)) = (run.waiting.clone(), self.ran_in.clone()) else {
            // The made-up run has no change in any vault.
            let mut ended = run.waiting.clone().unwrap_or_else(Outcome::stopped);
            ended.status = if apply {
                Status::Merged
            } else {
                Status::Discarded
            };
            run.finish(ended, |_| None);
            cx.notify();
            return;
        };
        let task = run.request.clone();
        self.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let into = vault.clone();
            let ended = cx
                .background_executor()
                .spawn(async move {
                    if apply {
                        Ok(run::approve(&into, &task, waiting))
                    } else {
                        run::discard(&into, waiting)
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                if let Some(run) = &mut this.run {
                    match ended {
                        Ok(ended) => {
                            let title = |path: &str| {
                                let path = VaultPath::new(path).ok()?;
                                vault.at(&path).map(|doc| doc.title())
                            };
                            run.finish(ended, title);
                        }
                        Err(e) => run
                            .problems
                            .push(format!("The changes could not be discarded: {e}.")),
                    }
                }
                this.scroll.scroll_to_bottom();
                cx.notify();
            });
        })
        .detach();
    }

    fn key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            cx.stop_propagation();
            cx.emit(Closed);
        }
    }

    /// What the agent will know of where the learner is, and a button to leave it out.
    fn here_part(&self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let knows = self.here.label();
        let line = div()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_size(px(Type::META))
            .text_color(t.secondary);
        if knows.is_empty() {
            return line.child("Works on your whole vault").into_any_element();
        }
        div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(line.child(format!("Knows {knows}")))
            .child(
                incipit_ui::icon_button(
                    "agent-leave-out",
                    IconName::Close,
                    "Leave out where I am",
                    t,
                )
                .on_click(cx.listener(|this, _, _, cx| this.leave_out_here(cx))),
            )
            .into_any_element()
    }

    /// A run: the request, the agent's latest steps, its words, and what it changed.
    fn run_part(&self, run: &Run, t: Theme) -> AnyElement {
        let steps = run.shown_steps().iter().map(|step| {
            div()
                .truncate()
                .text_size(px(Type::META))
                .text_color(t.tertiary)
                .child(step.clone())
        });
        let standing = run.standing().map(|line| {
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .text_color(t.secondary)
                .when(run.working(), |row| {
                    row.child(spinner("agent-working", 12.0, t.secondary))
                })
                .child(line)
        });
        let files = run.files.iter().enumerate().map(|(index, file)| {
            div()
                .id(("agent-file", index))
                .py(px(3.0))
                .flex()
                .items_baseline()
                .gap(px(10.0))
                .child(div().flex_none().child(file.title.clone()))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(Type::META))
                        .text_color(t.tertiary)
                        .child(file.path.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(px(Type::META))
                        .text_color(t.tertiary)
                        .child(file.what),
                )
        });
        let body = div()
            .id("agent-run")
            .max_h(px(RUN))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .flex()
            .flex_col()
            .gap(px(10.0))
            .when(!run.steps.is_empty(), |body| {
                body.child(div().flex().flex_col().gap(px(3.0)).children(steps))
            })
            .when(!run.words.trim().is_empty(), |body| {
                body.child(incipit_ui::markdown("agent-words", &run.words, t))
            })
            .children(
                run.problems
                    .iter()
                    .chain(&run.review)
                    .map(|problem| div().text_color(t.danger).child(problem.clone())),
            )
            .children(standing)
            .when(!run.files.is_empty(), |body| {
                body.child(div().flex().flex_col().children(files))
            });
        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(run.request.clone()),
            )
            .child(body)
            .into_any_element()
    }
}

impl Render for AgentView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        let working = self.working();
        let can_undo = self.run.as_ref().is_some_and(Run::can_undo);
        let waits = self.run.as_ref().is_some_and(Run::waits);
        let to_open = self.run.as_ref().and_then(Run::to_open).map(str::to_owned);
        let buttons = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .when_some(to_open, |row, path| {
                row.child(
                    button("agent-open", "Open", t)
                        .tooltip(path.clone())
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.emit(OpenFile(path.clone()));
                        })),
                )
            })
            .when(can_undo, |row| {
                row.child(
                    button("agent-undo", "Undo", t)
                        .icon(IconName::Refresh)
                        .disabled(self.busy)
                        .on_click(cx.listener(|this, _, _, cx| this.undo(cx))),
                )
            })
            .when(waits, |row| {
                row.child(
                    button("agent-apply", "Apply anyway", t)
                        .disabled(self.busy)
                        .on_click(cx.listener(|this, _, _, cx| this.apply(cx))),
                )
                .child(
                    button("agent-discard", "Discard", t)
                        .disabled(self.busy)
                        .on_click(cx.listener(|this, _, _, cx| this.discard(cx))),
                )
            })
            .child(div().flex_1())
            .when(working, |row| {
                row.child(
                    button("agent-stop", "Stop", t)
                        .on_click(cx.listener(|this, _, _, cx| this.stop(cx))),
                )
            })
            .child(
                button("agent-close", "Close", t)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(Closed))),
            )
            .child(
                button("agent-run-it", "Run", t)
                    .accent()
                    .disabled(working || self.busy)
                    .on_click(cx.listener(|this, _, _, cx| this.run(cx))),
            );
        let content = div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .on_key_down(cx.listener(|this, event, _, cx| this.key(event, cx)))
            .child(field(&self.words, t))
            .child(self.here_part(t, cx))
            .children(
                self.problem
                    .clone()
                    .map(|problem| div().text_color(t.danger).child(problem)),
            )
            .children(self.run.as_ref().map(|run| self.run_part(run, t)))
            .child(buttons);
        sheet("agent", WIDTH, t, content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(status: Status, answer: &str, files: &[(&str, FileStatus)]) -> Outcome {
        Outcome {
            status,
            answer: answer.to_owned(),
            open: None,
            files: files
                .iter()
                .map(|(path, status)| FileChange {
                    path: (*path).to_owned(),
                    status: *status,
                })
                .collect(),
            change: (status == Status::Merged).then(|| "change".to_owned()),
            error: None,
            review: Vec::new(),
        }
    }

    fn no_title(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn steps_are_kept_in_order_without_repeats_and_the_latest_are_shown() {
        let mut run = Run::new("x");
        run.hear(Event::Activity("  Reading a.md ".into()));
        run.hear(Event::Activity("Reading a.md".into()));
        run.hear(Event::Activity(" ".into()));
        run.hear(Event::Link("https://example.com".into()));
        assert_eq!(run.steps, ["Reading a.md"]);
        for n in 0..STEPS_KEPT + 5 {
            run.hear(Event::Activity(format!("Step {n}")));
        }
        assert_eq!(run.steps.len(), STEPS_KEPT);
        let shown = run.shown_steps();
        assert_eq!(shown.len(), STEPS_SHOWN);
        assert_eq!(shown.last().unwrap(), &format!("Step {}", STEPS_KEPT + 4));
    }

    #[test]
    fn the_reply_is_shown_once_the_run_ends_and_not_the_words_on_the_way() {
        let mut run = Run::new("x");
        run.hear(Event::Text("{\"reply\": \"I ad".into()));
        assert_eq!(run.words, "");
        assert_eq!(run.standing(), Some("Working…"));
        let files = [("cards/New.md", FileStatus::Added)];
        run.finish(
            outcome(Status::Merged, "I added one card.", &files),
            no_title,
        );
        assert_eq!(run.words, "I added one card.");
        assert!(!run.working());
        assert!(run.can_undo());
        assert_eq!(run.change.as_deref(), Some("change"));
        assert_eq!(run.standing(), None);
        assert!(run.problems.is_empty());
    }

    #[test]
    fn changed_files_are_listed_by_title_or_else_by_file_name() {
        let mut run = Run::new("x");
        let files = [
            ("notes/Waves.md", FileStatus::Modified),
            ("cards/old card.md", FileStatus::Removed),
            ("notes/Tides.md", FileStatus::Added),
        ];
        run.finish(outcome(Status::Merged, "Done.", &files), |path| {
            (path == "notes/Waves.md").then(|| "Waves and tides".to_owned())
        });
        let listed: Vec<(&str, &str, &str)> = run
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

        let mut run = Run::new("x");
        assert_eq!(run.to_open(), None, "not while it works");
        let mut answered = outcome(Status::Done, "It is in your note.", &[]);
        answered.open = Some("notes/Waves.md".into());
        run.finish(answered.clone(), no_title);
        assert_eq!(run.to_open(), Some("notes/Waves.md"));
        let mut waiting = Run::new("x");
        answered.status = Status::Review;
        waiting.finish(answered.clone(), no_title);
        assert_eq!(waiting.to_open(), None, "not before it is in the vault");
        answered.open = Some("log/reviews.jsonl".into());
        answered.status = Status::Merged;
        run.finish(answered, no_title);
        assert_eq!(run.to_open(), None);
    }

    #[test]
    fn a_run_that_changed_nothing_offers_no_undo() {
        let mut run = Run::new("x");
        run.finish(outcome(Status::Done, "Nothing to do.", &[]), no_title);
        assert!(!run.can_undo());
        assert_eq!(run.standing(), Some("No file was changed."));
    }

    #[test]
    fn a_stopped_run_changed_nothing() {
        let mut run = Run::new("x");
        run.finish(Outcome::stopped(), no_title);
        assert!(run.problems.is_empty());
        assert!(!run.can_undo());
        assert_eq!(run.standing(), Some("Stopped. No file was changed."));
    }

    #[test]
    fn a_failed_run_says_why_and_what_it_would_have_changed() {
        let mut run = Run::new("x");
        let mut failed = outcome(Status::Failed, "Done.", &[]);
        failed.error = Some("The agent could not finish: Codex: usage limit reached.".into());
        run.finish(failed, no_title);
        assert_eq!(
            run.problems,
            ["The agent could not finish: Codex: usage limit reached."]
        );
        assert!(!run.can_undo());
        assert_eq!(run.standing(), None);

        let mut clashed = Run::new("x");
        let files = [("notes/A.md", FileStatus::Modified)];
        let mut failed = outcome(Status::Failed, "Done.", &files);
        failed.error = Some("It clashes with edits made while the agent worked.".into());
        clashed.finish(failed, no_title);
        assert!(!clashed.can_undo());
        assert_eq!(
            clashed.standing(),
            Some("Nothing reached your vault. It would have changed these files:")
        );
    }

    #[test]
    fn a_change_waiting_for_the_learners_ok_says_why_and_offers_no_undo() {
        let mut run = Run::new("x");
        let files = [("notes/Waves.md", FileStatus::Added)];
        let mut waiting = outcome(Status::Review, "I wrote a note.", &files);
        waiting.change = Some("change".into());
        waiting.review = vec!["Can't be read: notes/Waves.md: broken".into()];
        run.finish(waiting.clone(), no_title);
        assert!(run.waits());
        assert!(!run.can_undo());
        assert_eq!(run.review, waiting.review);
        assert_eq!(run.waiting, Some(waiting));
        assert!(run.standing().unwrap().starts_with("Waiting for your OK"));
        // Applied, it is a landed run.
        run.finish(outcome(Status::Merged, "", &files), no_title);
        assert!(!run.waits());
        assert!(run.can_undo());
        assert!(run.review.is_empty());
        assert_eq!(run.waiting, None);
        assert_eq!(run.words, "I wrote a note.");
    }

    #[test]
    fn the_made_up_runs_wait_and_are_refused_without_a_vault() {
        let waits = Run::made_up_as("review");
        assert!(waits.waits());
        assert_eq!(waits.review.len(), 1);
        let refused = Run::made_up_as("refused");
        assert_eq!(refused.status, Some(Status::Failed));
        assert!(refused.problems[0].contains("agents may not change it"));
        assert_eq!(refused.files.len(), 1);
        assert!(!refused.can_undo());
    }

    #[test]
    fn after_undo_nothing_further_is_offered() {
        let mut run = Run::new("x");
        let files = [("notes/A.md", FileStatus::Modified)];
        run.finish(outcome(Status::Merged, "Done.", &files), no_title);
        run.undone(Err("it clashed".into()));
        assert!(run.can_undo(), "a refused undo can be tried again");
        assert_eq!(run.problems, ["it clashed"]);
        run.undone(Ok(()));
        assert!(!run.can_undo());
        assert_eq!(
            run.standing(),
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
    fn the_made_up_run_is_finished_with_three_steps_two_files_and_an_undo() {
        let run = Run::made_up();
        assert_eq!(run.steps.len(), 3);
        assert_eq!(run.files.len(), 2);
        assert!(run.can_undo());
        assert!(run.change.is_none(), "its undo touches no vault");
        assert!(!run.words.is_empty());
    }
}
