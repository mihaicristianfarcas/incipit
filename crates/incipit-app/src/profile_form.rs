//! The form that asks who the learner is and how they learn, the earlier app's `ProfileForm`:
//! their name and role, what they want to learn and why, their background and level, interests,
//! how much they want pictures, words and things to touch, and anything else in their own words
//! (Settings' You); the new cards a day, the recall they aim for and whether the schedule is
//! fitted to their reviews (Settings' Flashcards). It is kept as `profile.md` in the open vault
//! (`incipit_vault::Profile`).
//!
//! In Settings a change is kept as it is made, as asides' settings are: a choice at once, typing
//! once it pauses ([`TYPING_PAUSE`]), so there is no Save button. Each part is a form of its own
//! on the same file; a form hears the file change and takes what another part (or Obsidian)
//! wrote, and leaves alone a field whose words already say the same, so the field being typed in
//! keeps its spaces. The first run shows the whole form with a button that keeps it and marks
//! the first run done, as the earlier app's did.
//!
//! The earlier app's three sliders are choices here (a little, some, a lot; 80, 85, 90 or 95 per
//! cent), since the app has no slider; a value kept in the file that none of them is stays as it
//! is until the learner picks another.
//!
//! Beside the target recall, a line says whether the flashcards' schedule is fitted to the
//! learner's reviews, from how many and when (`crate::fitted`), with a switch back to the
//! standard schedule. The first run leaves it out: there are no reviews yet.

use std::time::Duration;

use gpui::{
    div, prelude::*, px, AnyElement, Context, Entity, EventEmitter, FontWeight, SharedString,
    Subscription, Window,
};
use incipit_ui::{
    button, card, field, footnote, segmented, select, setting, switch, FieldEvent, TextField,
    Theme, Type,
};
use incipit_vault::{Level, Profile, Role};

use crate::fitted::{self, Fitting};
use crate::workspace::{Happened, Workspace};

/// The first run's form was kept, or skipped.
pub struct Saved;

impl EventEmitter<Saved> for ProfileForm {}

/// How long typing pauses before what was typed is kept: long enough that a word typed at an
/// ordinary pace is kept once, not letter by letter.
const TYPING_PAUSE: Duration = Duration::from_millis(700);

/// Which of the profile a form asks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Part {
    /// Who the learner is and how they like to learn: Settings' You.
    You,
    /// The flashcards' schedule: Settings' Flashcards.
    Flashcards,
    /// All of it but the fitted schedule, kept with a button: the first run.
    Whole,
}

/// The ways to say how much of something the learner wants: its words and the value each keeps.
const WANTS: [(&str, f64); 3] = [("A little", 0.2), ("Some", 0.5), ("A lot", 0.9)];

/// What each way of learning is, in the earlier app's words.
const STYLES: [(&str, &str); 3] = [
    ("Seeing", "Pictures, diagrams and animation"),
    ("Reading", "Careful prose, definitions, narration"),
    ("Doing", "Sliders, dragging, predicting, breaking things"),
];

/// The target recalls offered.
const RECALLS: [f64; 4] = [0.80, 0.85, 0.90, 0.95];

/// Which of `WANTS` a value reads as: below 0.4 a little, from 0.7 a lot (the bands the
/// paragraph about the learner uses).
fn wants_at(value: f64) -> usize {
    if value >= 0.7 {
        2
    } else if value >= 0.4 {
        1
    } else {
        0
    }
}

/// The recall among `RECALLS` nearest to `value`.
fn recall_at(value: f64) -> usize {
    (0..RECALLS.len())
        .min_by(|a, b| {
            (RECALLS[*a] - value)
                .abs()
                .total_cmp(&(RECALLS[*b] - value).abs())
        })
        .unwrap_or(2)
}

/// The labels of the roles and levels, in the order of `Role::ALL` and `Level::ALL`.
const ROLES: [&str; 4] = [
    "Student",
    "Professor or teacher",
    "Professional",
    "Curious learner",
];
const LEVELS: [&str; 4] = ["Beginner", "Intermediate", "Advanced", "Expert"];

