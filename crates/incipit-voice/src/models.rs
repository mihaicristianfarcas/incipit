//! The Whisper models a learner can download: ggml files from whisper.cpp's own page on Hugging
//! Face, quantised so they fit and run fast. English is the default: it is smaller and is never
//! mistaken for another language; the other writes down any language Whisper knows. A model is
//! downloaded only when the learner asks, into a `.part` file that becomes the model only once
//! its SHA-256 matches the one written here, so a cut download or a changed file is never loaded.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

/// A model, as published.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Model {
    pub id: &'static str,
    /// What the learner reads when choosing.
    pub label: &'static str,
    pub file: &'static str,
    pub sha256: &'static str,
    /// In bytes.
    pub size: u64,
    /// Writes English only: no language is detected.
    pub english: bool,
}

pub const MODELS: &[Model] = &[
    Model {
        id: "small.en",
        label: "English",
        file: "ggml-small.en-q5_1.bin",
        sha256: "bfdff4894dcb76bbf647d56263ea2a96645423f1669176f4844a1bf8e478ad30",
        size: 190_098_681,
        english: true,
    },
    Model {
        id: "large-v3-turbo",
        label: "Any language",
        file: "ggml-large-v3-turbo-q5_0.bin",
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        size: 574_041_195,
        english: false,
    },
];

/// How often a download says how far it has got: often enough to move a bar, rarely enough not
/// to redraw the window for every megabyte.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

impl Model {
    /// The model with `id`, or the default (English) for an id this build does not know.
    pub fn find(id: &str) -> &'static Model {
        MODELS.iter().find(|m| m.id == id).unwrap_or(&MODELS[0])
    }

    /// Where it is kept in `dir`.
    pub fn path(&self, dir: &Path) -> PathBuf {
        dir.join(self.file)
    }

    /// It is in `dir`, checked.
    pub fn downloaded(&self, dir: &Path) -> bool {
        self.path(dir).is_file()
    }

    /// "190 MB".
    pub fn size_label(&self) -> String {
        match self.size {
            s if s >= 1_000_000_000 => format!("{:.1} GB", s as f64 / 1e9),
            s => format!("{} MB", s / 1_000_000),
        }
    }

    pub fn url(&self) -> String {
        format!(
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}",
            self.file
        )
    }

    /// Downloads it into `dir`, telling `progress` how far it has got (0 to 1). Blocks: run it on
    /// a thread of its own.
    pub fn download(&self, dir: &Path, progress: impl Fn(f32)) -> Result<()> {
        let response = ureq::get(&self.url())
            .call()
            .context("The download could not start. Check the connection to the internet")?;
        let reader = response
            .into_body()
            .into_with_config()
            .limit(u64::MAX)
            .reader();
        keep(reader, self, dir, progress)
    }
}

/// Writes what `reader` gives into `model`'s file in `dir`, through a `.part` file that is kept
/// only when its SHA-256 is the model's.
fn keep(mut reader: impl Read, model: &Model, dir: &Path, progress: impl Fn(f32)) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("{} could not be made", dir.display()))?;
    let path = model.path(dir);
    let partial = path.with_extension("part");
    let mut file = File::create(&partial)
        .with_context(|| format!("{} could not be written", partial.display()))?;
    let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buffer = vec![0u8; 1 << 20];
    let (mut done, mut told) = (0u64, Instant::now());
    loop {
        let n = reader
            .read(&mut buffer)
            .context("The download was cut off. Try again")?;
        if n == 0 {
            break;
        }
        file.write_all(&buffer[..n])
            .context("The model could not be written: is the disk full?")?;
        digest.update(&buffer[..n]);
        done += n as u64;
        if told.elapsed() >= PROGRESS_EVERY {
            progress((done as f64 / model.size.max(1) as f64).min(1.0) as f32);
            told = Instant::now();
        }
    }
    file.sync_all()?;
    drop(file);
    let sum: String = digest
        .finish()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if sum != model.sha256 {
        let _ = fs::remove_file(&partial);
        bail!("The downloaded model is not the one published (its checksum differs). Try again");
    }
    fs::rename(&partial, &path).with_context(|| format!("{} could not be kept", path.display()))?;
    progress(1.0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_model_is_the_english_one() {
        assert_eq!(Model::find("large-v3-turbo").id, "large-v3-turbo");
        assert_eq!(Model::find("gone").id, "small.en");
        assert_eq!(Model::find("small.en").size_label(), "190 MB");
    }

    #[test]
    fn a_download_is_kept_only_when_its_checksum_is_the_published_one() {
        let dir = tempfile::tempdir().unwrap();
        // The SHA-256 of "hello".
        let model = Model {
            sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
            size: 5,
            ..MODELS[0]
        };
        assert!(keep(&b"hullo"[..], &model, dir.path(), |_| {}).is_err());
        assert!(!model.downloaded(dir.path()));
        assert!(!model.path(dir.path()).with_extension("part").exists());
        let said = std::cell::Cell::new(0.0);
        keep(&b"hello"[..], &model, dir.path(), |p| said.set(p)).unwrap();
        assert!(model.downloaded(dir.path()));
        assert_eq!(said.get(), 1.0);
    }
}
