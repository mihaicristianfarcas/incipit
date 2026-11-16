//! Made-up notes for looking at the Notes page's extras without writing them: INCIPIT_NOTES,
//! on a throwaway vault (INCIPIT_VAULT) that has no notes, adds
//!
//! - `versions` or `changes`: a note, "Waves", written in three sittings, and the Notes page
//!   (INCIPIT_PAGE=notes) opens its Versions sheet with the middle version picked, as it was
//!   (`versions`) or as what it changed (`changes`).
//!
//! - `pictures`: a note, "Light", that embeds a picture kept in `attachments/` (a made-up
//!   gradient) with a width and one without, and a figure kept on its own (a sequence), so the
//!   Notes page (INCIPIT_PAGE=notes) shows both under the note's text.
//!
//! - `quiz` or `play`: a note, "Replication", with two `quiz` blocks, one that reads (two
//!   multiple-choice questions and one answered in words) and one that does not (a question with
//!   no model answer), so the Notes page lists a quiz to start and one it cannot read; `play`
//!   also starts the first quiz in the quiz sheet. A question answered in words is checked by a
//!   model, so only the choices are played without one.
//!
//! Nothing is added to a vault that has notes. No model is called.

use incipit_vault::{Doc, Kind, Vault};

/// What INCIPIT_NOTES asks for, if anything.
pub fn asked() -> Option<String> {
    std::env::var("INCIPIT_NOTES").ok()
}

/// The note the demo is about: the page opens on it.
pub const LANDING: &str = "Waves";
/// The note of `pictures`.
pub const PICTURES: &str = "Light";
/// The note of `quiz` and `play`.
pub const QUIZ: &str = "Replication";

/// Adds the made-up notes asked for to a vault that has none.
pub fn add(vault: &Vault) {
    if vault.count(Some(Kind::Note)) > 0 {
        return;
    }
    match asked().as_deref() {
        Some("versions" | "changes") => written_in_three_sittings(vault),
        Some("pictures") => with_a_picture_and_a_figure(vault),
        Some("quiz" | "play") => with_quizzes(vault),
        _ => {}
    }
}

/// A note with a quiz that reads and one that does not.
fn with_quizzes(vault: &Vault) {
    let mut note = Doc::new(Kind::Note);
    note.set_title(QUIZ);
    note.set_body(
        "Copies of the data live on several machines.\n\n```quiz\ntitle: Replication, check \
         yourself\nquestions:\n  - q: Which setup lets any replica accept writes?\n    choices: \
         [Single-leader, Multi-leader, Leaderless]\n    answer: 3\n    why: Leaderless stores \
         send each write to several replicas directly.\n    concept: Leaderless replication\n  - \
         q: Which setup has one machine take every write?\n    choices: [Single-leader, \
         Leaderless]\n    answer: Single-leader\n  - q: In a sentence, what is replication \
         lag?\n    answer: The delay before a write on the leader shows up on a follower.\n```\n\n\
         A second quiz, with a mistake in it:\n\n```quiz\ntitle: Half written\nquestions:\n  - \
         q: What is a quorum?\n```\n",
    );
    let _ = vault.add(note);
}

