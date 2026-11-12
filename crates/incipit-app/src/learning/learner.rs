//! A paragraph about the learner for the requests that adapt to them, as the earlier app's
//! `learnerSummary` wrote it: their name, role and level, background, goals and interests, how
//! much they want pictures, words and things to touch, the concepts they are solid on (a mastery
//! of at least 0.8, the first 12 in the vault's order), the ones they are shaky on (below 0.6,
//! least known first, up to 10), and the first 600 characters of their own words.
//!
//! It is what the learner wrote about themselves and the titles of their concepts: material
//! about them, sent apart from the instructions (`LABEL`), never part of them. The profile is
//! `incipit_vault::Profile`; how well a concept is known is `concepts::view`.

use incipit_llm::Request;
use incipit_vault::{Kind, Profile, Role, Vault};

use crate::concepts::view::Concepts;

/// The label of the material that holds the paragraph.
pub const LABEL: &str = "the learner";

/// From this mastery a concept counts as solid, and below `SHAKY` as shaky.
const SOLID: f64 = 0.8;
const SHAKY: f64 = 0.6;

/// The most concepts named as solid and as shaky.
const MOST_SOLID: usize = 12;
const MOST_SHAKY: usize = 10;

/// How much of the learner's own words go in.
const OWN_WORDS: usize = 600;

/// A concept and how well the learner knows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Known {
    pub title: String,
    pub mastery: f64,
}

fn style(value: f64) -> &'static str {
    if value >= 0.7 {
        "a lot"
    } else if value >= 0.4 {
        "some"
    } else {
        "little"
    }
}

/// The paragraph for a profile and the concepts that have a mastery, in the vault's order.
pub fn summary(profile: &Profile, known: &[Known]) -> String {
    let strong: Vec<&str> = known
        .iter()
        .filter(|k| k.mastery >= SOLID)
        .map(|k| k.title.as_str())
        .take(MOST_SOLID)
        .collect();
    let mut shaky: Vec<&Known> = known.iter().filter(|k| k.mastery < SHAKY).collect();
    shaky.sort_by(|a, b| a.mastery.total_cmp(&b.mastery));
    let weak: Vec<&str> = shaky
        .iter()
        .map(|k| k.title.as_str())
        .take(MOST_SHAKY)
        .collect();
    let who = if profile.name.is_empty() {
        "The learner"
    } else {
        profile.name.as_str()
    };
    let role = match profile.role {
        Role::Curious => "self-directed learner",
        other => other.key(),
    };
    let s = profile.styles;
    let mut lines = vec![format!(
        "{who} is a {role}; level: {}.",
        profile.level.key()
    )];
    if !profile.background.is_empty() {
        lines.push(format!("Background: {}", profile.background));
    }
    if !profile.goals.is_empty() {
        lines.push(format!("Goals: {}", profile.goals));
    }
    if !profile.interests.is_empty() {
        lines.push(format!("Interests: {}.", profile.interests.join(", ")));
    }
    lines.push(format!(
        "Wants pictures: {} (visual {:.1}); wants words: {} (reader {:.1}); wants things to touch and try: {} (doer {:.1}).",
        style(s.visual),
        s.visual,
        style(s.reader),
        s.reader,
        style(s.doer),
        s.doer,
    ));
    if !strong.is_empty() {
        lines.push(format!("Already solid on: {}.", strong.join(", ")));
    }
    if !weak.is_empty() {
        lines.push(format!("Shaky on: {}.", weak.join(", ")));
    }
    if !profile.notes.is_empty() {
        let words: String = profile.notes.chars().take(OWN_WORDS).collect();
        lines.push(format!("In their own words: {words}"));
    }
    lines.join("\n")
}

/// The paragraph for the learner of `vault`, with what they know as of `now`.
pub fn of(vault: &Vault, now: i64) -> String {
    let concepts = Concepts::read(vault, now);
    let known: Vec<Known> = vault
        .list(Some(Kind::Concept))
        .iter()
        .filter_map(|doc| {
            Some(Known {
                title: doc.title(),
                mastery: concepts.mastery(doc.id())?,
            })
        })
        .collect();
    summary(&vault.profile(), &known)
}

