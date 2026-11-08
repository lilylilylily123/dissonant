//! A fetched sample → where its hits are, how fast it goes, what key it is in.
//!
//! Runs once per sample on a worker thread, never on the audio thread, over the whole decoded
//! source. Everything here is deterministic: the same buffer always yields the same
//! [`Analysis`], so a re-fetch never reshuffles the default chops.
//!
//! The pipeline, every stage chosen for being explainable rather than clever:
//!
//! 1. **Fold-down**: all channels averaged and boxcar-decimated by an integer factor to about
//!    [`TARGET_RATE`]. Nothing analysed here lives above 5 kHz, and a quarter of the samples is
//!    what keeps a ten-minute source to well under a second.
//! 2. **Onsets** from spectral flux: the summed rise of log-compressed magnitudes between
//!    512-point frames every 64 samples (~5 ms). A frame is an onset when it is the largest
//!    within [`PEAK_SECONDS`] and stands [`ONSET_DEVIATIONS`] standard deviations over the
//!    novelty around it — an adaptive threshold, so a quiet intro still has hits. The frame
//!    only says "somewhere in this 43 ms window"; the time is then refined in the waveform to
//!    the 1.3 ms block whose energy jumps most over the blocks just before it. A
//!    [`REFRACTORY_SECONDS`] refractory keeps the stronger of two hits that close.
//! 3. **Tempo** from the autocorrelation of the same novelty (above its local mean, lightly
//!    smoothed) over lags of 60–200 BPM, weighted by a broad prior around 120 BPM so a beat
//!    wins over its half-time twin. The lag is interpolated, then sharpened on its own 2nd–4th
//!    multiples (four periods measured once beat one period measured four times), and the
//!    tempo folded into the 70–160 band producers count in. The normalised autocorrelation
//!    at the period is the confidence.
//! 4. **Key** from a whole-file chroma: the power spectrum averaged over every 4096-point
//!    frame, its peaks that stand [`TONAL_RATIO`] over the local median folded onto pitch
//!    classes (after estimating how far the whole recording sits from A440), matched against
//!    the Krumhansl–Schmuckler profiles by `dissonant-core`'s [`KeyDetector`]. Averaging first
//!    is what makes noise harmless: a hiss or a cymbal wash averages into a smooth spectrum
//!    with no peaks at all, while a held note stays a peak.

use crate::audio::AudioBuffer;
use crate::detect::Fft;
use dissonant_core::{KeyDetector, ScaleType};

/// The rate the source is decimated towards (by an integer factor, so 48 kHz lands on 12 kHz
/// and 44.1 kHz on 11.025 kHz). Onsets and pitch classes both live well under its Nyquist.
const TARGET_RATE: u32 = 11_025;
/// Onset spectrum size at the decimated rate: ~43 ms, enough bins to see a note change.
const ONSET_FFT: usize = 512;
/// Onset hop: ~5 ms, fine enough to interpolate a tempo to a fraction of a BPM.
const ONSET_HOP: usize = 64;
/// Magnitudes are compressed as `ln(1 + LOG_GAIN · |X|)` before differencing, so a quiet hit
/// rising out of silence counts as much as a loud one over a busy mix.
const LOG_GAIN: f32 = 100.0;
/// Mean per-bin log rise under which a frame is no onset whatever its surroundings: dither and
/// reverb tails, not hits.
const NOVELTY_FLOOR: f32 = 0.05;
/// Half-width of the window the adaptive threshold (and the tempo envelope's local mean) is
/// measured over.
const THRESHOLD_SECONDS: f64 = 0.15;
/// How many standard deviations over the local mean novelty an onset must stand.
const ONSET_DEVIATIONS: f32 = 1.0;
/// An onset frame is the largest novelty this far either side of it.
const PEAK_SECONDS: f64 = 0.03;
/// Two onsets closer than this are one hit (a flam, a strum); the stronger is kept.
const REFRACTORY_SECONDS: f64 = 0.05;
/// The most onsets reported: the strongest ones, enough to chop anything a pad can hold.
pub const MAX_ONSETS: usize = 512;
/// Onset times are refined to blocks this long at the decimated rate (~1.3 ms).
const REFINE_BLOCK: usize = 16;
/// A block's energy jump is measured against the mean of this many blocks before it.
const REFINE_HISTORY: usize = 4;
/// Block energies this far (power ratio) under the loudest in the search window are treated
/// as silence, so the jump out of a noise floor is not mistaken for the hit.
const REFINE_FLOOR: f32 = 1e-4;

