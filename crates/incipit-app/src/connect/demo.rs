//! Made-up notes, concepts and suggestions for looking at the connections without a model:
//! INCIPIT_CONNECT=demo, on a throwaway vault (INCIPIT_VAULT), adds
//!
//! - three notes on distributed systems, one of which names another without linking it (so the
//!   Notes page lists a mention to link), with suggestions waiting between them, one accepted
//!   (its link is in the note) and one turned down;
//! - four concepts on attention, where Self-attention builds on Attention and Attention on
//!   Softmax, and Softmax has only a card reviewed long ago, so it is forgotten: the line of
//!   foundations shows over the results of INCIPIT_QUIZ=done, whose misses are on Self-attention
//!   and Attention. Positional encoding builds on nothing and has a suggestion of what it builds
//!   on waiting.
//!
//! Nothing is added to a vault that has notes or concepts. No model is called.

use incipit_vault::{
    rfc3339, By, Card, Concept, Connection, Doc, Kind, Meaning, Review, Standing, Vault, REVIEWS,
};

const DAY: i64 = 86_400_000;

/// What INCIPIT_CONNECT asks for, if anything.
pub fn asked() -> Option<String> {
    std::env::var("INCIPIT_CONNECT").ok()
}

/// Adds the made-up documents to a vault that has no notes or concepts.
pub fn add(vault: &Vault, now: i64) {
    if vault.count(Some(Kind::Note)) + vault.count(Some(Kind::Concept)) > 0 {
        return;
    }
    let note = |title: &str, body: &str, related: &[&str]| {
        let mut doc = Doc::new(Kind::Note);
        doc.set_title(title);
        doc.set_body(body);
        doc.set_related(related.iter().map(|r| format!("[[{r}]]")).collect());
        vault.add(doc)
    };
    let concept = |title: &str, summary: &str, builds_on: &[&str]| {
        let mut doc = Doc::new(Kind::Concept);
        doc.set_title(title);
        doc.set_fields(&Concept {
            summary: summary.into(),
            builds_on: builds_on.iter().map(|b| format!("[[{b}]]")).collect(),
            ..Concept::default()
        });
        vault.add(doc)
    };
    let quorums = note(
        "Quorums",
        "A quorum is a majority of the nodes, so two quorums always share a node.\n",
        &[],
    );
    let consensus = note(
        "Consensus",
        "Nodes agree on one value even when some fail. See also quorums, which make the vote safe.\n",
        &["Raft"],
    );
    let raft = note(
        "Raft",
        "Raft elects a leader with a majority of the votes, and the leader orders the log.\n",
        &[],
    );
    let (Ok(quorums), Ok(consensus), Ok(raft)) = (quorums, consensus, raft) else {
        return;
    };
    let _ = concept(
        "Softmax",
        "Turns scores into weights that add up to one.",
        &[],
    );
    let _ = concept(
        "Attention",
        "Each query takes a weighted mix of the values.",
        &["Softmax"],
    );
    let _ = concept(
        "Self-attention",
        "Each position of a sequence attends to every other one.",
        &["Attention"],
    );
    let positional = concept(
        "Positional encoding",
        "Tells the model where in the sequence a word is.",
        &[],
    );
    let Ok(positional) = positional else {
        return;
    };
    // Softmax was learned once, long ago, and never recalled since.
    let mut card = Doc::new(Kind::Card);
    card.set_fields(&Card {
        front: "What does softmax turn scores into?".into(),
        back: "Weights that add up to one.".into(),
        deck: "Attention".into(),
        ..Card::default()
    });
    card.set_concepts(vec!["[[Softmax]]".into()]);
    if let Ok(card) = vault.add(card) {
        let _ = vault.append_log(
            REVIEWS,
            &Review {
                card: card.id().as_str().to_owned(),
                at: rfc3339(now - 200 * DAY),
                rating: 3,
                took: None,
                variant: false,
            },
        );
    }
    let by = Some(By {
        provider: "demo".into(),
        model: "made up".into(),
    });
    let connection =
        |id: &str, from: &Doc, to: &str, why: &str, status: Standing, builds: bool| Connection {
            id: id.into(),
            kind: builds.then_some(Meaning::BuildsOn),
            from: from.path().to_string(),
            to: to.into(),
            why: why.into(),
            status,
            by: by.clone(),
            created: rfc3339(now - 2 * DAY),
            decided: (status != Standing::Suggested).then(|| rfc3339(now - DAY)),
        };
    let _ = vault.save_connections(&[
        connection(
            "cdemo1",
            &quorums,
            "Consensus",
            "Quorums are how a consensus protocol makes its vote safe.",
            Standing::Suggested,
            false,
        ),
        connection(
            "cdemo2",
            &raft,
            "Quorums",
            "Raft's leader election needs a majority, which is a quorum.",
            Standing::Suggested,
            false,
        ),
        connection(
            "cdemo3",
            &consensus,
            "Raft",
            "Raft is a worked example of reaching consensus.",
            Standing::Accepted,
            false,
        ),
        connection(
            "cdemo4",
            &raft,
            "Consensus",
            "Both are about agreement.",
            Standing::Rejected,
            false,
        ),
        connection(
            "cdemo5",
            &positional,
            "Self-attention",
            "Positions are added to what self-attention weighs, since it has no order of its own.",
            Standing::Suggested,
            true,
        ),
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::concepts::upsert::find;
    use crate::concepts::view::Concepts;

    #[test]
    fn the_demo_has_suggestions_a_mention_and_a_foundation_being_forgotten() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(folder.path()).unwrap();
        let now = crate::study::now();
        add(&vault, now);
        let list = vault.connections().unwrap();
        let waiting = list.iter().filter(|c| c.status == Standing::Suggested);
        assert_eq!(waiting.count(), 3);
        assert_eq!(list.iter().filter(|c| c.builds_on()).count(), 1);

        // Consensus names quorums without linking them.
        let quorums = vault
            .list(Some(Kind::Note))
            .into_iter()
            .find(|n| n.title() == "Quorums")
            .unwrap();
        let mentions = vault.unlinked_mentions(quorums.id());
        assert_eq!(
            mentions
                .iter()
                .map(|m| m.title.as_str())
                .collect::<Vec<_>>(),
            ["Consensus"]
        );

        // The misses of the made-up quiz are on Self-attention and Attention, which build on
        // Softmax, long forgotten.
        let concepts = Concepts::read(&vault, now);
        let on: Vec<_> = ["Self-attention", "Attention"]
            .iter()
            .map(|name| find(&vault, name).unwrap().id().clone())
            .collect();
        let fading = concepts.fading(&on);
        assert_eq!(
            fading.iter().map(|b| b.title.as_str()).collect::<Vec<_>>(),
            ["Softmax"]
        );

        // A vault that has notes is left alone.
        add(&vault, now);
        assert_eq!(vault.connections().unwrap(), list);
    }
}
