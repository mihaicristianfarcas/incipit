//! Speaking instead of typing, as asides does it: the microphone is recorded into memory
//! (`record`), the recording is brought to Whisper's 16 kHz (`resample`) and written down by a
//! Whisper model that runs on this computer (`transcribe`), which is downloaded once, when the
//! learner asks, and checked against its published checksum (`models`). Nothing said leaves the
//! computer, and no model is reached through a provider: the words are only typed into a field.
//!
//! The crate has no window in it; `incipit-app`'s `voice` module puts it behind the fields.

pub mod models;
pub mod record;
pub mod resample;
pub mod transcribe;
pub mod wav;

pub use models::{Model, MODELS};
pub use record::Recorder;
pub use resample::resample;
pub use transcribe::{is_silent, Transcriber, RATE};
