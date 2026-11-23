//! A WAV file read as one channel of samples: for checks that say something without a
//! microphone (`INCIPIT_DICTATE`), and for the test that writes speech down. A file that is not
//! a WAV, or is cut short, is an error.

use std::path::Path;

use anyhow::{Context, Result};

/// The file's samples, its channels averaged into one, and their rate.
pub fn read(path: &Path) -> Result<(Vec<f32>, u32)> {
    let mut reader = hound::WavReader::open(path)
        .with_context(|| format!("{} is not a WAV file Incipit can read", path.display()))?;
    let spec = reader.spec();
    let channels = usize::from(spec.channels.max(1));
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let full = (1i64 << (spec.bits_per_sample.clamp(1, 32) - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|s| s as f32 / full))
                .collect::<Result<_, _>>()?
        }
    };
    let mono = samples
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
        .collect();
    Ok((mono, spec.sample_rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_channels_are_heard_as_one_and_a_text_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("two.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for _ in 0..10 {
            writer.write_sample(16_384i16).unwrap();
            writer.write_sample(0i16).unwrap();
        }
        writer.finalize().unwrap();
        let (mono, rate) = read(&path).unwrap();
        assert_eq!((mono.len(), rate), (10, 16_000));
        assert!((mono[0] - 0.25).abs() < 1e-6);
        let text = dir.path().join("note.wav");
        std::fs::write(&text, "not sound").unwrap();
        assert!(read(&text).is_err());
    }
}