/// The tempo range searched, as autocorrelation lags.
const MIN_BPM: f64 = 60.0;
const MAX_BPM: f64 = 200.0;
/// The band a found tempo is folded into by octaves.
const FOLD_LO_BPM: f64 = 70.0;
const FOLD_HI_BPM: f64 = 160.0;
/// Centre and width (octaves, one standard deviation) of the log-tempo prior that breaks the
/// tie between a beat and its half-time twin.
const PRIOR_BPM: f64 = 120.0;
const PRIOR_OCTAVES: f64 = 1.0;
/// Standard deviation, in novelty frames, of the Gaussian that smooths the tempo envelope: a
/// hit that lands on frame 80 one bar and 81 the next still correlates with itself.
const SMOOTH_FRAMES: f32 = 2.0;
/// Normalised autocorrelation under which there is no beat to speak of.
const MIN_BPM_CONFIDENCE: f64 = 0.2;
/// Highest period multiple the lag is sharpened on.
const MAX_MULTIPLE: usize = 4;
/// A multiple's peak must keep this share of the period's own to be trusted.
const MULTIPLE_SHARE: f64 = 0.5;

/// Chroma spectrum size at the decimated rate: ~340 ms, 2.9 Hz bins — a semitone apart from
/// the second octave up.
const CHROMA_FFT: usize = 4096;
const CHROMA_HOP: usize = 2048;
/// Spectral peaks are folded from this band: bass fundamentals to the upper partials of a lead.
const CHROMA_LO_HZ: f32 = 65.0;
const CHROMA_HI_HZ: f32 = 2_100.0;
/// Half-width, in bins, of the median the averaged spectrum's floor is taken from.
const FLOOR_BINS: usize = 16;
/// A peak must stand this far (power ratio, ~6 dB) over that floor to be a pitch.
const TONAL_RATIO: f64 = 4.0;
/// Best profile correlation under which no key is named.
const MIN_KEY_CORRELATION: f64 = 0.5;
/// Margin over the runner-up key at which confidence is full.
const KEY_MARGIN_FULL: f64 = 0.2;

/// Everything one pass over a sample heard. Engine-local; the application maps it onto
/// `dissonant_core::SampleAnalysis`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Analysis {
    /// Tempo folded into 70–160 BPM; `None` when nothing in the sample repeats.
    pub bpm: Option<f64>,
    /// 0..1: the novelty's normalised autocorrelation at the beat period. 0 when `bpm` is
    /// `None`.
    pub bpm_confidence: f64,
    /// Tonic pitch class 0..11 (0 = C); `None` for silence or unpitched material.
    pub key_root: Option<i32>,
    /// Minor rather than major. Meaningless when `key_root` is `None` (then `false`).
    pub key_minor: bool,
    /// 0..1: the best key's correlation margin over the runner-up. 0 when `key_root` is `None`.
    pub key_confidence: f64,
    /// `(seconds, strength)`, ascending by time, at most [`MAX_ONSETS`]. Strengths are the
    /// onsets' spectral flux relative to the strongest one, which is 1.
    pub onsets: Vec<(f64, f64)>,
}

/// Analyse a whole source.
pub fn analyze(buffer: &AudioBuffer) -> Analysis {
    let Some(mono) = fold_down(buffer) else {
        return Analysis::default();
    };
    let novelty = novelty(&mono.samples);
    let fps = mono.rate / ONSET_HOP as f64;
    let onsets = onsets(&mono, &novelty, fps);
    let (bpm, bpm_confidence) = match tempo(&novelty, fps) {
        Some((bpm, confidence)) => (Some(bpm), confidence),
        None => (None, 0.0),
    };
    let (key_root, key_minor, key_confidence) = match key(&mono.samples, mono.rate) {
        Some((root, minor, confidence)) => (Some(root), minor, confidence),
        None => (None, false, 0.0),
    };
    Analysis {
        bpm,
        bpm_confidence,
        key_root,
        key_minor,
        key_confidence,
        onsets,
    }
}

