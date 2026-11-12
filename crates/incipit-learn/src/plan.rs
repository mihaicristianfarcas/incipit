//! What Today suggests doing next, with the earlier app's conditions and words (`today()`):
//!
//! - **Explain** the concept least known, once it is below 0.65 ("Your recall of it is
//!   slipping"). Of equal ones the first listed is taken. A concept not practised is not slipping.
//! - **Quiz** on the chapter of the latest reading sitting that ended more than two days ago and
//!   knew its chapter ("A few days later is the right moment to test what stayed").
//! - **Mixed review** once there are six concepts or more.
//!
//! Reviewing the cards waiting and carrying on with the book read last are the page's own blocks,
//! not suggestions in its list. Nothing here asks a clock or a vault: what was found goes in.

use crate::DAY_MS;

/// A concept is slipping from below this recall.
pub const SLIPPING: f64 = 0.65;
/// A chapter is quizzed on once its reading is more than this long ago.
pub const QUIZ_AFTER_MS: i64 = 2 * DAY_MS;
/// A mixed review is offered from this many concepts.
pub const MIXED_FROM: usize = 6;

/// A reading sitting in the activity log.
#[derive(Clone, Debug, PartialEq)]
pub struct Sitting {
    /// When it ended (ms since the epoch).
    pub at: i64,
    pub book: Option<String>,
    pub chapter: Option<String>,
    /// The chapter's number in the book's contents, as the reader reported it.
    pub chapter_id: Option<String>,
}

/// The facts the suggestions come from.
#[derive(Clone, Debug)]
pub struct Facts<'a, K> {
    /// Every concept, in the vault's order, with its id, title and how well it is known.
    pub concepts: &'a [(K, String, Option<f64>)],
    /// The reading sittings, oldest first.
    pub sittings: &'a [Sitting],
    pub now: i64,
}

/// One suggestion.
#[derive(Clone, Debug, PartialEq)]
pub enum Suggestion<K> {
    /// Explain a concept back.
    Explain { concept: K, title: String },
    /// Quiz on a chapter of a book.
    Quiz {
        book: String,
        chapter_id: String,
        chapter: String,
    },
    /// A quiz across the concepts.
    Mixed,
}

impl<K> Suggestion<K> {
    /// What it says.
    pub fn title(&self) -> String {
        match self {
            Suggestion::Explain { title, .. } => format!("Explain “{title}” in your own words"),
            Suggestion::Quiz { chapter, .. } => format!("Quiz yourself on {chapter}"),
            Suggestion::Mixed => "Mixed review quiz".to_owned(),
        }
    }

    /// Why.
    pub fn detail(&self) -> &'static str {
        match self {
            Suggestion::Explain { .. } => {
                "Your recall of it is slipping. Teaching it back shows you exactly where the gaps are."
            }
            Suggestion::Quiz { .. } => "A few days later is the right moment to test what stayed.",
            Suggestion::Mixed => {
                "Interleaving topics feels harder and works better: it teaches you to tell ideas apart."
            }
        }
    }
}

