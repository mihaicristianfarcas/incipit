//! The learner's profile: `profile.md` at the vault's root. Who they are and how they like to
//! learn are its properties, in the earlier app's names (`name`, `role`, `goals`, `background`,
//! `level`, `styles`, `interests`, `dailyNew`, `retention`, `onboarded`), and one of Incipit's
//! own (`fittedSchedule`, whether the flashcards' schedule is fitted to their reviews); their own
//! words about how they learn are its body. It is not a document (it is not listed among the
//! notes, and has no id): the vault reads and writes it here, and the app's model prompts and
//! review settings read it from here.
//!
//! Reading is forgiving, as for every file in a vault: a missing file is the default profile, a
//! value of the wrong type or outside its limits takes the default or the nearest limit, and
//! properties this module does not know are kept when the profile is saved.

use crate::error::Result;
use crate::events::Change;
use crate::frontmatter;
use crate::fsio::{self, stat_fingerprint};
use crate::path::VaultPath;
use crate::store::{here, Vault};
use crate::value::{Properties, Value};

/// Where the profile is kept.
pub const PROFILE: &str = "profile.md";

/// What the body holds until the learner writes something: a hint, which is not their words.
pub const HINT: &str =
    "_Anything about how you learn best: write it here, and the tutor will read it._";

/// The least and the most of the target recall: a lower one lets cards be forgotten, a higher
/// one brings every card back too often. The earlier app's.
pub const RETENTION: (f64, f64) = (0.7, 0.97);

/// The most new cards a day.
pub const MOST_NEW: u32 = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Student,
    Professor,
    Professional,
    Curious,
}

impl Role {
    pub const ALL: [Role; 4] = [
        Role::Student,
        Role::Professor,
        Role::Professional,
        Role::Curious,
    ];

    /// The word kept in the file.
    pub fn key(self) -> &'static str {
        match self {
            Role::Student => "student",
            Role::Professor => "professor",
            Role::Professional => "professional",
            Role::Curious => "curious",
        }
    }

    fn from_key(key: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|r| r.key() == key)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Beginner,
    Intermediate,
    Advanced,
    Expert,
}

impl Level {
    pub const ALL: [Level; 4] = [
        Level::Beginner,
        Level::Intermediate,
        Level::Advanced,
        Level::Expert,
    ];

    /// The word kept in the file.
    pub fn key(self) -> &'static str {
        match self {
            Level::Beginner => "beginner",
            Level::Intermediate => "intermediate",
            Level::Advanced => "advanced",
            Level::Expert => "expert",
        }
    }

    fn from_key(key: &str) -> Option<Level> {
        Level::ALL.into_iter().find(|l| l.key() == key)
    }
}

/// How much the learner wants pictures, words, and things to touch, each 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Styles {
    pub visual: f64,
    pub reader: f64,
    pub doer: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub name: String,
    pub role: Role,
    /// What they want to learn and why.
    pub goals: String,
    /// Their background and expertise.
    pub background: String,
    pub level: Level,
    pub styles: Styles,
    pub interests: Vec<String>,
    /// New cards a day, 0 to `MOST_NEW`.
    pub daily_new: u32,
    /// The recall a card is scheduled to fall to when it comes due, within `RETENTION`.
    pub retention: f64,
    /// Whether the flashcards' schedule is fitted to the learner's own reviews once there are
    /// enough of them (true), or always the standard one.
    pub fitted_schedule: bool,
    /// Their own words, the file's body. Empty when they wrote none.
    pub notes: String,
    /// Whether the first run is done.
    pub onboarded: bool,
}

impl Default for Profile {
    fn default() -> Profile {
        Profile {
            name: String::new(),
            role: Role::Curious,
            goals: String::new(),
            background: String::new(),
            level: Level::Intermediate,
            styles: Styles {
                visual: 0.6,
                reader: 0.5,
                doer: 0.6,
            },
            interests: Vec::new(),
            daily_new: 20,
            retention: 0.9,
            fitted_schedule: true,
            notes: String::new(),
            onboarded: false,
        }
    }
}

