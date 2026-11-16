//! The vault agent: a sheet over the window, opened from any page with a key, where the learner
//! says what should be done in their vault, or asks about it, and the model they chose does it,
//! as an agent with a shell. The sheet says what the agent will know of where the learner is
//! (the note or concept open, the book and the place in it, the selection; `here.rs`), which the
//! learner can leave out.
//!
//! Up to four requests are worked on at once (`runs.rs`). The sheet shows one conversation: its
//! requests, newest first, each with what the agent is doing while it works and, once it ends,
//! its reply, the files it changed and what can be done with it (`turn.rs`). The next request
//! follows up in that conversation, and the agent is told its earlier turns; New starts
//! another. With no conversation on show, the recent ones are listed (or, in a vault with none,
//! a few requests to start from). A conversation is shown again when the sheet opens soon after
//! it, as the earlier app's panel did. Every run is logged in the vault (`log/agents.jsonl`), so
//! the list, Undo and the rest are there after a restart.
//!
//! The run itself has no window in it (`run.rs`): the agent works in a working copy of the vault
//! of its own, under the provider crate's OS sandbox, which lets it write there and nowhere else
//! and read the vault. The learner's words are the request. Where they are, the conversation so
//! far, and what the agent reads in the files are material, and the instructions (`prompt.rs`),
//! which hold no text from any file, say so. The words it writes on the way are not shown: it
//! ends with a reply in JSON.
//!
//! One request is one change in the vault's history, made of the agent's work alone. When the
//! agent ends, its change lands after whatever the learner wrote meanwhile, is read into the
//! index and announced, so open pages follow; one that changes the app's own files or clashes
//! with the learner's edits, or an agent that failed or was stopped, lands nothing. Undo takes
//! that change out and keeps what was written before and after it; it is refused when later
//! edits touched the same lines. "Show the changes" reads that change as a diff. The runs and
//! the history calls block, so they run off the window's thread and the sheet awaits them.
//!
//! Closing the sheet does not stop the agent: it goes on, and the key shows it again. Stop does,
//! and so does opening another vault.
//!
//! Without clicking: INCIPIT_AGENT=demo shows a made-up run that landed, `review` one waiting for
//! the learner's OK, `refused` one refused for changing the review log, `thread` a conversation
//! of an answer and a follow-up, and `recent` the list of recent conversations with one
//! working, and `diff` the first with its changes shown ("Show the changes"); none calls a
//! model, and what is done with them changes nothing. INCIPIT_AGENT=open
//! opens the sheet as the key does, on the vault's own runs. INCIPIT_AGENT with other words runs
//! them once the vault is open, with the learner's login. On the Reader page,
//! the sheet knows the book and the selection once the reader has them.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures::StreamExt;
use gpui::{
    div, prelude::*, px, AnyElement, Context, Entity, EventEmitter, FontWeight, IntoElement,
    KeyDownEvent, Render, ScrollHandle, SharedString, Subscription, Window,
};
use incipit_llm::Update;
use incipit_ui::{button, field, sheet, spinner, FieldEvent, IconName, TextField, Theme, Type};
use incipit_vault::{rfc3339, AgentRun, Vault, VaultPath};

use crate::learning::learner;
use crate::models::Models;
use crate::study::now;
use crate::workspace::{Happened, Workspace};
use here::Here;
use run::{Job, Status};
use turn::{status_word, undo_problem, Changes, Turn, DIFF_LINES};

pub mod guide;
pub mod here;
pub mod prompt;
pub mod run;
pub mod runs;
pub mod turn;