/// The source as one decimated channel.
struct Mono {
    samples: Vec<f32>,
    /// Decimated rate.
    rate: f64,
    /// Source frames per decimated sample.
    factor: usize,
    source_rate: f64,
}

impl Mono {
    /// Seconds into the source of (fractional) decimated sample `at`: the centre of the
    /// source frames it averages.
    fn seconds(&self, at: f64) -> f64 {
        (at * self.factor as f64 + (self.factor as f64 - 1.0) * 0.5) / self.source_rate
    }
}

/// Every channel averaged, `factor` frames at a time; non-finite samples read as silence.
/// `None` when there is nothing to analyse.
fn fold_down(buffer: &AudioBuffer) -> Option<Mono> {
    if buffer.sample_rate == 0 {
        return None;
    }
    let ch = buffer.channels.max(1) as usize;
    let frames = buffer.frames.min(buffer.data.len() / ch);
    let factor = (buffer.sample_rate / TARGET_RATE).max(1) as usize;
    let scale = 1.0 / (factor * ch) as f32;
    let samples: Vec<f32> = buffer.data[..frames * ch]
        .chunks_exact(factor * ch)
        .map(|chunk| {
            chunk
                .iter()
                .map(|&x| if x.is_finite() { x } else { 0.0 })
                .sum::<f32>()
                * scale
        })
        .collect();
    if samples.is_empty() {
        return None;
    }
    Some(Mono {
        samples,
        rate: buffer.sample_rate as f64 / factor as f64,
        factor,
        source_rate: buffer.sample_rate as f64,
    })
}

/// First decimated sample of onset frame `f`. Frames start `ONSET_FFT - ONSET_HOP` before the
/// signal (zero-padded), so a hit on the very first sample still rises out of silence.
fn frame_start(f: usize) -> isize {
    (f * ONSET_HOP) as isize - (ONSET_FFT - ONSET_HOP) as isize
}

/// Copy `n` samples of `x` from `start` into `out`, zero outside the signal.
fn fill_frame(x: &[f32], start: isize, out: &mut [f32]) {
    let n = out.len();
    if start >= 0 && start as usize + n <= x.len() {
        out.copy_from_slice(&x[start as usize..start as usize + n]);
        return;
    }
    for (i, slot) in out.iter_mut().enumerate() {
        let at = start + i as isize;
        *slot = if at >= 0 && (at as usize) < x.len() {
            x[at as usize]
        } else {
            0.0
        };
    }
}

/// Spectral flux per onset frame: the mean, over bins, of each bin's rise in log magnitude
/// since the previous frame (falls count as nothing).
fn novelty(x: &[f32]) -> Vec<f32> {
    let count = (x.len() + ONSET_FFT - ONSET_HOP).div_ceil(ONSET_HOP);
    let bins = ONSET_FFT / 2;
    let mut fft = Fft::new(ONSET_FFT);
    let mut frame = vec![0.0f32; ONSET_FFT];
    let mut power = vec![0.0f32; bins + 1];
    let mut prev = vec![0.0f32; bins + 1];
    let mut out = Vec::with_capacity(count);
    for f in 0..count {
        fill_frame(x, frame_start(f), &mut frame);
        fft.power_spectrum(&frame, &mut power);
        let mut flux = 0.0f32;
        for (p, last) in power[1..].iter().zip(prev[1..].iter_mut()) {
            let level = (LOG_GAIN * p.sqrt()).ln_1p();
            flux += (level - *last).max(0.0);
            *last = level;
        }
        out.push(flux / bins as f32);
    }
    out
}

/// Running sums of `v` and `v²`, for window means and deviations in O(1).
fn prefix_sums(v: &[f32]) -> (Vec<f64>, Vec<f64>) {
    let mut sum = Vec::with_capacity(v.len() + 1);
    let mut sq = Vec::with_capacity(v.len() + 1);
    let (mut s, mut q) = (0.0f64, 0.0f64);
    sum.push(0.0);
    sq.push(0.0);
    for &x in v {
        s += x as f64;
        q += x as f64 * x as f64;
        sum.push(s);
        sq.push(q);
    }
    (sum, sq)
}

