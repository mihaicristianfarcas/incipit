//! Writing down what was said, with whisper.cpp (through `whisper-rs`) and a model on disk.
//! Loading a model takes a second or more, so a `Transcriber` is made once and kept. An English
//! model writes English; the other detects the language of each recording. Each recording is
//! written down on its own (no context carried from the last), and what Whisper writes for
//! sound that is not speech, "[BLANK_AUDIO]" or "(music)", is left out.

use std::path::Path;

use anyhow::{Context, Result};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::models::Model;

/// The rate Whisper hears, in samples a second.
pub const RATE: u32 = 16_000;

/// Whisper writes nothing for less than a second: a shorter recording is padded with silence to
/// 1.1 s.
const SHORTEST: usize = 17_600;

/// Below this RMS (about −54 dB) throughout, a recording is silence: the microphone was muted or
/// gave nothing.
const QUIET: f32 = 0.002;

pub struct Transcriber {
    context: WhisperContext,
    english: bool,
    threads: i32,
}

impl Transcriber {
    /// Loads `model` from `dir`. Blocks for a second or more: run it off the window's thread.
    pub fn load(model: &Model, dir: &Path) -> Result<Transcriber> {
        // whisper.cpp writes to the terminal otherwise.
        whisper_rs::install_logging_hooks();
        let path = model.path(dir);
        let path = path
            .to_str()
            .context("The voice model's folder has an unusual name")?;
        let context = WhisperContext::new_with_params(path, WhisperContextParameters::default())
            .context("The voice model could not be loaded. Download it again in Settings")?;
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8)) as i32;
        Ok(Transcriber {
            context,
            english: model.english,
            threads,
        })
    }

    /// What was said in `pcm` (mono, at [`RATE`]), as text.
    pub fn transcribe(&self, pcm: &[f32]) -> Result<String> {
        let mut padded;
        let pcm = if pcm.len() < SHORTEST {
            padded = pcm.to_vec();
            padded.resize(SHORTEST, 0.0);
            &padded[..]
        } else {
            pcm
        };
        let mut state = self
            .context
            .create_state()
            .context("The voice model could not start")?;
        let mut params = FullParams::new(SamplingStrategy::BeamSearch {
            beam_size: 5,
            patience: -1.0,
        });
        params.set_language(Some(if self.english { "en" } else { "auto" }));
        params.set_n_threads(self.threads);
        params.set_translate(false);
        params.set_no_context(true);
        params.set_suppress_nst(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        state
            .full(params, pcm)
            .context("What was said could not be written down")?;
        let segments: Vec<String> = state
            .as_iter()
            .map(|s| s.to_str_lossy().map(|t| t.into_owned()))
            .collect::<Result<_, _>>()
            .context("What was said could not be read back")?;
        Ok(tidy(&segments))
    }
}

/// The segments as one text: trimmed, joined by a space, without those that only name a sound.
fn tidy(segments: &[String]) -> String {
    segments
        .iter()
        .map(|s| s.trim())
        .filter(|s| {
            let named = |open, close| s.starts_with(open) && s.ends_with(close);
            !s.is_empty() && !named('[', ']') && !named('(', ')')
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether nothing could be heard in `pcm` throughout.
pub fn is_silent(pcm: &[f32]) -> bool {
    pcm.chunks(480).all(|frame| {
        let rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();
        rms < QUIET
    })
}

#[cfg(test)]
mod tests {
    //! The test that writes down speech needs a model: `INCIPIT_VOICE_MODEL` names a folder with
    //! the English one (`ggml-small.en-q5_1.bin`) in it, and `INCIPIT_VOICE_WAV` a recording of
    //! "the quick brown fox" (macOS: `say -o fox.wav --data-format=LEI16@16000 "The quick brown
    //! fox"`). Run it with `cargo test -p incipit-voice -- --ignored`.
    use super::*;

    #[test]
    fn what_only_names_a_sound_is_left_out() {
        let said = [
            " Hello there.",
            " [BLANK_AUDIO]",
            "(music)",
            " How are you?",
        ];
        let said: Vec<String> = said.iter().map(|s| s.to_string()).collect();
        assert_eq!(tidy(&said), "Hello there. How are you?");
        assert_eq!(tidy(&[]), "");
    }

    #[test]
    fn silence_is_heard_as_silence() {
        assert!(is_silent(&[0.0; 16_000]));
        assert!(is_silent(&[]));
        let voice: Vec<f32> = (0..16_000).map(|i| 0.2 * (i as f32 * 0.07).sin()).collect();
        assert!(!is_silent(&voice));
    }

    #[test]
    #[ignore = "requires a voice model and a recording (INCIPIT_VOICE_MODEL, INCIPIT_VOICE_WAV)"]
    fn a_recording_is_written_down() {
        let dir = std::env::var("INCIPIT_VOICE_MODEL").expect("INCIPIT_VOICE_MODEL");
        let wav = std::env::var("INCIPIT_VOICE_WAV").expect("INCIPIT_VOICE_WAV");
        let (pcm, rate) = crate::wav::read(Path::new(&wav)).unwrap();
        let pcm = crate::resample(&pcm, rate, RATE);
        let transcriber = Transcriber::load(Model::find("small.en"), Path::new(&dir)).unwrap();
        let text = transcriber.transcribe(&pcm).unwrap().to_lowercase();
        assert!(text.contains("quick brown fox"), "{text}");
    }
}