/// `request` with the paragraph as material, apart from its instructions. An empty paragraph
/// (no vault) adds nothing.
pub fn add_to(request: Request, learner: &str) -> Request {
    if learner.trim().is_empty() {
        request
    } else {
        request.with_material(LABEL, learner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use incipit_vault::{Level, Styles};

    fn known(title: &str, mastery: f64) -> Known {
        Known {
            title: title.into(),
            mastery,
        }
    }

    #[test]
    fn a_new_learner_is_described_by_the_defaults_alone() {
        assert_eq!(
            summary(&Profile::default(), &[]),
            "The learner is a self-directed learner; level: intermediate.\n\
             Wants pictures: some (visual 0.6); wants words: some (reader 0.5); \
             wants things to touch and try: some (doer 0.6)."
        );
    }

    #[test]
    fn everything_the_learner_said_is_in_it() {
        let profile = Profile {
            name: "Ada".into(),
            role: Role::Professor,
            goals: "Teach topology".into(),
            background: "Mathematician".into(),
            level: Level::Expert,
            styles: Styles {
                visual: 0.9,
                reader: 0.4,
                doer: 0.1,
            },
            interests: vec!["knots".into(), "music".into()],
            notes: "Proofs first.".into(),
            ..Profile::default()
        };
        assert_eq!(
            summary(&profile, &[]),
            "Ada is a professor; level: expert.\n\
             Background: Mathematician\n\
             Goals: Teach topology\n\
             Interests: knots, music.\n\
             Wants pictures: a lot (visual 0.9); wants words: some (reader 0.4); \
             wants things to touch and try: little (doer 0.1).\n\
             In their own words: Proofs first."
        );
    }

    #[test]
    fn solid_concepts_keep_the_vaults_order_and_shaky_ones_least_known_first() {
        let list = [
            known("Limits", 0.95),
            known("Slopes", 0.3),
            known("Sums", 0.7),
            known("Areas", 0.8),
            known("Chains", 0.1),
            known("Exact", 0.6),
        ];
        let text = summary(&Profile::default(), &list);
        assert!(
            text.contains("\nAlready solid on: Limits, Areas."),
            "{text}"
        );
        assert!(text.contains("\nShaky on: Chains, Slopes."), "{text}");
        assert!(!text.contains("Sums") && !text.contains("Exact"), "{text}");
    }

    #[test]
    fn at_most_twelve_solid_and_ten_shaky_concepts_are_named() {
        let mut list: Vec<Known> = (0..15).map(|n| known(&format!("S{n}"), 0.9)).collect();
        list.extend((0..15).map(|n| known(&format!("W{n}"), 0.01 * f64::from(n))));
        let text = summary(&Profile::default(), &list);
        let solid = text
            .lines()
            .find(|l| l.starts_with("Already solid"))
            .unwrap();
        assert_eq!(solid.matches("S").count(), 12);
        let shaky = text.lines().find(|l| l.starts_with("Shaky")).unwrap();
        assert_eq!(shaky.matches('W').count(), 10);
        assert!(shaky.starts_with("Shaky on: W0, W1, W2"), "{shaky}");
    }

    #[test]
    fn only_the_first_six_hundred_characters_of_their_own_words_go_in() {
        let profile = Profile {
            notes: "é".repeat(900),
            ..Profile::default()
        };
        let text = summary(&profile, &[]);
        let own = text.lines().last().unwrap();
        assert_eq!(own, format!("In their own words: {}", "é".repeat(600)));
    }

    #[test]
    fn the_paragraph_is_material_apart_from_the_instructions() {
        let request = add_to(Request::new("Do the job."), "Ada is a student.");
        assert!(!request.system().contains("Ada is a student."));
        let found = request.material.iter().any(|m| m.label == LABEL);
        assert!(found);
        assert_eq!(add_to(Request::new("x"), "  ").material.len(), 0);
    }

    #[test]
    fn it_reads_the_vault_it_is_given() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        vault
            .save_profile(&Profile {
                name: "Ada".into(),
                onboarded: true,
                ..Profile::default()
            })
            .unwrap();
        crate::concepts::testing::concept(&vault, "Limits", &[], &[]);
        let text = of(&vault, 1_800_000_000_000);
        assert!(
            text.starts_with("Ada is a self-directed learner;"),
            "{text}"
        );
        assert!(
            !text.contains("Limits"),
            "a concept with no evidence is not named: {text}"
        );
    }
}