/// A gradient, as the PNG a person would paste.
fn gradient() -> Vec<u8> {
    let picture = image::RgbaImage::from_fn(240, 120, |x, y| {
        image::Rgba([(x * 255 / 239) as u8, (y * 255 / 119) as u8, 160, 255])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    picture
        .write_to(&mut out, image::ImageFormat::Png)
        .expect("a picture in memory is written as a PNG");
    out.into_inner()
}

/// A note that embeds a picture twice (the second time narrower) and a figure.
fn with_a_picture_and_a_figure(vault: &Vault) {
    let Ok(place) = vault.add_picture(&gradient(), Some("Sky colours.png")) else {
        return;
    };
    let mut figure = Doc::new(Kind::Figure);
    figure.set_title("How a request travels");
    figure.set_fields(&incipit_vault::Figure {
        spec: serde_json::to_value(incipit_scene::schema::example(incipit_scene::Kind::ALL[0]))
            .ok(),
        ..incipit_vault::Figure::default()
    });
    let Ok(figure) = vault.add(figure) else {
        return;
    };
    let mut note = Doc::new(Kind::Note);
    note.set_title(PICTURES);
    note.set_body(format!(
        "A picture kept in the vault:\n\n![[{name}]]\n\nThe same, narrower:\n\n![[{name}|120]]\n\n\
         A figure kept on its own:\n\n![[figure:{id}]]\n",
        name = place.name(),
        id = figure.id()
    ));
    let _ = vault.add(note);
}

/// A note whose history has three sittings, the last of which replaced a sentence.
fn written_in_three_sittings(vault: &Vault) {
    let mut doc = Doc::new(Kind::Note);
    doc.set_title(LANDING);
    doc.set_body("A wave carries energy from one place to another.\n");
    let Ok(mut doc) = vault.add(doc) else {
        return;
    };
    let _ = vault.close_sitting(Some("New note “Waves”"));
    doc.set_body(
        "A wave carries energy from one place to another.\n\nIt does so without carrying the \
         matter it moves through.\n",
    );
    let Ok(mut doc) = vault.update(&doc) else {
        return;
    };
    let _ = vault.close_sitting(Some("Edited note “Waves”"));
    doc.set_body(
        "A wave carries energy and momentum from one place to another.\n\nIt does so without \
         carrying the matter it moves through.\n\nThe distance between two crests is the \
         wavelength.\n",
    );
    let _ = vault.update(&doc);
    let _ = vault.close_sitting(Some("Edited note “Waves”"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_made_up_note_has_a_quiz_that_reads_and_one_that_does_not() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(&folder.path().join("vault")).unwrap();
        with_quizzes(&vault);
        let note = vault
            .list(Some(Kind::Note))
            .into_iter()
            .find(|n| n.title() == QUIZ)
            .unwrap();
        let found = crate::note_quizzes::read(note.body());
        assert_eq!(found.len(), 2);
        let quiz = found[0].quiz.as_ref().unwrap();
        assert_eq!(quiz.questions.len(), 3);
        assert_eq!(
            crate::note_quizzes::size_of(quiz),
            "3 questions, some in your own words"
        );
        assert!(found[1].quiz.is_err());
    }

    #[test]
    fn the_made_up_note_embeds_a_picture_and_a_figure_that_resolve() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(&folder.path().join("vault")).unwrap();
        with_a_picture_and_a_figure(&vault);
        let note = vault
            .list(Some(Kind::Note))
            .into_iter()
            .find(|n| n.title() == PICTURES)
            .unwrap();
        let embeds = vault.embeds(note.id());
        assert_eq!(embeds.len(), 3, "{embeds:?}");
        let picture = incipit_vault::VaultPath::new("attachments/Sky colours.png").unwrap();
        assert!(vault.picture(&picture).unwrap().starts_with(&[0x89, b'P']));
        assert!(vault.unresolved().is_empty());
    }

    #[test]
    fn the_made_up_note_has_three_saved_versions_and_the_unsaved_one_is_not_among_them() {
        let folder = tempfile::tempdir().unwrap();
        let vault = Vault::create(&folder.path().join("vault")).unwrap();
        written_in_three_sittings(&vault);
        let note = vault.list(Some(Kind::Note)).remove(0);
        let versions = vault.versions_of(note.id()).unwrap();
        assert_eq!(versions.len(), 3);
        let middle = vault.doc_at(note.id(), &versions[1].commit).unwrap();
        assert!(middle.body().contains("without carrying the matter"));
        assert!(!middle.body().contains("wavelength"));
        let diff = vault
            .changes_in(&versions[1].commit, Some(note.id()))
            .unwrap();
        assert!(diff.contains("+It does so without carrying"), "{diff}");
    }
}