/// The interests as the learner types them: separated by commas.
fn interests_of(text: &str) -> Vec<String> {
    text.split(',')
        .map(|i| i.trim().to_owned())
        .filter(|i| !i.is_empty())
        .collect()
}

/// New cards a day as typed: a whole number, else none (the old one stays).
fn daily_of(text: &str) -> Option<u32> {
    let n: f64 = text.trim().parse().ok().filter(|n: &f64| n.is_finite())?;
    Some(
        n.round()
            .clamp(0.0, f64::from(incipit_vault::profile::MOST_NEW)) as u32,
    )
}

pub struct ProfileForm {
    workspace: Entity<Workspace>,
    part: Part,
    /// What the vault has (as this form last read or kept it), with the choices made since.
    profile: Profile,
    name: Entity<TextField>,
    goals: Entity<TextField>,
    background: Entity<TextField>,
    interests: Entity<TextField>,
    daily: Entity<TextField>,
    notes: Entity<TextField>,
    /// What the button that keeps the form says; none where changes are kept as they are made.
    submit: Option<SharedString>,
    /// Why the last change could not be kept, or how the button's save went.
    said: Option<(SharedString, bool)>,
    /// Counts the pauses waited on, so only the last one keeps what was typed.
    typing: u64,
    /// The fields have been filled from the vault once.
    filled: bool,
    /// The fitted schedule, for its line; none on the first run.
    fitting: Option<Entity<Fitting>>,
    _subscriptions: Vec<Subscription>,
}

fn text_field(
    placeholder: &'static str,
    multiline: bool,
    cx: &mut Context<ProfileForm>,
) -> Entity<TextField> {
    let field = cx.new(|cx| {
        let field = TextField::new(cx).placeholder(placeholder);
        if multiline {
            field.multiline()
        } else {
            field
        }
    });
    cx.subscribe(
        &field,
        |this: &mut ProfileForm, _, event: &FieldEvent, cx| {
            if matches!(event, FieldEvent::Changed | FieldEvent::Submit) {
                this.typed(cx);
            }
        },
    )
    .detach();
    field
}

impl ProfileForm {
    /// The form for `part`. With `submit`, a button with those words keeps it; without, every
    /// change is kept as it is made.
    pub fn new(
        workspace: Entity<Workspace>,
        part: Part,
        submit: Option<&'static str>,
        fitting: Option<Entity<Fitting>>,
        cx: &mut Context<Self>,
    ) -> Self {
        if let Some(fitting) = &fitting {
            cx.observe(fitting, |_, _, cx| cx.notify()).detach();
        }
        let subscriptions =
            vec![
                cx.subscribe(&workspace, |this: &mut Self, _, happened: &Happened, cx| {
                    match happened {
                        Happened::Opened => {
                            this.filled = false;
                            this.load(cx)
                        }
                        Happened::Changed(event)
                            if event.path.as_str() == incipit_vault::profile::PROFILE =>
                        {
                            this.load(cx)
                        }
                        Happened::Changed(_) => {}
                    }
                }),
            ];
        let mut form = ProfileForm {
            workspace,
            part,
            profile: Profile::default(),
            name: text_field("What should the tutor call you?", false, cx),
            goals: text_field(
                "e.g. Engineer software systems; specialise in agentic infrastructure and \
                 distributed systems. Understand how markets today map to Adam Smith's principles.",
                true,
                cx,
            ),
            background: text_field(
                "e.g. Software engineer, 6 years of backend work; comfortable with calculus, rusty \
                 on statistics.",
                true,
                cx,
            ),
            interests: text_field("distributed systems, AI agents, economics", false, cx),
            daily: text_field("20", false, cx),
            notes: text_field(
                "e.g. I understand things best through concrete systems I've worked on. Don't \
                 over-explain the basics.",
                true,
                cx,
            ),
            submit: submit.map(SharedString::from),
            said: None,
            typing: 0,
            filled: false,
            fitting,
            _subscriptions: subscriptions,
        };
        form.load(cx);
        form
    }

    fn vault(&self, cx: &Context<Self>) -> Option<incipit_vault::Vault> {
        self.workspace.read(cx).vault.clone()
    }