/// The suggestions, in the order the earlier app lists them: explain, quiz, mixed.
pub fn suggestions<K: Clone>(facts: &Facts<K>) -> Vec<Suggestion<K>> {
    let mut out = Vec::new();
    let mut slipping: Option<&(K, String, Option<f64>)> = None;
    for item in facts.concepts {
        let Some(m) = item.2.filter(|m| *m < SLIPPING) else {
            continue;
        };
        if slipping.is_none_or(|best| m < best.2.unwrap_or(f64::INFINITY)) {
            slipping = Some(item);
        }
    }
    if let Some((concept, title, _)) = slipping {
        out.push(Suggestion::Explain {
            concept: concept.clone(),
            title: title.clone(),
        });
    }
    let read = facts
        .sittings
        .iter()
        .rev()
        .find(|s| s.at < facts.now - QUIZ_AFTER_MS && s.chapter_id.is_some());
    if let Some(Sitting {
        book: Some(book),
        chapter,
        chapter_id: Some(chapter_id),
        ..
    }) = read
    {
        out.push(Suggestion::Quiz {
            book: book.clone(),
            chapter_id: chapter_id.clone(),
            chapter: chapter
                .clone()
                .filter(|c| !c.is_empty())
                .unwrap_or_else(|| "a chapter you read".to_owned()),
        });
    }
    if facts.concepts.len() >= MIXED_FROM {
        out.push(Suggestion::Mixed);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;

    fn concept(n: u32, m: Option<f64>) -> (u32, String, Option<f64>) {
        (n, format!("Idea {n}"), m)
    }

    fn sitting(days_ago: f64, chapter: Option<&str>, id: Option<&str>) -> Sitting {
        Sitting {
            at: NOW - (days_ago * DAY_MS as f64) as i64,
            book: Some("b".into()),
            chapter: chapter.map(str::to_owned),
            chapter_id: id.map(str::to_owned),
        }
    }

    fn of(concepts: &[(u32, String, Option<f64>)], sittings: &[Sitting]) -> Vec<Suggestion<u32>> {
        suggestions(&Facts {
            concepts,
            sittings,
            now: NOW,
        })
    }

    #[test]
    fn nothing_is_suggested_with_nothing_to_go_on() {
        assert!(of(&[], &[]).is_empty());
    }

    #[test]
    fn the_least_known_concept_below_the_line_is_to_explain() {
        let concepts = [
            concept(1, Some(0.9)),
            concept(2, Some(0.6)),
            concept(3, Some(0.3)),
            concept(4, None),
            concept(5, Some(0.3)),
        ];
        let found = of(&concepts, &[]);
        assert_eq!(
            found,
            [Suggestion::Explain {
                concept: 3,
                title: "Idea 3".into()
            }]
        );
        assert_eq!(found[0].title(), "Explain “Idea 3” in your own words");
    }

    #[test]
    fn a_concept_at_the_line_or_not_practised_is_not_slipping() {
        assert!(of(&[concept(1, Some(0.65)), concept(2, None)], &[]).is_empty());
        assert_eq!(of(&[concept(1, Some(0.0))], &[]).len(), 1);
    }

    #[test]
    fn a_chapter_read_more_than_two_days_ago_is_to_be_quizzed() {
        let found = of(&[], &[sitting(3.0, Some("Entropy"), Some("4"))]);
        assert_eq!(
            found,
            [Suggestion::Quiz {
                book: "b".into(),
                chapter_id: "4".into(),
                chapter: "Entropy".into()
            }]
        );
        assert_eq!(found[0].title(), "Quiz yourself on Entropy");
        // Two days ago exactly is not yet "more than"; a sitting without a chapter is no cue.
        assert!(of(&[], &[sitting(2.0, Some("Entropy"), Some("4"))]).is_empty());
        assert!(of(&[], &[sitting(5.0, Some("Entropy"), None)]).is_empty());
    }

    #[test]
    fn the_latest_old_sitting_is_the_one_quizzed() {
        let sittings = [
            sitting(9.0, Some("Old"), Some("1")),
            sitting(4.0, Some("Newer"), Some("2")),
            sitting(0.5, Some("Today"), Some("3")),
        ];
        let found = of(&[], &sittings);
        assert_eq!(found[0].title(), "Quiz yourself on Newer");
    }

    #[test]
    fn an_unnamed_chapter_is_still_quizzed() {
        let found = of(&[], &[sitting(3.0, Some(""), Some("0"))]);
        assert_eq!(found[0].title(), "Quiz yourself on a chapter you read");
    }

    #[test]
    fn a_mixed_review_needs_six_concepts() {
        let five: Vec<_> = (0..5).map(|n| concept(n, None)).collect();
        assert!(of(&five, &[]).is_empty());
        let six: Vec<_> = (0..6).map(|n| concept(n, None)).collect();
        assert_eq!(of(&six, &[]), [Suggestion::Mixed]);
    }

    #[test]
    fn they_come_in_the_earlier_apps_order() {
        let concepts: Vec<_> = (0..6).map(|n| concept(n, Some(0.2))).collect();
        let found = of(&concepts, &[sitting(3.0, Some("C"), Some("1"))]);
        assert!(matches!(
            found.as_slice(),
            [
                Suggestion::Explain { .. },
                Suggestion::Quiz { .. },
                Suggestion::Mixed
            ]
        ));
    }
}
