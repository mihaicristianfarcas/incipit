//! The flashcards' schedule fitted to the learner's own reviews (`incipit_learn::fit`). The
//! weights are derived from `log/reviews.jsonl`, so they are a cache in the vault's generated
//! folder (`.incipit/fitted-schedule.json`), never a source of truth: deleting it only means
//! fitting again. Whether they are used is the learner's choice, `fittedSchedule` in the profile;
//! the target recall still comes from the profile either way.
//!
//! A fit is never started by a timer: it is considered when a vault opens and when a review
//! sitting ends, and runs (off the window's thread) only when there are enough reviews to learn
//! from and the log has grown by a set share since the last fit, or there is no usable cache. A
//! fit that fails or is stopped leaves the previous weights; another vault opening stops one
//! under way. INCIPIT_FIT=demo writes a made-up fit into the open vault's cache when it opens, so
//! Settings shows a fitted schedule without months of reviews (a throwaway vault only).

use gpui::{Context, Entity, Subscription};
use incipit_learn::fit::{self, Logged, Stop, Unfitted, FIT_AFTER};
use incipit_learn::params::DEFAULT_WEIGHTS;
use incipit_learn::{Params, Rating, Zone};
use incipit_vault::{Profile, Review, Vault, REVIEWS};
use serde::{Deserialize, Serialize};

use crate::study::Local;
use crate::workspace::{Happened, Workspace};

/// The cache's name in the vault's generated folder.
pub const CACHE: &str = "fitted-schedule.json";

/// How much the reviews to learn from must have grown since the last fit before fitting again: a
/// fifth. A share rather than a count, so a new learner's weights follow their first months closely
/// and a long log is seldom fitted again: from 400 reviews, the fits come at 480, 576, 692 and so
/// on. Anki's manual finds there is "no need to optimize your parameters frequently: once every
/// month is sufficient".
const GROWTH: f64 = 1.2;

/// The cache as it is kept: JSON, the time in RFC 3339.
#[derive(Serialize, Deserialize)]
struct CacheFile {
    weights: Vec<f64>,
    reviews: usize,
    fitted: String,
}

/// A fit read back from the cache, its weights checked.
#[derive(Clone, Debug, PartialEq)]
pub struct Cached {
    pub weights: [f64; 21],
    /// How many reviews it learnt from.
    pub reviews: usize,
    /// When it was made, in ms since the epoch.
    pub fitted: i64,
}

/// The vault's fit, if its cache is there and can be used: weights a fit can give and a time.
pub fn cached(vault: &Vault) -> Option<Cached> {
    let file: CacheFile = serde_json::from_slice(&vault.read_generated(CACHE)?).ok()?;
    Some(Cached {
        weights: fit::checked(&file.weights)?,
        reviews: file.reviews,
        fitted: chrono::DateTime::parse_from_rfc3339(&file.fitted)
            .ok()?
            .timestamp_millis(),
    })
}

/// Keeps a fit in the vault's cache.
pub(crate) fn keep(vault: &Vault, fitted: &Cached) -> incipit_vault::Result<()> {
    let file = CacheFile {
        weights: fitted.weights.to_vec(),
        reviews: fitted.reviews,
        fitted: chrono::DateTime::from_timestamp_millis(fitted.fitted)
            .unwrap_or_default()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    };
    // A struct of numbers and a string always serializes.
    let json = serde_json::to_vec_pretty(&file).unwrap_or_default();
    vault.write_generated(CACHE, &json)
}

/// The scheduler's settings for this vault: the fitted weights when the learner wants them and
/// there are some, the standard ones otherwise, and the profile's target recall.
pub fn params(vault: &Vault, profile: &Profile) -> Params {
    let weights = profile
        .fitted_schedule
        .then(|| cached(vault))
        .flatten()
        .map_or(DEFAULT_WEIGHTS, |c| c.weights);
    Params {
        weights,
        desired_retention: profile.retention,
        ..Params::default()
    }
}