    /// Changes are kept as they are made.
    fn keeps_as_it_goes(&self) -> bool {
        self.submit.is_none()
    }

    /// Reads the profile again from the open vault into the form. The form's own last write is
    /// heard as a change of the file too, and changes nothing.
    fn load(&mut self, cx: &mut Context<Self>) {
        let read = self
            .vault(cx)
            .map(|vault| vault.profile())
            .unwrap_or_default();
        if self.filled && read == self.profile {
            return;
        }
        self.filled = true;
        self.profile = read.clone();
        let put = |field: &Entity<TextField>, text: String, cx: &mut Context<Self>| {
            if field.read(cx).text().trim() != text.trim() {
                field.update(cx, |f, cx| f.set_text(text, cx));
            }
        };
        put(&self.name, read.name, cx);
        put(&self.goals, read.goals, cx);
        put(&self.background, read.background, cx);
        put(&self.interests, read.interests.join(", "), cx);
        if daily_of(self.daily.read(cx).text()) != Some(read.daily_new) {
            self.daily
                .update(cx, |f, cx| f.set_text(read.daily_new.to_string(), cx));
        }
        put(&self.notes, read.notes, cx);
        cx.notify();
    }

    /// The profile the form shows now.
    fn made(&self, cx: &Context<Self>) -> Profile {
        let mut p = self.profile.clone();
        p.name = self.name.read(cx).text().trim().to_owned();
        p.goals = self.goals.read(cx).text().trim().to_owned();
        p.background = self.background.read(cx).text().trim().to_owned();
        p.interests = interests_of(self.interests.read(cx).text());
        p.daily_new = daily_of(self.daily.read(cx).text()).unwrap_or(p.daily_new);
        p.notes = self.notes.read(cx).text().trim().to_owned();
        p
    }

    /// Writes the profile the form shows, `onboarded` as asked, and fits the schedule if it was
    /// not fitted (`was_fitted`) and now is. Says why when it cannot.
    fn write(&mut self, onboarded: bool, was_fitted: bool, cx: &mut Context<Self>) -> bool {
        let Some(vault) = self.vault(cx) else {
            self.said = Some(("Open a vault first.".into(), true));
            cx.notify();
            return false;
        };
        let mut profile = self.made(cx);
        profile.onboarded |= onboarded;
        match vault.save_profile(&profile) {
            Ok(saved) => {
                if saved.fitted_schedule && !was_fitted {
                    // Switched on again: fit now if a fit is due.
                    if let Some(fitting) = &self.fitting {
                        fitting.update(cx, |fitting, cx| fitting.check(cx));
                    }
                }
                self.profile = saved;
                self.said = None;
                cx.notify();
                true
            }
            Err(e) => {
                self.said = Some((format!("The profile could not be kept: {e}").into(), true));
                cx.notify();
                false
            }
        }
    }

    /// The first run's button: keeps the profile and marks the first run done.
    pub fn save(&mut self, cx: &mut Context<Self>) {
        if self.write(true, self.profile.fitted_schedule, cx) {
            self.load(cx);
            cx.emit(Saved);
        }
    }

    /// Keep the profile as it is (the defaults, if nothing was kept) and mark the first run done.
    pub fn skip(&mut self, cx: &mut Context<Self>) {
        if let Some(vault) = self.vault(cx) {
            let mut profile = vault.profile();
            profile.onboarded = true;
            let _ = vault.save_profile(&profile);
        }
        cx.emit(Saved);
    }

    /// A choice changed the profile the form holds: kept at once where changes are kept as they
    /// are made.
    fn choose(&mut self, change: impl FnOnce(&mut Profile), cx: &mut Context<Self>) {
        let was_fitted = self.profile.fitted_schedule;
        change(&mut self.profile);
        self.said = None;
        if self.keeps_as_it_goes() {
            self.write(false, was_fitted, cx);
        }
        cx.notify();
    }

