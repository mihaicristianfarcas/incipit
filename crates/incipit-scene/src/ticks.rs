//! Numbers on axes: round tick values for a range, and numbers written the way a page prints them
//! (no trailing zeros, a true minus sign, thousands grouped only where a year could not be meant).

/// A round step near `raw`: 1, 2 or 5 times a power of ten.
pub fn nice_step(raw: f64) -> f64 {
    if !raw.is_finite() || raw <= 0.0 {
        return 1.0;
    }
    let mag = 10f64.powf(raw.log10().floor());
    let norm = raw / mag;
    mag * if norm < 1.5 {
        1.0
    } else if norm < 3.0 {
        2.0
    } else if norm < 7.0 {
        5.0
    } else {
        10.0
    }
}

/// Round tick values between `a` and `b`, about `count` of them.
pub fn nice_ticks(a: f32, b: f32, count: usize) -> Vec<f32> {
    if !a.is_finite() || !b.is_finite() || a == b {
        return vec![a];
    }
    let (lo, hi) = (a.min(b) as f64, a.max(b) as f64);
    let step = nice_step((hi - lo) / count.max(1) as f64);
    let mut out = Vec::new();
    let mut i = (lo / step - 1e-6).ceil();
    while i * step <= hi + step * 1e-4 && out.len() < 64 {
        let v = i * step;
        out.push(if v.abs() < step * 1e-4 { 0.0 } else { v as f32 });
        i += 1.0;
    }
    out
}

/// The step between ticks `nice_ticks` gives for the same range.
pub fn tick_step(ticks: &[f32]) -> f32 {
    match ticks {
        [a, b, ..] => b - a,
        _ => 1.0,
    }
}

/// `lo` and `hi` pushed out to the nearest ticks around them, so the data never touches the edge
/// of the frame and the axis ends on a labelled number.
pub fn nice_extent(lo: f32, hi: f32, count: usize) -> (f32, f32) {
    if !lo.is_finite() || !hi.is_finite() {
        return (0.0, 1.0);
    }
    let (lo, hi) = if (hi - lo).abs() < 1e-9 {
        let pad = if lo == 0.0 { 1.0 } else { lo.abs() * 0.5 };
        (lo - pad, hi + pad)
    } else {
        (lo.min(hi), lo.max(hi))
    };
    let step = nice_step(((hi - lo) / count.max(1) as f32) as f64);
    let a = ((lo as f64 / step) + 1e-6).floor() * step;
    let b = ((hi as f64 / step) - 1e-6).ceil() * step;
    (a as f32, b as f32)
}

/// A number as an axis shows it: no trailing zeros.
pub fn tick_label(v: f32) -> String {
    let s = format!("{:.2}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0".into()
    } else {
        s.replace('-', "−")
    }
}

/// A tick's number with as many decimals as the step between ticks needs, and thousands grouped
/// from 10 000 up (so 1789 stays a year).
pub fn axis_label(v: f32, step: f32) -> String {
    let decimals = if step > 0.0 && step.is_finite() {
        (-(step as f64).log10().floor()).clamp(0.0, 6.0) as usize
    } else {
        2
    };
    let s = format!("{:.*}", decimals, v);
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    };
    let s = if s == "-0" { "0".to_string() } else { s };
    group(&s).replace('-', "−")
}

/// A number in a sentence or a readout: up to three decimals, none trailing.
pub fn number(v: f32) -> String {
    if !v.is_finite() {
        return if v.is_nan() {
            "not a number".into()
        } else if v > 0.0 {
            "infinity".into()
        } else {
            "−infinity".into()
        };
    }
    let decimals = if v.abs() >= 100.0 {
        1
    } else if v.abs() >= 10.0 {
        2
    } else {
        3
    };
    let s = format!("{:.*}", decimals, v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    let s = if s == "-0" || s.is_empty() { "0" } else { s };
    group(s).replace('-', "−")
}

/// Digits grouped by three with commas, from five digits before the point.
fn group(s: &str) -> String {
    let (sign, rest) = s.strip_prefix('-').map_or(("", s), |r| ("-", r));
    let (whole, fraction) = rest
        .split_once('.')
        .map_or((rest, None), |(w, f)| (w, Some(f)));
    if whole.len() < 5 || !whole.chars().all(|c| c.is_ascii_digit()) {
        return s.to_string();
    }
    let mut out = String::new();
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    match fraction {
        Some(f) => format!("{sign}{out}.{f}"),
        None => format!("{sign}{out}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_are_round_and_inside_the_range() {
        assert_eq!(
            nice_ticks(0.0, 10.0, 5),
            vec![0.0, 2.0, 4.0, 6.0, 8.0, 10.0]
        );
        let ticks = nice_ticks(-3.3, 7.9, 6);
        assert!(ticks.iter().all(|t| (-3.3..=7.9).contains(t)), "{ticks:?}");
        assert!(ticks.contains(&0.0));
    }

    #[test]
    fn an_extent_ends_on_ticks_around_the_data() {
        assert_eq!(nice_extent(3.0, 97.0, 5), (0.0, 100.0));
        let (a, b) = nice_extent(5.0, 5.0, 5);
        assert!(a < 5.0 && b > 5.0);
    }

    #[test]
    fn numbers_read_as_a_page_prints_them() {
        assert_eq!(axis_label(1789.0, 10.0), "1789");
        assert_eq!(axis_label(25000.0, 5000.0), "25,000");
        assert_eq!(axis_label(-0.5, 0.25), "−0.5");
        assert_eq!(axis_label(0.25, 0.05), "0.25");
        assert_eq!(number(2.0), "2");
        assert_eq!(number(-1.23456), "−1.235");
        assert_eq!(number(f32::NAN), "not a number");
    }
}