/// Peak-picked, refined, refractory-filtered, normalised and capped onsets.
fn onsets(mono: &Mono, novelty: &[f32], fps: f64) -> Vec<(f64, f64)> {
    let len = novelty.len();
    let reach = ((THRESHOLD_SECONDS * fps).round() as usize).max(1);
    let peak = ((PEAK_SECONDS * fps).round() as usize).max(1);
    let (sum, sq) = prefix_sums(novelty);
    let mut picked: Vec<(f64, f64)> = Vec::new();
    for (t, &v) in novelty.iter().enumerate() {
        if v <= NOVELTY_FLOOR {
            continue;
        }
        let before = &novelty[t.saturating_sub(peak)..t];
        let after = &novelty[t + 1..(t + 1 + peak).min(len)];
        if before.iter().any(|&u| u >= v) || after.iter().any(|&u| u > v) {
            continue;
        }
        let (lo, hi) = (t.saturating_sub(reach), (t + reach + 1).min(len));
        let n = (hi - lo) as f64;
        let mean = (sum[hi] - sum[lo]) / n;
        let var = ((sq[hi] - sq[lo]) / n - mean * mean).max(0.0);
        if (v as f64) < mean + ONSET_DEVIATIONS as f64 * var.sqrt() {
            continue;
        }
        let at = refine(&mono.samples, frame_start(t));
        picked.push((mono.seconds(at), v as f64));
    }
    picked.sort_by(|a, b| a.0.total_cmp(&b.0));

    let mut kept: Vec<(f64, f64)> = Vec::with_capacity(picked.len());
    for onset in picked {
        match kept.last_mut() {
            Some(last) if onset.0 - last.0 < REFRACTORY_SECONDS => {
                if onset.1 > last.1 {
                    *last = onset;
                }
            }
            _ => kept.push(onset),
        }
    }

    let strongest = kept.iter().map(|o| o.1).fold(0.0f64, f64::max);
    if strongest <= 0.0 {
        return vec![];
    }
    if kept.len() > MAX_ONSETS {
        // Strongest first, earlier first among equals, then back into time order.
        kept.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.total_cmp(&b.0)));
        kept.truncate(MAX_ONSETS);
        kept.sort_by(|a, b| a.0.total_cmp(&b.0));
    }
    for onset in &mut kept {
        onset.1 /= strongest;
    }
    kept
}

/// Where, inside the onset frame starting at `start`, the hit actually is: the centre (in
/// fractional decimated samples) of the block whose log energy rises most over the mean of
/// the [`REFINE_HISTORY`] blocks before it.
fn refine(x: &[f32], start: isize) -> f64 {
    let lo = start.max(0) as usize;
    let hi = ((start + ONSET_FFT as isize).max(0) as usize).min(x.len());
    if hi <= lo {
        return lo as f64;
    }
    let first = lo as isize - (REFINE_HISTORY * REFINE_BLOCK) as isize;
    let blocks = REFINE_HISTORY + (hi - lo).div_ceil(REFINE_BLOCK);
    let energy: Vec<f32> = (0..blocks)
        .map(|b| {
            let from = first + (b * REFINE_BLOCK) as isize;
            (from..from + REFINE_BLOCK as isize)
                .filter(|&i| i >= 0 && (i as usize) < x.len())
                .map(|i| x[i as usize] * x[i as usize])
                .sum()
        })
        .collect();
    let loudest = energy.iter().copied().fold(0.0f32, f32::max);
    if loudest <= 0.0 {
        return lo as f64;
    }
    let eps = loudest * REFINE_FLOOR;
    let mut best = (REFINE_HISTORY, f32::NEG_INFINITY);
    for b in REFINE_HISTORY..blocks {
        let base = energy[b - REFINE_HISTORY..b].iter().sum::<f32>() / REFINE_HISTORY as f32;
        let rise = (energy[b] + eps).ln() - (base + eps).ln();
        if rise > best.1 {
            best = (b, rise);
        }
    }
    let from = first + (best.0 * REFINE_BLOCK) as isize;
    (from as f64 + (REFINE_BLOCK as f64 - 1.0) * 0.5).max(0.0)
}