    /// Something was typed: kept once typing pauses, where changes are kept as they are made.
    fn typed(&mut self, cx: &mut Context<Self>) {
        if !self.keeps_as_it_goes() {
            return;
        }
        self.typing += 1;
        let typing = self.typing;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TYPING_PAUSE).await;
            let _ = this.update(cx, |this, cx| {
                if this.typing == typing && this.made(cx) != this.profile {
                    let fitted = this.profile.fitted_schedule;
                    this.write(false, fitted, cx);
                }
            });
        })
        .detach();
    }

    /// A label above a field, and a line on what it is for.
    fn asked(
        label: &'static str,
        hint: Option<&'static str>,
        control: impl IntoElement,
        t: Theme,
    ) -> AnyElement {
        div()
            .px(px(12.0))
            .py(px(10.0))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .text_size(px(Type::BODY))
                    .font_weight(FontWeight::MEDIUM)
                    .child(label),
            )
            .children(hint.map(|h| {
                div()
                    .text_size(px(Type::META))
                    .text_color(t.tertiary)
                    .child(h)
            }))
            .child(control)
            .into_any_element()
    }

    /// Who the learner is.
    fn about(&self, t: Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let me = cx.entity().downgrade();
        let p = &self.profile;
        let role = Role::ALL.iter().position(|r| *r == p.role).unwrap_or(3);
        let level = Level::ALL.iter().position(|l| *l == p.level).unwrap_or(1);
        vec![
            Self::asked("Your name", None, field(&self.name, t), t),
            setting(
                "You are a",
                None,
                select("profile-role", ROLES, Some(role), t)
                    .width(190.0)
                    .on_select({
                        let me = me.clone();
                        move |i, _, cx| {
                            let _ = me
                                .update(cx, |this, cx| this.choose(|p| p.role = Role::ALL[i], cx));
                        }
                    }),
                t,
            )
            .into_any_element(),
            Self::asked(
                "What do you want to learn, and why?",
                None,
                field(&self.goals, t).min_rows(3),
                t,
            ),
            Self::asked(
                "Your background",
                None,
                field(&self.background, t).min_rows(2),
                t,
            ),
            setting(
                "Level in what you're studying",
                None,
                select("profile-level", LEVELS, Some(level), t)
                    .width(150.0)
                    .on_select(move |i, _, cx| {
                        let _ =
                            me.update(cx, |this, cx| this.choose(|p| p.level = Level::ALL[i], cx));
                    }),
                t,
            )
            .into_any_element(),
            Self::asked(
                "Interests",
                Some("Separated by commas."),
                field(&self.interests, t),
                t,
            ),
        ]
    }

    /// How much the learner wants pictures, words and things to touch.
    fn ways(&self, t: Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = &self.profile;
        let styles = [p.styles.visual, p.styles.reader, p.styles.doer];
        STYLES
            .iter()
            .enumerate()
            .map(|(which, (name, help))| {
                let me = cx.entity().downgrade();
                setting(
                    *name,
                    Some((*help).into()),
                    segmented(
                        format!("profile-style-{which}"),
                        WANTS.map(|w| w.0),
                        wants_at(styles[which]),
                        t,
                    )
                    .on_select(move |i, _, cx| {
                        let _ = me.update(cx, |this, cx| {
                            this.choose(
                                |p| match which {
                                    0 => p.styles.visual = WANTS[i].1,
                                    1 => p.styles.reader = WANTS[i].1,
                                    _ => p.styles.doer = WANTS[i].1,
                                },
                                cx,
                            )
                        });
                    }),
                    t,
                )
                .into_any_element()
            })
            .collect()
    }

    /// The flashcards' schedule: new cards a day, the recall aimed for, and (in Settings)
    /// whether it is fitted to the learner's reviews.
    fn schedule(&self, t: Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let me = cx.entity().downgrade();
        let p = &self.profile;
        let mut rows = vec![
            Self::asked(
                "New flashcards per day",
                Some("From 0 to 200."),
                div().w(px(90.0)).child(field(&self.daily, t)),
                t,
            ),
            setting(
                format!("Target recall ({}%)", (p.retention * 100.0).round()),
                Some("How much you aim to remember when a card comes back.".into()),
                segmented(
                    "profile-recall",
                    RECALLS.map(|r| format!("{}%", (r * 100.0).round())),
                    recall_at(p.retention),
                    t,
                )
                .on_select({
                    let me = me.clone();
                    move |i, _, cx| {
                        let _ =
                            me.update(cx, |this, cx| this.choose(|p| p.retention = RECALLS[i], cx));
                    }
                }),
                t,
            )
            .into_any_element(),
        ];
        if let Some(fitting) = &self.fitting {
            let fitting = fitting.read(cx);
            let line = fitted::describe(
                p.fitted_schedule,
                fitting.status.as_ref(),
                fitting.running(),
                fitted::local_date,
            );
            rows.push(
                setting(
                    "Schedule fitted to your reviews",
                    Some(line.into()),
                    switch("profile-fitted", p.fitted_schedule, t).on_toggle(move |on, _, cx| {
                        let _ =
                            me.update(cx, |this, cx| this.choose(|p| p.fitted_schedule = on, cx));
                    }),
                    t,
                )
                .into_any_element(),
            );
        }
        rows
    }

    /// The learner's own words about how they learn.
    fn own_words(&self, t: Theme) -> AnyElement {
        card(t)
            .child(Self::asked(
                "Anything else about how you learn",
                None,
                field(&self.notes, t).min_rows(2),
                t,
            ))
            .into_any_element()
    }
}

