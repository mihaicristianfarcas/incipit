use incipit_learn::Fixed;

use super::*;

const UTC: Fixed = Fixed(0);
const DAY: i64 = 86_400_000;
/// Noon, UTC, on a day in 2024.
const START: i64 = 19_800 * DAY + 12 * 3_600_000;

fn at(time: i64) -> String {
    chrono::DateTime::from_timestamp_millis(time)
        .unwrap()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Reviews of `cards` cards, each on the days `days` after `START` (shifted by a minute a card), in
/// a fixed pattern of ratings with a lapse now and then. Each day after the first is one review
/// a fit can learn from.
fn review(vault: &Vault, cards: std::ops::Range<usize>, days: &[i64]) {
    for card in cards {
        for (i, day) in days.iter().enumerate() {
            let rating = match (card + i) % 9 {
                0 => 1,
                1 => 2,
                8 => 4,
                _ => 3,
            };
            let line = Review {
                card: format!("card-{card}"),
                at: at(START + day * DAY + card as i64 * 60_000),
                rating,
                took: None,
                variant: false,
            };
            vault.append_log(REVIEWS, &line).unwrap();
        }
    }
}

fn vault() -> (tempfile::TempDir, Vault) {
    let folder = tempfile::tempdir().unwrap();
    let vault = Vault::create(folder.path()).unwrap();
    (folder, vault)
}

const DAYS: [i64; 6] = [0, 1, 3, 7, 15, 30];

#[test]
fn the_cache_is_made_once_there_are_enough_reviews_and_made_again_when_missing() {
    let (_folder, vault) = vault();
    review(&vault, 0..100, &DAYS);
    let now = START + 40 * DAY;
    let status = refresh(&vault, now, &UTC, &Stop::new());
    assert_eq!(status.usable, 500);
    assert_eq!(status.fitted, Some((500, now)));
    let first = cached(&vault).expect("the fit is kept");
    assert_ne!(first.weights, DEFAULT_WEIGHTS);
    assert_eq!(params(&vault, &vault.profile()).weights, first.weights);
    std::fs::remove_file(vault.app_folder().join(CACHE)).unwrap();
    assert_eq!(params(&vault, &vault.profile()).weights, DEFAULT_WEIGHTS);
    let later = now + DAY;
    assert_eq!(
        refresh(&vault, later, &UTC, &Stop::new()).fitted,
        Some((500, later))
    );
    assert_eq!(cached(&vault).unwrap().weights, first.weights);
}

#[test]
fn a_fit_waits_until_the_reviews_have_grown_by_a_fifth() {
    let (_folder, vault) = vault();
    review(&vault, 0..100, &DAYS);
    let now = START + 40 * DAY;
    refresh(&vault, now, &UTC, &Stop::new());
    // Nineteen cards more is 95 reviews more: under a fifth of 500.
    review(&vault, 100..119, &DAYS);
    let status = refresh(&vault, now + DAY, &UTC, &Stop::new());
    assert_eq!((status.usable, status.fitted), (595, Some((500, now))));
    // One more card makes it 600, a fifth more.
    review(&vault, 119..120, &DAYS);
    let status = refresh(&vault, now + 2 * DAY, &UTC, &Stop::new());
    assert_eq!(status.fitted, Some((600, now + 2 * DAY)));
}

#[test]
fn too_few_reviews_keep_the_standard_schedule() {
    let (_folder, vault) = vault();
    review(&vault, 0..79, &DAYS);
    let status = refresh(&vault, START + 40 * DAY, &UTC, &Stop::new());
    assert_eq!((status.usable, status.fitted), (395, None));
    assert_eq!(vault.read_generated(CACHE), None);
    assert_eq!(params(&vault, &vault.profile()).weights, DEFAULT_WEIGHTS);
}

#[test]
fn switched_off_the_schedule_is_the_standard_one_and_nothing_is_fitted() {
    let (_folder, vault) = vault();
    review(&vault, 0..100, &DAYS);
    let mut profile = vault.profile();
    profile.fitted_schedule = false;
    profile.retention = 0.85;
    vault.save_profile(&profile).unwrap();
    let status = refresh(&vault, START + 40 * DAY, &UTC, &Stop::new());
    assert_eq!(status.fitted, None);
    assert_eq!(vault.read_generated(CACHE), None);
    // A fit kept from before is not used either; the target recall is still the profile's.
    keep(
        &vault,
        &Cached {
            weights: DEMO,
            reviews: 1_234,
            fitted: START,
        },
    )
    .unwrap();
    let off = params(&vault, &vault.profile());
    assert_eq!(
        (off.weights, off.desired_retention),
        (DEFAULT_WEIGHTS, 0.85)
    );
    profile.fitted_schedule = true;
    vault.save_profile(&profile).unwrap();
    assert_eq!(params(&vault, &vault.profile()).weights, DEMO);
}

#[test]
fn a_cache_that_cannot_be_used_is_passed_over_and_fitted_again() {
    let (_folder, vault) = vault();
    for bad in [
        &b"not json"[..],
        br#"{"weights": [1, 2, 3], "reviews": 500, "fitted": "2024-01-01T00:00:00Z"}"#,
        br#"{"weights": [0.2,1.3,2.3,8.3,6.4,0.8,3.0,0.0,1.9,0.2,0.8,1.5,0.06,0.26,1.6,0.6,1.9,0.5,0.09,0.07,9.0], "reviews": 500, "fitted": "2024-01-01T00:00:00Z"}"#,
    ] {
        vault.write_generated(CACHE, bad).unwrap();
        assert_eq!(cached(&vault), None);
        assert_eq!(params(&vault, &vault.profile()).weights, DEFAULT_WEIGHTS);
    }
    review(&vault, 0..100, &DAYS);
    let now = START + 40 * DAY;
    assert_eq!(
        refresh(&vault, now, &UTC, &Stop::new()).fitted,
        Some((500, now))
    );
}

#[test]
fn a_stopped_fit_leaves_the_previous_weights() {
    let (_folder, vault) = vault();
    review(&vault, 0..100, &DAYS);
    let stop = Stop::new();
    stop.stop();
    let status = refresh(&vault, START + 40 * DAY, &UTC, &stop);
    assert_eq!((status.fitted, status.failed), (None, false));
    assert_eq!(vault.read_generated(CACHE), None);
}

#[test]
fn the_line_says_whether_the_schedule_is_fitted_from_how_many_reviews_and_when() {
    let date = |_| "12 November 2026".to_string();
    let fitted = Status {
        fitted: Some((1_234, 0)),
        usable: 1_300,
        failed: false,
    };
    assert_eq!(
        describe(true, Some(&fitted), false, date),
        "Fitted to 1,234 of your reviews on 12 November 2026."
    );
    assert_eq!(
        describe(false, Some(&fitted), false, date),
        "Off: every card follows the standard schedule."
    );
    let few = Status {
        fitted: None,
        usable: 57,
        failed: false,
    };
    assert_eq!(
        describe(true, Some(&few), false, date),
        "The standard schedule for now. It is fitted to you once 400 reviews came on a later day \
         than the card's last (57 so far)."
    );
    assert_eq!(
        describe(true, Some(&few), true, date),
        "Fitting the schedule to your reviews…"
    );
    let failed = Status {
        failed: true,
        ..few
    };
    assert!(describe(true, Some(&failed), false, date).contains("did not work"));
    assert_eq!(grouped(1_234_567), "1,234,567");
    assert_eq!(grouped(400), "400");
}