/// What the learner is told when no vault is open.
const NO_VAULT: &str = "No vault is open. Choose its folder in Settings first.";
/// The sheet's width, and the most height the runs take before they scroll: with the field and
/// the buttons, the sheet fits the smallest window.
const WIDTH: f32 = 560.0;
const RUNS: f32 = 340.0;
/// How many recent conversations are listed.
const RECENT: usize = 6;
/// The most characters of a recent conversation's first request on its button.
const RECENT_TASK: usize = 64;
/// Requests to start from, in a vault with none yet: the earlier app's.
const EXAMPLES: [&str; 4] = [
    "Summarise this chapter into a note in a folder for this book",
    "Make a quiz on what I highlighted, with corrections",
    "Turn this note into exercises with hidden solutions",
    "What have I written about consensus?",
];
/// What the field says while it is empty.
const ASK: &str = "What should be done in your vault?";
const REPLY: &str = "Reply, or ask for a change…";

/// The learner closed the sheet.
pub struct Closed;

/// The learner asked to open a file of the vault the runs are in, by its path there.
pub struct OpenFile(pub String);

/// The title of the document at a path, as `vault` has it now.
fn titles(vault: &Vault) -> impl Fn(&str) -> Option<String> {
    let vault = vault.clone();
    move |path| {
        let path = VaultPath::new(path).ok()?;
        vault.at(&path).map(|doc| doc.title())
    }
}

/// Something done to a run, by its id.
type Act = fn(&mut AgentView, &str, &mut Context<AgentView>);

/// A click that does `act` to the run `id`.
fn on_turn(
    id: &str,
    act: Act,
    cx: &mut Context<AgentView>,
) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static {
    let id = id.to_owned();
    let view = cx.entity().downgrade();
    move |_, _, cx| {
        let _ = view.update(cx, |this, cx| act(this, &id, cx));
    }
}

/// A click that opens the file at `path`.
fn opens(
    path: &str,
    cx: &mut Context<AgentView>,
) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static {
    let path = path.to_owned();
    let view = cx.entity().downgrade();
    move |_, _, cx| {
        let _ = view.update(cx, |_, cx| cx.emit(OpenFile(path.clone())));
    }
}

/// What is done to a run in the vault, off the window's thread: Undo, Apply anyway, Discard,
/// Keep as a note. It gives the run as it then stands, or why not, in words for the learner.
type Work = Box<dyn FnOnce(&Vault, AgentRun) -> Result<AgentRun, String> + Send>;