impl Render for ProfileForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        let you = matches!(self.part, Part::You | Part::Whole);
        let schedule = matches!(self.part, Part::Flashcards | Part::Whole);
        let mut form = div().flex().flex_col().gap(px(18.0));
        if you {
            form = form
                .child(
                    incipit_ui::section("About you", t)
                        .child(incipit_ui::rows(self.about(t, cx), t)),
                )
                .child(
                    incipit_ui::section("How do you like to learn?", t)
                        .child(incipit_ui::rows(self.ways(t, cx), t))
                        .child(footnote(
                            "Figures, the tutor and quizzes adapt to this. Most people are a mix.",
                            t,
                        )),
                );
        }
        if schedule {
            form = form.child(
                incipit_ui::section("Review", t).child(incipit_ui::rows(self.schedule(t, cx), t)),
            );
        }
        if you {
            form = form.child(incipit_ui::section("In your own words", t).child(self.own_words(t)));
        }
        let said = self.said.clone().map(|(text, bad)| {
            div()
                .text_size(px(Type::BODY))
                .text_color(if bad { t.danger } else { t.secondary })
                .child(text)
        });
        match self.submit.clone() {
            Some(submit) => form.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(
                        button("profile-save", submit, t)
                            .accent()
                            .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                    )
                    .children(said),
            ),
            None => form.children(said),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_reads_as_the_choice_whose_band_it_is_in() {
        assert_eq!(wants_at(0.0), 0);
        assert_eq!(wants_at(0.39), 0);
        assert_eq!(wants_at(0.4), 1);
        assert_eq!(wants_at(0.6), 1);
        assert_eq!(wants_at(0.7), 2);
        assert_eq!(wants_at(1.0), 2);
        for (at, (_, value)) in WANTS.iter().enumerate() {
            assert_eq!(wants_at(*value), at);
        }
    }

    #[test]
    fn the_recall_shown_is_the_nearest_offered() {
        assert_eq!(recall_at(0.9), 2);
        assert_eq!(recall_at(0.7), 0);
        assert_eq!(recall_at(0.97), 3);
        assert_eq!(recall_at(0.86), 1);
    }

    #[test]
    fn interests_are_split_at_commas_and_new_cards_are_a_whole_number_within_limits() {
        assert_eq!(
            interests_of(" chess, ,go ,  music theory "),
            ["chess", "go", "music theory"]
        );
        assert!(interests_of(" , ").is_empty());
        assert_eq!(daily_of("35"), Some(35));
        assert_eq!(daily_of(" 12.6 "), Some(13));
        assert_eq!(daily_of("0"), Some(0));
        assert_eq!(daily_of("900"), Some(200));
        assert_eq!(daily_of("-3"), Some(0));
        assert_eq!(daily_of("many"), None);
        assert_eq!(daily_of(""), None);
        assert_eq!(daily_of("NaN"), None);
    }
}
