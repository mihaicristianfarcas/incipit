//! The form that asks who the learner is and how they learn, the earlier app's `ProfileForm`:
//! their name and role, what they want to learn and why, their background and level, interests,
//! how much they want pictures, words and things to touch, the new cards a day and the recall
//! they aim for, and anything else in their own words. It is kept as `profile.md` in the open
//! vault (`incipit_vault::Profile`). The first run and Settings show the same form; saving it
//! also marks the first run as done, as the earlier app's did.
//!
//! The earlier app's three sliders are choices here (a little, some, a lot; 80, 85, 90 or 95 per
//! cent), since the app has no slider; a value kept in the file that none of them is stays as it
//! is until the learner picks another.

use gpui::{
    div, prelude::*, px, AnyElement, Context, Entity, EventEmitter, FontWeight, SharedString,
    Subscription, Window,
};
use incipit_ui::{
    button, card, field, footnote, segmented, select, setting, TextField, Theme, Type,
};
use incipit_vault::{Level, Profile, Role};

use crate::workspace::{Happened, Workspace};

/// The profile was saved.
pub struct Saved;

impl EventEmitter<Saved> for ProfileForm {}

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
    /// What was read from the vault, and what the choices change.
    profile: Profile,
    name: Entity<TextField>,
    goals: Entity<TextField>,
    background: Entity<TextField>,
    interests: Entity<TextField>,
    daily: Entity<TextField>,
    notes: Entity<TextField>,
    /// What the button that saves says.
    submit: SharedString,
    /// How the last save went, in words.
    said: Option<(SharedString, bool)>,
    _subscriptions: Vec<Subscription>,
}

fn text_field(
    placeholder: &'static str,
    multiline: bool,
    cx: &mut Context<ProfileForm>,
) -> Entity<TextField> {
    cx.new(|cx| {
        let field = TextField::new(cx).placeholder(placeholder);
        if multiline {
            field.multiline()
        } else {
            field
        }
    })
}

impl ProfileForm {
    pub fn new(
        workspace: Entity<Workspace>,
        submit: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions =
            vec![
                cx.subscribe(&workspace, |this: &mut Self, _, happened: &Happened, cx| {
                    match happened {
                        Happened::Opened => this.load(cx),
                        Happened::Changed(event) if event.path.as_str() == "profile.md" => {
                            this.load(cx)
                        }
                        Happened::Changed(_) => {}
                    }
                }),
            ];
        let mut form = ProfileForm {
            workspace,
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
            submit: submit.into(),
            said: None,
            _subscriptions: subscriptions,
        };
        form.load(cx);
        form
    }

    fn vault(&self, cx: &Context<Self>) -> Option<incipit_vault::Vault> {
        self.workspace.read(cx).vault.clone()
    }

    /// Read the profile again from the open vault into the form.
    fn load(&mut self, cx: &mut Context<Self>) {
        self.profile = self
            .vault(cx)
            .map(|vault| vault.profile())
            .unwrap_or_default();
        let p = self.profile.clone();
        let put = |field: &Entity<TextField>, text: String, cx: &mut Context<Self>| {
            if field.read(cx).text() != text {
                field.update(cx, |f, cx| f.set_text(text, cx));
            }
        };
        put(&self.name, p.name, cx);
        put(&self.goals, p.goals, cx);
        put(&self.background, p.background, cx);
        put(&self.interests, p.interests.join(", "), cx);
        put(&self.daily, p.daily_new.to_string(), cx);
        put(&self.notes, p.notes, cx);
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

    /// Save the profile, marking the first run done.
    pub fn save(&mut self, cx: &mut Context<Self>) {
        let Some(vault) = self.vault(cx) else {
            self.said = Some(("Open a vault first.".into(), true));
            cx.notify();
            return;
        };
        let mut profile = self.made(cx);
        profile.onboarded = true;
        match vault.save_profile(&profile) {
            Ok(saved) => {
                self.profile = saved;
                self.said = Some(("Profile saved".into(), false));
                self.load(cx);
                cx.emit(Saved);
            }
            Err(e) => {
                self.said = Some((format!("The profile could not be saved: {e}").into(), true))
            }
        }
        cx.notify();
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

    /// A choice or a typed value changed the profile the form holds: no save yet.
    fn choose(&mut self, change: impl FnOnce(&mut Profile), cx: &mut Context<Self>) {
        change(&mut self.profile);
        self.said = None;
        cx.notify();
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
}

impl Render for ProfileForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        let me = cx.entity().downgrade();
        let p = &self.profile;
        let role = Role::ALL.iter().position(|r| *r == p.role).unwrap_or(3);
        let level = Level::ALL.iter().position(|l| *l == p.level).unwrap_or(1);
        let styles = [p.styles.visual, p.styles.reader, p.styles.doer];

        let about = vec![
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
                    .on_select({
                        let me = me.clone();
                        move |i, _, cx| {
                            let _ = me.update(cx, |this, cx| {
                                this.choose(|p| p.level = Level::ALL[i], cx)
                            });
                        }
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
        ];

        let ways: Vec<AnyElement> = STYLES
            .iter()
            .enumerate()
            .map(|(which, (name, help))| {
                let me = me.clone();
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
            .collect();

        let review = vec![
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

        let more = vec![Self::asked(
            "Anything else about how you learn",
            None,
            field(&self.notes, t).min_rows(2),
            t,
        )];

        let save = button("profile-save", self.submit.clone(), t)
            .accent()
            .on_click({
                let me = me.clone();
                move |_, _, cx| {
                    let _ = me.update(cx, |this, cx| this.save(cx));
                }
            });
        let said = self.said.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(18.0))
            .child(incipit_ui::section("About you", t).child(incipit_ui::rows(about, t)))
            .child(
                incipit_ui::section("How do you like to learn?", t)
                    .child(incipit_ui::rows(ways, t))
                    .child(footnote(
                        "Figures, the tutor and quizzes adapt to this. Most people are a mix.",
                        t,
                    )),
            )
            .child(incipit_ui::section("Review", t).child(incipit_ui::rows(review, t)))
            .child(incipit_ui::section("In your own words", t).child(card(t).children(more)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(save)
                    .children(said.map(|(text, bad)| {
                        div()
                            .text_size(px(Type::BODY))
                            .text_color(if bad { t.danger } else { t.secondary })
                            .child(text)
                    })),
            )
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
