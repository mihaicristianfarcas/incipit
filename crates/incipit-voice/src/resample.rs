//! A finished recording brought to another rate (Whisper hears 16 kHz; microphones give 44.1 or
//! 48 kHz), by windowed-sinc interpolation: each sample out is a weighted sum of the samples in
//! around it, the weights a low-pass filter below the lower of the two Nyquist frequencies, so
//! sound too high for the new rate is removed instead of folding back into the voice.

use std::f64::consts::PI;

/// Zero crossings of the filter on each side, at the lower rate: 16 leaves the voice's band
/// flat and what is above the new Nyquist frequency at least 60 dB down.
const CROSSINGS: f64 = 16.0;

/// Steps of the filter's table per sample in: fine enough that reading between steps adds no
/// error a voice model hears.
const STEPS: usize = 512;

/// `input`, at `from` Hz, as it would be at `to` Hz.
pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() || from == 0 || to == 0 {
        return input.to_vec();
    }
    let ratio = f64::from(to) / f64::from(from);
    let scale = ratio.min(1.0);
    // In cycles per sample in, a little below the Nyquist frequency of the lower rate.
    let cutoff = 0.5 * scale * 0.94;
    let reach = CROSSINGS / scale;
    let filter = Filter::new(cutoff, reach);
    let last = input.len() as i64 - 1;
    let span = reach.ceil() as i64;
    let length = (input.len() as f64 * ratio).round() as usize;
    (0..length)
        .map(|n| {
            let at = n as f64 / ratio;
            let near = at.floor() as i64;
            ((near - span + 1).max(0)..=(near + span).min(last))
                .map(|k| f64::from(input[k as usize]) * filter.at((at - k as f64).abs()))
                .sum::<f64>() as f32
        })
        .collect()
}

/// The low-pass filter's weights, by distance in samples in: a sinc under a Blackman window.
struct Filter {
    table: Vec<f64>,
}

impl Filter {
    fn new(cutoff: f64, reach: f64) -> Filter {
        let length = (reach * STEPS as f64).ceil() as usize + 2;
        let table = (0..length)
            .map(|i| {
                let d = i as f64 / STEPS as f64;
                if d >= reach {
                    return 0.0;
                }
                let x = PI * 2.0 * cutoff * d;
                let sinc = if x == 0.0 { 1.0 } else { x.sin() / x };
                let u = PI * d / reach;
                let window = 0.42 + 0.5 * u.cos() + 0.08 * (2.0 * u).cos();
                2.0 * cutoff * sinc * window
            })
            .collect();
        Filter { table }
    }

    /// The weight at `distance`, read between the table's steps.
    fn at(&self, distance: f64) -> f64 {
        let place = distance * STEPS as f64;
        let i = place as usize;
        match (self.table.get(i), self.table.get(i + 1)) {
            (Some(a), Some(b)) => a + (b - a) * (place - i as f64),
            _ => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f64, rate: u32, seconds: f64) -> Vec<f32> {
        (0..(f64::from(rate) * seconds) as usize)
            .map(|i| (2.0 * PI * hz * i as f64 / f64::from(rate)).sin() as f32 * 0.5)
            .collect()
    }

    fn rms(samples: &[f32]) -> f64 {
        (samples.iter().map(|&s| f64::from(s).powi(2)).sum::<f64>() / samples.len() as f64).sqrt()
    }

    #[test]
    fn a_voice_keeps_its_loudness_and_its_length() {
        let heard = tone(440.0, 48_000, 1.0);
        let out = resample(&heard, 48_000, 16_000);
        assert_eq!(out.len(), 16_000);
        // Away from the ends, where the filter runs out of samples.
        let middle = &out[1000..15_000];
        assert!((rms(middle) - rms(&heard)).abs() < 0.01, "{}", rms(middle));
    }

    #[test]
    fn sound_too_high_for_the_new_rate_is_removed() {
        // 12 kHz is above 16 kHz's Nyquist frequency (8 kHz): it would fold back to 4 kHz.
        let out = resample(&tone(12_000.0, 48_000, 1.0), 48_000, 16_000);
        assert!(
            rms(&out[1000..15_000]) < 0.005,
            "{}",
            rms(&out[1000..15_000])
        );
    }

    #[test]
    fn the_same_rate_or_nothing_is_left_as_it_is() {
        assert_eq!(resample(&[0.1, 0.2], 16_000, 16_000), vec![0.1, 0.2]);
        assert!(resample(&[], 48_000, 16_000).is_empty());
        assert_eq!(
            resample(&tone(440.0, 44_100, 0.5), 44_100, 16_000).len(),
            8000
        );
    }
}