fn clamp_style(x: f64) -> f64 {
    if x.is_finite() {
        x.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

impl Profile {
    /// The profile a file's text holds: anything missing or wrong is the default.
    pub fn parse(text: &str) -> Profile {
        let split = frontmatter::split(text);
        let props = split
            .front
            .and_then(|front| frontmatter::parse(front).ok())
            .unwrap_or_default();
        let default = Profile::default();
        let styles = props.get("styles").and_then(Value::as_map);
        let style = |key: &str, fallback: f64| {
            styles
                .and_then(|s| s.get(key))
                .and_then(Value::as_f64)
                .map_or(fallback, clamp_style)
        };
        let notes = split.body.trim();
        Profile {
            name: props.text("name").unwrap_or_default().trim().to_owned(),
            role: props
                .text("role")
                .and_then(|r| Role::from_key(&r))
                .unwrap_or(default.role),
            goals: props.text("goals").unwrap_or_default(),
            background: props.text("background").unwrap_or_default(),
            level: props
                .text("level")
                .and_then(|l| Level::from_key(&l))
                .unwrap_or(default.level),
            styles: Styles {
                visual: style("visual", default.styles.visual),
                reader: style("reader", default.styles.reader),
                doer: style("doer", default.styles.doer),
            },
            interests: props
                .texts("interests")
                .into_iter()
                .map(|i| i.trim().to_owned())
                .filter(|i| !i.is_empty())
                .collect(),
            daily_new: props
                .get("dailyNew")
                .and_then(Value::as_f64)
                .filter(|n| n.is_finite())
                .map_or(default.daily_new, clamp_daily),
            retention: props
                .get("retention")
                .and_then(Value::as_f64)
                .filter(|r| r.is_finite())
                .map_or(default.retention, clamp_retention),
            fitted_schedule: props
                .get("fittedSchedule")
                .and_then(Value::as_bool)
                .unwrap_or(default.fitted_schedule),
            notes: if notes == HINT {
                String::new()
            } else {
                notes.to_owned()
            },
            onboarded: props
                .get("onboarded")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }
    }

    /// The profile with its values brought within their limits.
    pub fn limited(mut self) -> Profile {
        self.retention = if self.retention.is_finite() {
            clamp_retention(self.retention)
        } else {
            Profile::default().retention
        };
        self.daily_new = self.daily_new.min(MOST_NEW);
        for style in [
            &mut self.styles.visual,
            &mut self.styles.reader,
            &mut self.styles.doer,
        ] {
            *style = clamp_style(*style);
        }
        self.name = self.name.trim().to_owned();
        self.interests = self
            .interests
            .iter()
            .map(|i| i.trim().to_owned())
            .filter(|i| !i.is_empty())
            .collect();
        self
    }

    /// The file's text for this profile, keeping the properties of `old` this module does not
    /// know (a person's own).
    fn to_text(&self, old: Option<&str>) -> String {
        let mut props = old
            .and_then(|text| frontmatter::parse(frontmatter::split(text).front?).ok())
            .unwrap_or_default();
        props.set("name", self.name.as_str());
        props.set("role", self.role.key());
        props.set("goals", self.goals.as_str());
        props.set("background", self.background.as_str());
        props.set("level", self.level.key());
        let mut styles = Properties::new();
        styles.set("visual", self.styles.visual);
        styles.set("reader", self.styles.reader);
        styles.set("doer", self.styles.doer);
        props.set("styles", Value::Map(styles));
        props.set("interests", self.interests.clone());
        props.set("dailyNew", i64::from(self.daily_new));
        props.set("retention", self.retention);
        props.set("fittedSchedule", self.fitted_schedule);
        props.set("onboarded", self.onboarded);
        let body = if self.notes.trim().is_empty() {
            HINT
        } else {
            self.notes.trim()
        };
        frontmatter::compose(&frontmatter::write(&props), body)
    }
}

fn clamp_retention(x: f64) -> f64 {
    x.clamp(RETENTION.0, RETENTION.1)
}

fn clamp_daily(x: f64) -> u32 {
    x.round().clamp(0.0, f64::from(MOST_NEW)) as u32
}

impl Vault {
    /// The learner's profile: the default one until it is saved.
    pub fn profile(&self) -> Profile {
        match std::fs::read(self.root().join(PROFILE)) {
            Ok(bytes) => Profile::parse(&String::from_utf8_lossy(&bytes)),
            Err(_) => Profile::default(),
        }
    }

    /// Saves the profile, within its limits, and announces the file. Returns it as saved.
    pub fn save_profile(&self, profile: &Profile) -> Result<Profile> {
        let profile = profile.clone().limited();
        let path = VaultPath::new(PROFILE)?;
        let inner = &self.inner;
        let mut state = inner.lock();
        let full = inner.locate(&path)?;
        let old = std::fs::read_to_string(&full).ok();
        let existed = old.is_some();
        let text = profile.to_text(old.as_deref());
        fsio::write_atomic(&full, text.as_bytes())?;
        if let Some(print) = stat_fingerprint(&full) {
            state.files.insert(path.clone(), print);
        }
        let change = if existed {
            Change::Changed
        } else {
            Change::Created
        };
        inner.announce(vec![here(&path, change, None)]);
        Ok(profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_file_is_the_default_profile() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        let profile = vault.profile();
        assert_eq!(profile, Profile::default());
        assert_eq!(profile.daily_new, 20);
        assert_eq!(profile.retention, 0.9);
        assert!(!profile.onboarded);
        assert!(profile.fitted_schedule);
        assert_eq!(profile.role, Role::Curious);
        assert_eq!(profile.level, Level::Intermediate);
        assert_eq!(
            (
                profile.styles.visual,
                profile.styles.reader,
                profile.styles.doer
            ),
            (0.6, 0.5, 0.6)
        );
    }

    #[test]
    fn a_profile_is_kept_and_read_back_and_the_file_is_not_a_note() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        let mine = Profile {
            name: "Ada".into(),
            role: Role::Professor,
            goals: "Teach topology".into(),
            background: "Mathematician".into(),
            level: Level::Expert,
            styles: Styles {
                visual: 0.2,
                reader: 0.9,
                doer: 0.4,
            },
            interests: vec!["knots".into(), "music".into()],
            daily_new: 35,
            retention: 0.85,
            fitted_schedule: false,
            notes: "I like proofs first.".into(),
            onboarded: true,
        };
        assert_eq!(vault.save_profile(&mine).unwrap(), mine);
        assert_eq!(vault.profile(), mine);
        let text = std::fs::read_to_string(dir.path().join(PROFILE)).unwrap();
        assert!(text.contains("dailyNew: 35"), "{text}");
        assert!(text.ends_with("I like proofs first.\n"), "{text}");
        assert!(vault.list(Some(crate::Kind::Note)).is_empty());
        let reopened = Vault::open(dir.path()).unwrap();
        assert_eq!(reopened.profile(), mine);
        assert!(reopened.list(None).is_empty());
    }

    #[test]
    fn values_are_held_within_their_limits() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        let mut p = Profile {
            retention: 0.5,
            daily_new: 900,
            ..Profile::default()
        };
        p.styles.visual = 3.0;
        let saved = vault.save_profile(&p).unwrap();
        assert_eq!(
            (saved.retention, saved.daily_new, saved.styles.visual),
            (0.7, 200, 1.0)
        );
        p.retention = 0.99;
        p.daily_new = 0;
        let saved = vault.save_profile(&p).unwrap();
        assert_eq!((saved.retention, saved.daily_new), (0.97, 0));
        assert_eq!(vault.profile().daily_new, 0);
        p.retention = f64::NAN;
        assert_eq!(vault.save_profile(&p).unwrap().retention, 0.9);
    }

    #[test]
    fn a_hand_written_file_is_read_forgivingly() {
        let text = "---\nname: 12\nrole: wizard\nlevel: Advanced\nstyles:\n  visual: 0.9\n  doer: lots\ninterests: chess\ndailyNew: 500\nretention: 0.2\nonboarded: yes please\n---\n_Anything about how you learn best: write it here, and the tutor will read it._\n";
        let p = Profile::parse(text);
        assert_eq!(p.name, "12");
        assert_eq!(p.role, Role::Curious);
        assert_eq!(p.level, Level::Intermediate);
        assert_eq!(p.styles.visual, 0.9);
        assert_eq!((p.styles.reader, p.styles.doer), (0.5, 0.6));
        assert_eq!(p.interests, ["chess"]);
        assert_eq!((p.daily_new, p.retention), (200, 0.7));
        assert!(!p.onboarded);
        assert_eq!(p.notes, "", "the hint is not the learner's words");
        assert_eq!(Profile::parse("---\n: : [\n---\nhello"), {
            Profile {
                notes: "hello".into(),
                ..Profile::default()
            }
        });
        assert_eq!(Profile::parse("").notes, "");
    }

    #[test]
    fn saving_keeps_properties_the_learner_added() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        std::fs::write(
            dir.path().join(PROFILE),
            "---\nname: Ada\nmood: curious\n---\nhello\n",
        )
        .unwrap();
        let mut p = vault.profile();
        p.goals = "Learn Rust".into();
        vault.save_profile(&p).unwrap();
        let text = std::fs::read_to_string(dir.path().join(PROFILE)).unwrap();
        assert!(text.contains("mood: curious"), "{text}");
        assert!(text.contains("goals: Learn Rust"), "{text}");
    }

    #[test]
    fn saving_is_announced() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        let events = vault.subscribe();
        vault.save_profile(&Profile::default()).unwrap();
        let event = events.recv().unwrap();
        assert_eq!(event.path.as_str(), PROFILE);
        assert_eq!(event.change, Change::Created);
        vault.save_profile(&Profile::default()).unwrap();
        assert_eq!(events.recv().unwrap().change, Change::Changed);
    }
}