/// `(bpm folded into 70–160, confidence)`, or `None` when nothing repeats.
fn tempo(novelty: &[f32], fps: f64) -> Option<(f64, f64)> {
    let len = novelty.len();
    let lag_min = ((fps * 60.0 / MAX_BPM).floor() as usize).max(2);
    let lag_max = (fps * 60.0 / MIN_BPM).ceil() as usize;
    if len < lag_min + 2 {
        return None;
    }

    // The envelope: novelty over its local mean, smoothed, then centred.
    let reach = ((THRESHOLD_SECONDS * fps).round() as usize).max(1);
    let (sum, _) = prefix_sums(novelty);
    let rectified: Vec<f32> = (0..len)
        .map(|t| {
            let (lo, hi) = (t.saturating_sub(reach), (t + reach + 1).min(len));
            let mean = (sum[hi] - sum[lo]) / (hi - lo) as f64;
            (novelty[t] as f64 - mean).max(0.0) as f32
        })
        .collect();
    let radius = (3.0 * SMOOTH_FRAMES).ceil() as usize;
    let kernel: Vec<f32> = (0..=2 * radius)
        .map(|i| {
            let d = i as f32 - radius as f32;
            (-0.5 * (d / SMOOTH_FRAMES).powi(2)).exp()
        })
        .collect();
    let mut env: Vec<f64> = (0..len)
        .map(|t| {
            kernel
                .iter()
                .enumerate()
                .filter_map(|(i, &k)| {
                    let at = (t + i).checked_sub(radius)?;
                    rectified.get(at).map(|&v| (v * k) as f64)
                })
                .sum()
        })
        .collect();
    let mean = env.iter().sum::<f64>() / len as f64;
    for v in &mut env {
        *v -= mean;
    }

    let max_lag = (MAX_MULTIPLE * lag_max + MAX_MULTIPLE + 1).min(len - 1);
    let acf: Vec<f64> = (0..=max_lag)
        .map(|lag| {
            env[..len - lag]
                .iter()
                .zip(&env[lag..])
                .map(|(a, b)| a * b)
                .sum()
        })
        .collect();
    if acf[0] <= 1e-12 {
        return None;
    }
    let is_peak =
        |l: usize| l >= 1 && l + 1 < acf.len() && acf[l] >= acf[l - 1] && acf[l] >= acf[l + 1];
    let prior = |lag: f64| {
        let octaves = (60.0 * fps / lag / PRIOR_BPM).log2() / PRIOR_OCTAVES;
        (-0.5 * octaves * octaves).exp()
    };
    let period_lag = (lag_min..=lag_max.min(acf.len().saturating_sub(2)))
        .filter(|&l| is_peak(l) && acf[l] > 0.0)
        .max_by(|&a, &b| (acf[a] * prior(a as f64)).total_cmp(&(acf[b] * prior(b as f64))))?;
    let confidence = (acf[period_lag] / acf[0]).clamp(0.0, 1.0);
    if confidence < MIN_BPM_CONFIDENCE {
        return None;
    }

    let interpolate = |l: usize| {
        let (a, b, c) = (acf[l - 1], acf[l], acf[l + 1]);
        let denom = a - 2.0 * b + c;
        let delta = if denom.abs() > 1e-12 {
            (0.5 * (a - c) / denom).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        l as f64 + delta
    };
    let mut period = interpolate(period_lag);
    for k in 2..=MAX_MULTIPLE {
        let centre = (period * k as f64).round() as usize;
        let lo = centre.saturating_sub(k).max(1);
        let hi = (centre + k).min(acf.len().saturating_sub(2));
        if lo > hi {
            break;
        }
        let Some(l) = (lo..=hi).max_by(|&a, &b| acf[a].total_cmp(&acf[b])) else {
            break;
        };
        if !is_peak(l) || acf[l] < MULTIPLE_SHARE * acf[period_lag] {
            break;
        }
        period = interpolate(l) / k as f64;
    }

    let mut bpm = 60.0 * fps / period;
    while bpm < FOLD_LO_BPM {
        bpm *= 2.0;
    }
    while bpm > FOLD_HI_BPM {
        bpm /= 2.0;
    }
    Some((bpm, confidence))
}

/// `(root pitch class, minor, confidence)` from the whole file's chroma, or `None` when
/// nothing in it is pitched.
fn key(x: &[f32], rate: f64) -> Option<(i32, bool, f64)> {
    let bins = CHROMA_FFT / 2;
    let mut fft = Fft::new(CHROMA_FFT);
    let mut frame = vec![0.0f32; CHROMA_FFT];
    let mut power = vec![0.0f32; bins + 1];
    let mut average = vec![0.0f64; bins + 1];
    let mut start = 0usize;
    while start < x.len() {
        fill_frame(x, start as isize, &mut frame);
        fft.power_spectrum(&frame, &mut power);
        for (sum, &p) in average.iter_mut().zip(&power) {
            *sum += p as f64;
        }
        start += CHROMA_HOP;
    }

    let bin_hz = rate / CHROMA_FFT as f64;
    let lo = ((CHROMA_LO_HZ as f64 / bin_hz).ceil() as usize).max(1);
    let hi = ((CHROMA_HI_HZ as f64).min(rate * 0.45) / bin_hz).floor() as usize;
    let hi = hi.min(bins - 1);
    if lo >= hi {
        return None;
    }

    // Peaks of the averaged spectrum over its local median: `(midi, magnitude)`.
    let mut window = Vec::with_capacity(2 * FLOOR_BINS + 1);
    let mut peaks: Vec<(f64, f64)> = Vec::new();
    for k in lo..=hi {
        let p = average[k];
        if p <= 0.0 || p <= average[k - 1] || p < average[k + 1] {
            continue;
        }
        window.clear();
        window.extend_from_slice(
            &average[k.saturating_sub(FLOOR_BINS).max(1)..=(k + FLOOR_BINS).min(bins)],
        );
        let mid = window.len() / 2;
        let floor = *window.select_nth_unstable_by(mid, f64::total_cmp).1;
        if p <= TONAL_RATIO * floor {
            continue;
        }
        let (a, b, c) = (
            average[k - 1].max(1e-30).ln(),
            p.ln(),
            average[k + 1].max(1e-30).ln(),
        );
        let denom = a - 2.0 * b + c;
        let delta = if denom.abs() > 1e-12 {
            (0.5 * (a - c) / denom).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        let hz = (k as f64 + delta) * bin_hz;
        peaks.push((69.0 + 12.0 * (hz / 440.0).log2(), p.sqrt()));
    }
    if peaks.is_empty() {
        return None;
    }

    // How far the whole recording sits from A440: the magnitude-weighted circular mean of
    // every peak's offset from its nearest semitone.
    let (mut cos, mut sin) = (0.0f64, 0.0f64);
    for &(midi, weight) in &peaks {
        let angle = std::f64::consts::TAU * (midi - midi.round());
        cos += weight * angle.cos();
        sin += weight * angle.sin();
    }
    let tuning = sin.atan2(cos) / std::f64::consts::TAU;

    let mut chroma = [0.0f64; 12];
    for &(midi, weight) in &peaks {
        chroma[((midi - tuning).round() as i64).rem_euclid(12) as usize] += weight;
    }
    let total: f64 = chroma.iter().sum();
    if total <= 0.0 {
        return None;
    }
    // Scaled to 12 "notes" so the detector reads it as evidence; correlation ignores scale.
    let chroma = chroma.map(|c| (12.0 * c / total) as f32);
    let result = KeyDetector::default().detect_with_chroma(&[], &chroma);
    let best = result.candidates.first()?;
    if best.score < MIN_KEY_CORRELATION {
        return None;
    }
    let runner_up = result.candidates.get(1).map_or(0.0, |c| c.score);
    let confidence = ((best.score - runner_up) / KEY_MARGIN_FULL).clamp(0.0, 1.0);
    Some((
        best.root_pitch_class,
        matches!(best.scale, ScaleType::Minor),
        confidence,
    ))
}