/// The review log as the fit reads it. Lines that cannot be read are passed over, as the deck
/// passes them over.
fn logged(vault: &Vault) -> Vec<Logged<String>> {
    let lines = vault
        .read_log::<Review>(REVIEWS)
        .map(|log| log.entries)
        .unwrap_or_default();
    lines
        .into_iter()
        .filter_map(|line| {
            let time = chrono::DateTime::parse_from_rfc3339(&line.at)
                .ok()?
                .timestamp_millis();
            let rating = *Rating::ALL.get(usize::from(line.rating).checked_sub(1)?)?;
            Some(Logged {
                card: line.card,
                time,
                rating,
            })
        })
        .collect()
}

/// Whether a fit is due: enough reviews to learn from, and no usable fit yet, or the reviews have
/// grown by `GROWTH` since it, or are fewer than it learnt from (the log was cut).
fn due(cached: Option<&Cached>, usable: usize) -> bool {
    usable >= FIT_AFTER
        && cached.is_none_or(|c| usable as f64 >= c.reviews as f64 * GROWTH || usable < c.reviews)
}

/// Where the fitted schedule stands, for the line in Settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    /// The fit in the cache: from how many reviews, and when (ms since the epoch).
    pub fitted: Option<(usize, i64)>,
    /// How many reviews the log gives a fit now.
    pub usable: usize,
    /// The last fit tried did not work; the previous weights, if any, stay.
    pub failed: bool,
}

/// Where the fitted schedule stands, and whether a fit is due and wanted. Reading the log is
/// quick, but it is a file of any length: call it off the window's thread.
pub fn survey(vault: &Vault, zone: &impl Zone) -> (Status, bool) {
    let usable = fit::usable(&logged(vault), zone);
    let before = cached(vault);
    let status = Status {
        fitted: before.as_ref().map(|c| (c.reviews, c.fitted)),
        usable,
        failed: false,
    };
    let wanted = vault.profile().fitted_schedule && due(before.as_ref(), usable);
    (status, wanted)
}

/// Fits the weights to the vault's log and keeps them in the cache, and says where things stand
/// after. It blocks for as long as the fit takes: call it off the window's thread.
fn fit_and_keep(
    vault: &Vault,
    mut status: Status,
    now: i64,
    zone: &impl Zone,
    stop: &Stop,
) -> Status {
    match fit::fit(&logged(vault), zone, stop) {
        Ok(found) => {
            let made = Cached {
                weights: found.weights,
                reviews: found.items,
                fitted: now,
            };
            match keep(vault, &made) {
                Ok(()) => status.fitted = Some((made.reviews, made.fitted)),
                Err(_) => status.failed = true,
            }
        }
        Err(Unfitted::TooFew(_) | Unfitted::Stopped) => {}
        Err(Unfitted::Failed(_)) => status.failed = true,
    }
    status
}

/// Fits the weights if a fit is due and the learner wants one, and says where things stand: the
/// survey and the fit in one call.
#[cfg(test)]
pub fn refresh(vault: &Vault, now: i64, zone: &impl Zone, stop: &Stop) -> Status {
    let (status, wanted) = survey(vault, zone);
    if wanted {
        fit_and_keep(vault, status, now, zone, stop)
    } else {
        status
    }
}

/// A number as a person writes it: 1,234.
fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The line beside the switch in Settings: whether the schedule is fitted, from how many reviews
/// and when. `status` is none until the vault has been looked at, `running` says a fit is under
/// way, and `date` writes a time as a day.
pub fn describe(
    on: bool,
    status: Option<&Status>,
    running: bool,
    date: impl Fn(i64) -> String,
) -> String {
    if !on {
        return "Off: every card follows the standard schedule.".into();
    }
    match status {
        Some(Status {
            fitted: Some((reviews, when)),
            ..
        }) => format!(
            "Fitted to {} of your reviews on {}.",
            grouped(*reviews),
            date(*when)
        ),
        _ if running => "Fitting the schedule to your reviews…".into(),
        Some(Status { failed: true, .. }) => {
            "The standard schedule: fitting it to your reviews did not work.".into()
        }
        Some(Status { usable, .. }) => format!(
            "The standard schedule for now. It is fitted to you once {} reviews came on a later \
             day than the card's last ({} so far).",
            grouped(FIT_AFTER),
            grouped(*usable)
        ),
        None => "The standard schedule for now.".into(),
    }
}

