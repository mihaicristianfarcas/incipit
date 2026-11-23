//! The microphone: the system's default input, recorded as mono samples at the device's own
//! rate, kept in memory until the learner stops (a minute of 48 kHz is under 12 MB). The audio
//! thread tells the caller each 50 ms of sound heard ([`HEARD_EVERY`]), so a level can be drawn
//! without a timer, and no faster than it can be seen.
//! Each call to `start` asks the system for the microphone; on macOS the first one asks the
//! learner whether Incipit may use it.

use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};

pub struct Recorder {
    stream: cpal::Stream,
    samples: Arc<Mutex<Vec<f32>>>,
    pub rate: u32,
}

/// How often the caller is told more was heard: every twentieth of a second of sound.
pub const HEARD_EVERY: u32 = 20;

/// Called on the audio thread as more is heard.
type Heard = Arc<dyn Fn() + Send + Sync>;

impl Recorder {
    /// Starts recording from the default microphone; `heard` is called on the audio thread each
    /// 50 ms of sound.
    pub fn start(heard: impl Fn() + Send + Sync + 'static) -> Result<Recorder> {
        let device = cpal::default_host().default_input_device().context(
            "No microphone was found. Plug one in, or choose one in the system's sound settings",
        )?;
        let supported = device
            .default_input_config()
            .context("The microphone gives no sound Incipit can use")?;
        let channels = supported.channels() as usize;
        let rate = supported.sample_rate();
        let samples = Arc::new(Mutex::new(Vec::with_capacity(rate as usize * 60)));
        let config = supported.config();
        let heard: Heard = Arc::new(heard);
        let stream = match supported.sample_format() {
            SampleFormat::F32 => {
                build::<f32>(&device, config, channels, rate, samples.clone(), heard)
            }
            SampleFormat::I16 => {
                build::<i16>(&device, config, channels, rate, samples.clone(), heard)
            }
            SampleFormat::I32 => {
                build::<i32>(&device, config, channels, rate, samples.clone(), heard)
            }
            SampleFormat::U16 => {
                build::<u16>(&device, config, channels, rate, samples.clone(), heard)
            }
            other => bail!("The microphone gives sound as {other:?}, which Incipit cannot read"),
        }?;
        stream.play().context(
            "The microphone could not be started. If Incipit is not allowed to use it, allow it \
             in the system's privacy settings",
        )?;
        Ok(Recorder {
            stream,
            samples,
            rate,
        })
    }

    /// How loud the last 50 ms were, from 0 (−50 dB or quieter) to 1 (−10 dB or louder).
    pub fn level(&self) -> f32 {
        let Ok(samples) = self.samples.lock() else {
            return 0.0;
        };
        let recent = &samples[samples.len().saturating_sub(self.rate as usize / 20)..];
        ((loudness(recent) + 50.0) / 40.0).clamp(0.0, 1.0)
    }

    /// Stops recording: the samples and their rate.
    pub fn finish(self) -> (Vec<f32>, u32) {
        drop(self.stream);
        let samples = std::mem::take(&mut *self.samples.lock().unwrap_or_else(|e| e.into_inner()));
        (samples, self.rate)
    }
}

/// How loud `samples` are, in dB of full scale (−120 for none), around their mean: an offset some
/// microphones add is not sound.
pub fn loudness(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return -120.0;
    }
    let n = samples.len() as f64;
    let mean = samples.iter().map(|&s| f64::from(s)).sum::<f64>() / n;
    let rms = (samples
        .iter()
        .map(|&s| (f64::from(s) - mean).powi(2))
        .sum::<f64>()
        / n)
        .sqrt();
    (20.0 * rms.max(1e-6).log10()) as f32
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    rate: u32,
    samples: Arc<Mutex<Vec<f32>>>,
    heard: Heard,
) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let every = (rate / HEARD_EVERY).max(1) as usize;
    let mut since = 0usize;
    let stream = device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            if let Ok(mut out) = samples.lock() {
                // The channels averaged into one.
                out.extend(data.chunks(channels).map(|frame| {
                    frame.iter().map(|s| f32::from_sample(*s)).sum::<f32>() / channels as f32
                }));
            }
            since += data.len() / channels.max(1);
            if since >= every {
                since = 0;
                heard();
            }
        },
        // A stream that fails mid-way leaves what was heard; stopping says what there is.
        |_| {},
        None,
    )?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_offset_the_microphone_adds_is_not_loud() {
        // A tone at a tenth of full scale is −23 dB, on an offset or not.
        let tone: Vec<f32> = (0..4800).map(|i| 0.1 * (i as f32 * 0.05).sin()).collect();
        let offset: Vec<f32> = tone.iter().map(|s| s + 0.4).collect();
        assert!((loudness(&tone) + 23.0).abs() < 0.5);
        assert!((loudness(&offset) - loudness(&tone)).abs() < 0.1);
        assert!(loudness(&[0.4; 4800]) < -100.0);
        assert_eq!(loudness(&[]), -120.0);
    }
}