pub struct AgentView {
    workspace: Entity<Workspace>,
    models: Entity<Models>,
    words: Entity<TextField>,
    /// The runs of the vault, newest first.
    turns: Vec<Turn>,
    /// The vault the runs are of: what is done with them goes there.
    vault: Option<Vault>,
    /// The runs have been read from the vault's log.
    loaded: bool,
    /// What tells each run going on that another vault opened, by its id.
    closing: HashMap<String, Arc<AtomicBool>>,
    /// The conversation on show, by its thread; none for a fresh request.
    thread: Option<String>,
    /// The runs are made up (INCIPIT_AGENT=demo and the like): no vault replaces them.
    demo: bool,
    /// Why nothing was started, in words for the learner.
    problem: Option<String>,
    /// Words to run as soon as the vault is open and its runs are read.
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
        let words = cx.new(|cx| TextField::new(cx).placeholder(ASK));
        let typed = cx.subscribe(
            &words,
            |this: &mut Self, _, event: &FieldEvent, cx| match event {
                FieldEvent::Changed => {}
                FieldEvent::Submit => this.run(cx),
                FieldEvent::Cancel => cx.emit(Closed),
            },
        );
        let opened = cx.subscribe(&workspace, |this: &mut Self, _, happened, cx| {
            if matches!(happened, Happened::Opened) {
                this.opened(cx);
            }
        });
        let mut view = AgentView {
            workspace,
            models,
            words,
            turns: Vec::new(),
            vault: None,
            loaded: false,
            closing: HashMap::new(),
            thread: None,
            demo: false,
            problem: None,
            waiting: None,
            here: Here::default(),
            scroll: ScrollHandle::new(),
            _heard: [typed, opened],
        };
        if view.workspace.read(cx).vault.is_some() {
            view.opened(cx);
        }
        view
    }

    /// Take the keyboard: the sheet was opened where the learner is, `here`. The conversation
    /// on show stays while it goes on or was asked lately.
    pub fn start(&mut self, here: Here, window: &mut Window, cx: &mut Context<Self>) {
        self.here = here;
        let records: Vec<AgentRun> = self.turns.iter().map(|t| t.run.clone()).collect();
        if let Some(thread) = &self.thread {
            if !runs::fresh(&records, thread, now()) {
                self.show_thread(None, cx);
            }
        }
        TextField::focus(&self.words, window, cx);
        cx.notify();
    }

    /// Where the learner is, learnt after the sheet opened (INCIPIT_AGENT on the Reader page).
    pub fn know(&mut self, here: Here, cx: &mut Context<Self>) {
        self.here = here;
        cx.notify();
    }

    /// INCIPIT_AGENT=demo, diff, review, refused, thread or recent: made-up runs, with no model
    /// called. What is done with them changes no vault.
    pub fn show_demo(&mut self, which: &str, cx: &mut Context<Self>) {
        self.demo = true;
        self.turns = turn::made_up(which, now());
        let thread = (which != "recent")
            .then(|| {
                self.turns
                    .first()
                    .map(|t| runs::thread_of(&t.run).to_owned())
            })
            .flatten();
        self.show_thread(thread, cx);
        cx.notify();
    }

    /// INCIPIT_AGENT=words: run them, now or as soon as the vault is open and its runs are
    /// read.
    pub fn run_words(&mut self, words: &str, cx: &mut Context<Self>) {
        self.words.update(cx, |field, cx| field.set_text(words, cx));
        if self.loaded {
            self.run(cx);
        } else {
            self.waiting = Some(words.to_owned());
        }
    }

    /// Another vault is open (or none): the runs going in the one that was are stopped, as the
    /// earlier app's `stopRuns`, and the new vault's runs are read from its log.
    fn opened(&mut self, cx: &mut Context<Self>) {
        self.vault = self.workspace.read(cx).vault.clone();
        if self.demo {
            self.loaded = true;
            return;
        }
        for turn in self.turns.iter().filter(|t| t.working()) {
            if let Some(closing) = self.closing.get(&turn.run.id) {
                closing.store(true, Ordering::SeqCst);
            }
            if let Some(stop) = &turn.stop {
                stop.cancel();
            }
        }
        self.closing.clear();
        self.turns.clear();
        self.loaded = false;
        self.show_thread(None, cx);
        cx.notify();
        let Some(vault) = self.vault.clone() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let reading = vault.clone();
            let read = cx
                .background_executor()
                .spawn(async move { runs::load(&reading) })
                .await;
            let _ = this.update(cx, |this, cx| {
                // Another vault opened while this one's log was read.
                if this.vault.as_ref().map(Vault::root) != Some(vault.root()) {
                    return;
                }
                let title = titles(&vault);
                this.turns = read.into_iter().map(|run| Turn::new(run, &title)).collect();
                this.loaded = true;
                if this.waiting.is_some() {
                    this.run(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Shows a conversation (none: a fresh request).
    fn show_thread(&mut self, thread: Option<String>, cx: &mut Context<Self>) {
        let placeholder = if thread.is_some() { REPLY } else { ASK };
        self.thread = thread;
        self.words
            .update(cx, |field, cx| field.set_placeholder(placeholder, cx));
    }

    fn turn_mut(&mut self, id: &str) -> Option<&mut Turn> {
        self.turns.iter_mut().find(|t| t.run.id == id)
    }

    /// The learner leaves out where they are: the agent works on the whole vault.
    fn leave_out_here(&mut self, cx: &mut Context<Self>) {
        self.here = self.here.page_only();
        cx.notify();
    }

    /// Have the agent do what is in the field: a follow-up in the conversation on show, else a
    /// new one.
    fn run(&mut self, cx: &mut Context<Self>) {
        let words = self.words.read(cx).text().trim().to_string();
        self.waiting = None;
        if words.is_empty() {
            return;
        }
        let Some(vault) = self.workspace.read(cx).vault.clone() else {
            if self.demo {
                return;
            }
            self.problem = Some(NO_VAULT.to_owned());
            cx.notify();
            return;
        };
        if !self.loaded || self.vault.as_ref().map(Vault::root) != Some(vault.root()) {
            // The runs are read first, so a follow-up knows its conversation and the run is not
            // taken for one cut short.
            self.waiting = Some(words);
            return;
        }
        let running = self.turns.iter().filter(|t| t.working()).count();
        let after = self.thread.as_deref().and_then(|thread| {
            self.turns
                .iter()
                .find(|t| runs::thread_of(&t.run) == thread)
                .map(|t| t.run.clone())
        });
        let described = here::describe(&vault, &self.here);
        let record = match runs::begin(
            &words,
            after.as_ref(),
            &described.label,
            running,
            rfc3339(now()),
        ) {
            Ok(record) => record,
            Err(problem) => {
                self.problem = Some(problem);
                cx.notify();
                return;
            }
        };
        let records: Vec<AgentRun> = self.turns.iter().map(|t| t.run.clone()).collect();
        let earlier = runs::earlier(&runs::turns_before(&records, &record));
        let request = prompt::request_for(
            &record.task,
            &described.lines,
            &earlier,
            &learner::of(&vault, now()),
        )
        .with_timeout(runs::TIME_LIMIT);
        let job = Job {
            runs: run::runs_folder(&crate::workspace::home(), &vault),
            id: record.id.clone(),
            task: record.task.clone(),
            vault: vault.clone(),
        };
        let closing = Arc::new(AtomicBool::new(false));
        let (going, closed) = (record.clone(), closing.clone());
        let asking = self
            .models
            .read(cx)
            .act(request, move |provider, request, cancel, on| {
                Ok(runs::go(
                    provider, request, &job, going, cancel, &closed, on,
                ))
            });
        let mut asking = match asking {
            Ok(asking) => asking,
            Err(problem) => {
                self.problem = Some(problem);
                cx.notify();
                return;
            }
        };
        self.problem = None;
        self.words.update(cx, |field, cx| field.set_text("", cx));
        self.show_thread(Some(runs::thread_of(&record).to_owned()), cx);
        let id = record.id.clone();
        let mut turn = Turn::new(record, |_| None);
        turn.stop = Some(asking.cancel.clone());
        self.turns.insert(0, turn);
        self.closing.insert(id.clone(), closing);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let mut ended = None;
            while let Some(update) = asking.updates.next().await {
                match update {
                    Update::Event(event) => {
                        let heard = this.update(cx, |this, cx| {
                            if let Some(turn) = this.turn_mut(&id) {
                                turn.hear(event);
                            }
                            cx.notify();
                        });
                        if heard.is_err() {
                            asking.cancel.cancel();
                        }
                    }
                    Update::Done(result) => {
                        ended = result.ok();
                        break;
                    }
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.closing.remove(&id);
                let title = titles(&vault);
                let Some(turn) = this.turn_mut(&id) else {
                    return;
                };
                let run = ended.unwrap_or_else(|| {
                    // A channel that closes without an end is a run that was dropped: as if
                    // stopped, and logged so.
                    let stopped = AgentRun {
                        status: Status::Stopped,
                        ..turn.run.clone()
                    };
                    let (into, line) = (vault.clone(), stopped.clone());
                    cx.background_executor()
                        .spawn(async move { runs::save(&into, &line) })
                        .detach();
                    stopped
                });
                turn.finish(run, title);
                cx.notify();
            });
        })
        .detach();
    }

    /// Stop a run. Nothing it changed reaches the vault.
    fn stop(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(stop) = self.turn_mut(id).and_then(|t| t.stop.as_ref()) {
            stop.cancel();
        }
        cx.notify();
    }

    /// Ask a failed request again, with where the learner is now, in the conversation on show.
    fn try_again(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(task) = self.turn_mut(id).map(|t| t.run.task.clone()) else {
            return;
        };
        self.words.update(cx, |field, cx| field.set_text(task, cx));
        self.run(cx);
    }

    /// Does `work` to run `id` in its vault, off the window's thread; a made-up run becomes what
    /// `pretend` makes of it, and no vault is touched.
    fn act_on(
        &mut self,
        id: &str,
        pretend: fn(AgentRun) -> AgentRun,
        work: Work,
        cx: &mut Context<Self>,
    ) {
        let vault = self.vault.clone();
        let Some(turn) = self.turn_mut(id) else {
            return;
        };
        if turn.busy {
            return;
        }
        if turn.made_up {
            let run = pretend(turn.run.clone());
            turn.finish(run, turn::made_up_title);
            cx.notify();
            return;
        }
        let Some(vault) = vault else {
            return;
        };
        turn.busy = true;
        let run = turn.run.clone();
        let id = id.to_owned();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let into = vault.clone();
            let result = cx
                .background_executor()
                .spawn(async move { work(&into, run) })
                .await;
            let _ = this.update(cx, |this, cx| {
                let title = titles(&vault);
                if let Some(turn) = this.turn_mut(&id) {
                    turn.busy = false;
                    match result {
                        Ok(run) => turn.finish(run, title),
                        Err(problem) => turn.problems.push(problem),
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Shows the changes a run made as a diff, or hides them again.
    fn toggle_changes(&mut self, id: &str, cx: &mut Context<Self>) {
        let vault = self.vault.clone();
        let Some(turn) = self.turn_mut(id) else {
            return;
        };
        if turn.changes != Changes::Hidden {
            turn.changes = Changes::Hidden;
            cx.notify();
            return;
        }
        if turn.made_up {
            turn.changes = Changes::Shown(turn::MADE_UP_DIFF.to_owned());
            cx.notify();
            return;
        }
        let (Some(vault), Some(commit)) = (vault, turn.run.commit.clone()) else {
            return;
        };
        turn.changes = Changes::Reading;
        let id = id.to_owned();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let diff = cx
                .background_executor()
                .spawn(async move { vault.history().diff(&commit, None).unwrap_or_default() })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(turn) = this.turn_mut(&id) {
                    if turn.changes == Changes::Reading {
                        turn.changes = Changes::Shown(diff);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Take a run's change out of the vault, and nothing else.
    fn undo(&mut self, id: &str, cx: &mut Context<Self>) {
        self.act_on(
            id,
            |run| AgentRun {
                status: Status::Undone,
                ..run
            },
            Box::new(|vault, run| {
                let change = run
                    .change
                    .clone()
                    .ok_or("Only a request that changed your vault can be undone.")?;
                vault.drop_change(&change).map_err(|e| undo_problem(&e))?;
                let undone = AgentRun {
                    status: Status::Undone,
                    ..run
                };
                runs::save(vault, &undone);
                Ok(undone)
            }),
            cx,
        );
    }

    /// The learner's OK for a change that waited for it: it lands as any other.
    fn apply(&mut self, id: &str, cx: &mut Context<Self>) {
        self.act_on(
            id,
            |run| AgentRun {
                status: Status::Merged,
                review: Vec::new(),
                ..run
            },
            Box::new(|vault, run| {
                let landed = run::approve(vault, &run.task, runs::outcome_of(&run));
                let ended = runs::with_outcome(&run, landed);
                runs::save(vault, &ended);
                Ok(ended)
            }),
            cx,
        );
    }

    /// The learner turned down a change that waited for their OK: it is left out.
    fn discard(&mut self, id: &str, cx: &mut Context<Self>) {
        self.act_on(
            id,
            |run| AgentRun {
                status: Status::Discarded,
                review: Vec::new(),
                ..run
            },
            Box::new(|vault, run| {
                let discarded = run::discard(vault, runs::outcome_of(&run))
                    .map_err(|e| format!("The changes could not be discarded: {e}."))?;
                let ended = runs::with_outcome(&run, discarded);
                runs::save(vault, &ended);
                Ok(ended)
            }),
            cx,
        );
    }

    /// Keep an answer as a note.
    fn keep(&mut self, id: &str, cx: &mut Context<Self>) {
        self.act_on(
            id,
            |run| AgentRun {
                kept: Some(format!("notes/{}.md", run.task.trim_end_matches('?'))),
                ..run
            },
            Box::new(|vault, run| runs::keep(vault, &run, now())),
            cx,
        );
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

    /// What can be done with a run, as buttons.
    fn actions(&self, ix: usize, turn: &Turn, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let id = turn.run.id.as_str();
        let on = |f: Act, cx: &mut Context<Self>| on_turn(id, f, cx);
        let mut row = div().flex().flex_wrap().items_center().gap(px(6.0));
        if let Some(path) = turn.to_open() {
            row = row.child(
                button(("agent-open", ix), "Open", t)
                    .accent()
                    .tooltip(SharedString::from(path.to_owned()))
                    .on_click(opens(path, cx)),
            );
        }
        if turn.can_undo() {
            row = row.child(
                button(("agent-undo", ix), "Undo", t)
                    .icon(IconName::Refresh)
                    .disabled(turn.busy)
                    .on_click(on(Self::undo, cx)),
            );
        }
        if turn.waits() {
            row = row
                .child(
                    button(("agent-apply", ix), "Apply anyway", t)
                        .disabled(turn.busy)
                        .on_click(on(Self::apply, cx)),
                )
                .child(
                    button(("agent-discard", ix), "Discard", t)
                        .disabled(turn.busy)
                        .on_click(on(Self::discard, cx)),
                );
        }
        if turn.can_keep() {
            row = row.child(
                button(("agent-keep", ix), "Keep as a note", t)
                    .disabled(turn.busy)
                    .on_click(on(Self::keep, cx)),
            );
        }
        if let Some(path) = turn.kept() {
            row = row.child(
                button(("agent-kept", ix), "Kept · open the note", t)
                    .ghost()
                    .on_click(opens(path, cx)),
            );
        }
        if turn.run.status == Status::Failed {
            row = row.child(
                button(("agent-again", ix), "Try again", t).on_click(on(Self::try_again, cx)),
            );
        }
        if turn.can_show_changes() {
            let shown = turn.changes != Changes::Hidden;
            row = row.child(
                button(
                    ("agent-changes", ix),
                    if shown {
                        "Hide the changes"
                    } else {
                        "Show the changes"
                    },
                    t,
                )
                .ghost()
                .on_click(on(Self::toggle_changes, cx)),
            );
        }
        if turn.working() {
            row = row.child(
                button(("agent-stop", ix), "Stop", t)
                    .icon(IconName::Stop)
                    .on_click(on(Self::stop, cx)),
            );
        }
        row.into_any_element()
    }

    /// One run: the request, how it stands, the agent's latest steps while it works, its reply,
    /// what went wrong, the files it changed, and what can be done with it.
    fn turn_part(
        &self,
        ix: usize,
        turn: &Turn,
        t: Theme,
        now: i64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let steps = turn.shown_steps().iter().map(|step| {
            div()
                .truncate()
                .text_size(px(Type::META))
                .text_color(t.tertiary)
                .child(step.clone())
        });
        let standing = turn.standing().map(|line| {
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .text_color(t.secondary)
                .when(turn.working(), |row| {
                    row.child(spinner(("agent-working", ix), 12.0, t.secondary))
                })
                .child(line)
        });
        let files = turn.files.iter().enumerate().map(|(index, file)| {
            div()
                .id(SharedString::from(format!("agent-file-{ix}-{index}")))
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
        let problems = turn
            .run
            .error
            .iter()
            .chain(&turn.run.review)
            .chain(&turn.problems)
            .map(|problem| div().text_color(t.danger).child(problem.clone()));
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(turn.run.task.clone()),
            )
            .child(
                div()
                    .text_size(px(Type::META))
                    .text_color(t.tertiary)
                    .child(turn.meta(now)),
            )
            .when(turn.working() && !turn.steps.is_empty(), |body| {
                body.child(div().flex().flex_col().gap(px(3.0)).children(steps))
            })
            .when(!turn.run.answer.trim().is_empty(), |body| {
                body.child(incipit_ui::markdown(
                    SharedString::from(format!("agent-reply-{ix}")),
                    &turn.run.answer,
                    t,
                ))
            })
            .children(problems)
            .children(standing)
            .when(!turn.files.is_empty(), |body| {
                body.child(div().flex().flex_col().children(files))
            })
            .child(self.actions(ix, turn, t, cx))
            .children(match &turn.changes {
                Changes::Hidden => None,
                Changes::Reading => Some(
                    div()
                        .text_size(px(Type::META))
                        .text_color(t.tertiary)
                        .child("Reading the changes…")
                        .into_any_element(),
                ),
                Changes::Shown(diff) => {
                    Some(incipit_ui::diff(("agent-diff", ix), diff, DIFF_LINES, t))
                }
            })
            .into_any_element()
    }

    /// The recent conversations, each a button that shows it.
    fn recent_part(&self, t: Theme, now: i64, cx: &mut Context<Self>) -> Option<AnyElement> {
        let records: Vec<AgentRun> = self.turns.iter().map(|t| t.run.clone()).collect();
        let conversations = runs::conversations(&records);
        if conversations.is_empty() {
            return None;
        }
        let rows = conversations
            .into_iter()
            .take(RECENT)
            .enumerate()
            .map(|(ix, c)| {
                let mut task: String = c.task.lines().next().unwrap_or_default().to_owned();
                if task.chars().count() > RECENT_TASK {
                    task = task.chars().take(RECENT_TASK).collect::<String>() + "…";
                }
                let turns = if c.turns > 1 {
                    format!(" · {} turns", c.turns)
                } else {
                    String::new()
                };
                let detail = format!(
                    "{} · {}{turns}",
                    status_word(c.status),
                    runs::ago(&c.at, now)
                );
                let thread = c.thread.clone();
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(
                        button(("agent-recent", ix), task, t)
                            .ghost()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.show_thread(Some(thread.clone()), cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(Type::META))
                            .text_color(t.tertiary)
                            .child(detail),
                    )
            });
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(Type::META))
                        .text_color(t.secondary)
                        .child("Recent"),
                )
                .children(rows)
                .into_any_element(),
        )
    }

    /// Requests to start from, each a button that puts it in the field.
    fn examples_part(&self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(4.0))
            .children(EXAMPLES.iter().enumerate().map(|(ix, example)| {
                button(("agent-example", ix), *example, t)
                    .ghost()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.words
                            .update(cx, |field, cx| field.set_text(*example, cx));
                        TextField::focus(&this.words, window, cx);
                    }))
            }))
            .into_any_element()
    }
}

impl Render for AgentView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        let now = now();
        let shown: Vec<AnyElement> = match &self.thread {
            Some(thread) => self
                .turns
                .iter()
                .enumerate()
                .filter(|(_, turn)| runs::thread_of(&turn.run) == thread)
                .map(|(ix, turn)| self.turn_part(ix, turn, t, now, cx))
                .collect(),
            None => vec![self
                .recent_part(t, now, cx)
                .unwrap_or_else(|| self.examples_part(t, cx))],
        };
        let body = div()
            .id("agent-runs")
            .max_h(px(RUNS))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .flex()
            .flex_col()
            .gap(px(20.0))
            .children(shown);
        let buttons = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .when(self.thread.is_some(), |row| {
                row.child(
                    button("agent-new", "New", t)
                        .icon(IconName::Plus)
                        .tooltip("Start another conversation")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_thread(None, cx);
                            cx.notify();
                        })),
                )
            })
            .child(div().flex_1())
            .child(
                button("agent-close", "Close", t)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(Closed))),
            )
            .child(
                button("agent-run-it", "Run", t)
                    .accent()
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
            .child(body)
            .child(buttons);
        sheet("agent", WIDTH, t, content)
    }
}