/// A day as the learner reads it, on this computer's clock: 12 November 2026.
pub fn local_date(at: i64) -> String {
    chrono::DateTime::from_timestamp_millis(at)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%-d %B %Y")
                .to_string()
        })
        .unwrap_or_default()
}

/// Weights of the kind a fit gives, for INCIPIT_FIT=demo.
const DEMO: [f64; 21] = [
    0.17, 0.39, 0.94, 1.63, 6.52, 0.67, 3.25, 0.014, 1.63, 0.21, 0.57, 1.35, 0.1, 0.14, 1.58, 0.62,
    1.87, 0.64, 0.004, 0.067, 0.11,
];

/// Fitting for the open vault: considered when a vault opens and when a sitting ends.
pub struct Fitting {
    workspace: Entity<Workspace>,
    /// Stops the check under way (its survey, then its fit), if one is.
    stop: Option<Stop>,
    /// A fit is under way.
    fitting: bool,
    /// Counts the vaults opened and the checks begun, so a check that ends after another vault
    /// opened, or another check began, is not taken.
    generation: u64,
    pub status: Option<Status>,
    _subscription: Subscription,
}

impl Fitting {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.subscribe(&workspace, |this: &mut Self, _, happened, cx| {
            if let Happened::Opened = happened {
                this.opened(cx);
            }
        });
        let mut fitting = Fitting {
            workspace,
            stop: None,
            fitting: false,
            generation: 0,
            status: None,
            _subscription: subscription,
        };
        fitting.opened(cx);
        fitting
    }

    /// Whether a fit is under way.
    pub fn running(&self) -> bool {
        self.fitting
    }

    /// Another vault (or none) is open: what was being fitted for the last one stops.
    fn opened(&mut self, cx: &mut Context<Self>) {
        if let Some(stop) = self.stop.take() {
            stop.stop();
        }
        self.generation += 1;
        self.fitting = false;
        self.status = None;
        if std::env::var("INCIPIT_FIT").as_deref() == Ok("demo") {
            if let Some(vault) = &self.workspace.read(cx).vault {
                let made = Cached {
                    weights: DEMO,
                    reviews: 1_234,
                    fitted: crate::study::now(),
                };
                let _ = keep(vault, &made);
            }
        }
        self.check(cx);
        cx.notify();
    }

    /// Looks at the open vault's log off the window's thread and, if a fit is due, fits it there
    /// too. A check while one is under way is left out: it reads the log as it is now.
    pub fn check(&mut self, cx: &mut Context<Self>) {
        if self.stop.is_some() {
            return;
        }
        let Some(vault) = self.workspace.read(cx).vault.clone() else {
            return;
        };
        let stop = Stop::new();
        self.stop = Some(stop.clone());
        self.generation += 1;
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            let looked = vault.clone();
            let (status, wanted) = cx
                .background_executor()
                .spawn(async move { survey(&looked, &Local) })
                .await;
            let go_on = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return false;
                }
                this.status = Some(status.clone());
                this.fitting = wanted;
                if !wanted {
                    this.stop = None;
                }
                cx.notify();
                wanted
            });
            if !matches!(go_on, Ok(true)) {
                return;
            }
            let status = cx
                .background_executor()
                .spawn(
                    async move { fit_and_keep(&vault, status, crate::study::now(), &Local, &stop) },
                )
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.generation == generation {
                    this.stop = None;
                    this.fitting = false;
                    this.status = Some(status);
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests;
