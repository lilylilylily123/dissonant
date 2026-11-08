//! The sampler: one [`crate::AudioSource`] cut into [`Chop`]s that play from MIDI keys.
//!
//! Chop *i* answers pitch `base_pitch + i`, so a sampler track is sequenced from patterns like
//! any synth track. Everything here is a plain description; the engine owns the voices and the
//! Tauri shell owns fetching and decoding. [`Sampler::sanitized`] is the one place a chop's
//! numbers are held in range, so every door (commands, file load) agrees on what a chop can be.

use crate::model::ScaleType;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Most chops one sampler holds. 64 keys is more than a pad grid shows and keeps
/// `base_pitch + i` comfortably inside MIDI.
pub const MAX_CHOPS: usize = 64;
/// Shortest chop, in seconds. Anything shorter is a click, not a sound.
pub const MIN_CHOP_SECONDS: f64 = 0.01;
/// The key chop 0 plays on a new sampler: C1, where pad controllers start.
pub const DEFAULT_SAMPLER_BASE_PITCH: u8 = 36;
/// Loudest a chop may be boosted, linear. The same ceiling as an audio clip's gain.
pub const MAX_CHOP_GAIN: f64 = 4.0;
/// Longest attack ramp, in seconds.
pub const MAX_CHOP_ATTACK_SECONDS: f64 = 2.0;
/// Longest release fade, in seconds.
pub const MAX_CHOP_RELEASE_SECONDS: f64 = 5.0;
/// A new chop's release: just long enough that letting go of a key never clicks.
pub const DEFAULT_CHOP_RELEASE_SECONDS: f64 = 0.01;
/// Highest choke group. 0 means "chokes nothing".
pub const MAX_CHOKE_GROUP: u8 = 16;
/// Longest source a sampler will take, in seconds. A source is held in RAM as f32, so a whole
/// album would cost gigabytes.
pub const MAX_SAMPLE_SECONDS: f64 = 600.0;
/// Most onsets an analysis keeps: the strongest, by [`Onset::strength`].
pub const MAX_ONSETS: usize = 512;
/// How strong an onset must be to start a suggested region: the first hit at least half as
/// loud as the loudest is the downbeat, so a quiet pre-roll or a breath is skipped.
pub const STRONG_ONSET: f64 = 0.5;
/// Bars in a suggested region when the source has a tempo: a loop-sized phrase.
pub const SUGGESTED_REGION_BARS: u32 = 4;
/// A suggested region's length, seconds, when the source has no tempo.
pub const SUGGESTED_REGION_SECONDS: f64 = 8.0;
/// Chops a fresh link is cut into: a 4 × 4 pad page.
pub const DEFAULT_CHOP_COUNT: u32 = 16;

fn default_base_pitch() -> u8 {
    DEFAULT_SAMPLER_BASE_PITCH
}
fn one() -> f64 {
    1.0
}
fn default_release() -> f64 {
    DEFAULT_CHOP_RELEASE_SECONDS
}

/// A sampler track's instrument: which source it cuts up, where its keys start, and the chops
/// in key order. Lives on [`crate::Track::sampler`]; a track with one is a sampler track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sampler {
    /// The [`crate::AudioSource`] every chop reads from.
    pub source_id: Uuid,
    /// The MIDI pitch chop 0 plays on; chop *i* plays on `base_pitch + i`.
    #[serde(default = "default_base_pitch")]
    pub base_pitch: u8,
    /// In key order: reordering chops remaps the keys, never the audio.
    #[serde(default)]
    pub chops: Vec<Chop>,
    /// What analysis heard in the source, kept so the SAMPLE screen can offer the tempo, key
    /// and onset grid again without re-analysing. `None` until analysed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis: Option<SampleAnalysis>,
    /// Whether a hit cuts everything sounding (mono) or layers (poly). Files from before this
    /// field load as mono, the default.
    #[serde(default)]
    pub voice: SamplerVoice,
    /// The stretch of the source the chop tools cut inside. `None` means the whole source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<SampleRegion>,
    /// The chop tool used last, so moving the region can cut it again the same way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chopper: Option<Chopper>,
}

/// How many chops a sampler sounds at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SamplerVoice {
    /// Any new hit cuts everything the track is sounding, ignoring choke groups: chops cut
    /// from one song never pile up into mud.
    #[default]
    Mono,
    /// Hits layer; only the same chop again or a shared choke group cuts.
    Poly,
}

/// A stretch of the source, seconds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleRegion {
    pub start_seconds: f64,
    pub end_seconds: f64,
}

/// A chop tool, applied inside a [`SampleRegion`] by [`chop_region`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Chopper {
    /// At most `count` chops: the region's start plus its strongest onsets.
    Auto { count: u32 },
    /// `count` chops of equal length.
    Equal { count: u32 },
    /// One chop every `step` of the detected tempo, from the region's start.
    Grid { step: GridStep },
}

/// How long one grid chop is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GridStep {
    Bar,
    Half,
    Beat,
}

/// How a chop answers note-off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ChopMode {
    /// Plays to the chop's end whatever the key does: drum hits and stabs.
    #[default]
    OneShot,
    /// Plays while the key is held, then releases: pads and held vocals.
    Gate,
}

/// One slice of the source, and how it plays.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chop {
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,
    /// What the pad says. Empty shows the key name instead.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Where the chop starts in the source, seconds.
    pub start_seconds: f64,
    /// Where it ends, seconds; at least [`MIN_CHOP_SECONDS`] after `start_seconds`.
    pub end_seconds: f64,
    /// Linear, 0…[`MAX_CHOP_GAIN`].
    #[serde(default = "one")]
    pub gain: f64,
    /// Plays end to start.
    #[serde(default)]
    pub reverse: bool,
    #[serde(default)]
    pub mode: ChopMode,
    /// Linear fade-in, 0…[`MAX_CHOP_ATTACK_SECONDS`].
    #[serde(default)]
    pub attack_seconds: f64,
    /// Fade-out at note-off (gate) or the chop's end, 0…[`MAX_CHOP_RELEASE_SECONDS`].
    #[serde(default = "default_release")]
    pub release_seconds: f64,
    /// Choke group: a hit in the same non-zero group cuts this chop off (an open hat closed by
    /// a closed one). 0 = none, 1…[`MAX_CHOKE_GROUP`].
    #[serde(default)]
    pub choke: u8,
}

/// What analysis heard in a source: a tempo, a key, and where the hits are.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SampleAnalysis {
    /// `None` when no tempo stood out.
    #[serde(default)]
    pub bpm: Option<f64>,
    /// 0…1.
    #[serde(default)]
    pub bpm_confidence: f64,
    /// Pitch class of the tonic, 0 = C … 11 = B. `None` when no key stood out.
    #[serde(default)]
    pub key_root: Option<i32>,
    #[serde(default)]
    pub key_scale: ScaleType,
    /// 0…1.
    #[serde(default)]
    pub key_confidence: f64,
    /// Ascending by `seconds`, at most [`MAX_ONSETS`]. The raw material for chop grids.
    #[serde(default)]
    pub onsets: Vec<Onset>,
}

/// One detected hit in the source.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Onset {
    pub seconds: f64,
    /// Normalised 0…1 against the source's strongest onset.
    pub strength: f64,
}

/// `v` inside `[lo, hi]`, or `fallback` when it is not a number at all.
fn ranged(v: f64, lo: f64, hi: f64, fallback: f64) -> f64 {
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        fallback
    }
}

impl Chop {
    /// A chop over `[start_seconds, end_seconds)` with every other field at its default.
    pub fn new(start_seconds: f64, end_seconds: f64) -> Self {
        Chop {
            id: Uuid::new_v4(),
            name: String::new(),
            start_seconds,
            end_seconds,
            gain: 1.0,
            reverse: false,
            mode: ChopMode::OneShot,
            attack_seconds: 0.0,
            release_seconds: DEFAULT_CHOP_RELEASE_SECONDS,
            choke: 0,
        }
    }

    /// Every field in range against a source `duration` seconds long, or `None` when the chop
    /// cannot fit in it (no room for [`MIN_CHOP_SECONDS`] after its start, or a bound that is
    /// not a number).
    fn sanitized(&self, duration: f64) -> Option<Chop> {
        if !(self.start_seconds.is_finite() && self.end_seconds.is_finite()) {
            return None;
        }
        let start = self.start_seconds.clamp(0.0, duration);
        if start + MIN_CHOP_SECONDS > duration {
            return None;
        }
        Some(Chop {
            id: self.id,
            name: self.name.clone(),
            start_seconds: start,
            end_seconds: self.end_seconds.clamp(start + MIN_CHOP_SECONDS, duration),
            gain: ranged(self.gain, 0.0, MAX_CHOP_GAIN, 1.0),
            reverse: self.reverse,
            mode: self.mode,
            attack_seconds: ranged(self.attack_seconds, 0.0, MAX_CHOP_ATTACK_SECONDS, 0.0),
            release_seconds: ranged(
                self.release_seconds,
                0.0,
                MAX_CHOP_RELEASE_SECONDS,
                DEFAULT_CHOP_RELEASE_SECONDS,
            ),
            choke: self.choke.min(MAX_CHOKE_GROUP),
        })
    }
}

impl SampleAnalysis {
    /// Confidences in 0…1, a key root that is a pitch class, a tempo that is a positive number,
    /// and onsets inside a source `duration` seconds long: ascending, strengths in 0…1, at most
    /// [`MAX_ONSETS`] (the strongest).
    fn sanitized(&self, duration: f64) -> SampleAnalysis {
        let mut onsets: Vec<Onset> = self
            .onsets
            .iter()
            .filter(|o| o.seconds.is_finite() && o.strength.is_finite())
            .filter(|o| o.seconds >= 0.0 && o.seconds <= duration)
            .map(|o| Onset {
                seconds: o.seconds,
                strength: o.strength.clamp(0.0, 1.0),
            })
            .collect();
        if onsets.len() > MAX_ONSETS {
            sort_strongest_first(&mut onsets);
            onsets.truncate(MAX_ONSETS);
        }
        onsets.sort_by(|a, b| a.seconds.total_cmp(&b.seconds));
        SampleAnalysis {
            bpm: self.bpm.filter(|b| b.is_finite() && *b > 0.0),
            bpm_confidence: ranged(self.bpm_confidence, 0.0, 1.0, 0.0),
            key_root: self.key_root.filter(|r| (0..12).contains(r)),
            key_scale: self.key_scale,
            key_confidence: ranged(self.key_confidence, 0.0, 1.0, 0.0),
            onsets,
        }
    }
}

impl Sampler {
    /// A sampler over `source_id` with `chops`, starting at [`DEFAULT_SAMPLER_BASE_PITCH`].
    pub fn new(source_id: Uuid, chops: Vec<Chop>) -> Self {
        Sampler {
            source_id,
            base_pitch: DEFAULT_SAMPLER_BASE_PITCH,
            chops,
            analysis: None,
            voice: SamplerVoice::default(),
            region: None,
            chopper: None,
        }
    }

    /// Every chop held inside a source `source_seconds` long and its own ranges, in the same
    /// order; a chop that cannot fit is dropped. `base_pitch` comes down far enough that the
    /// last chop still has a MIDI key. The region is held inside the source (`None` when less
    /// than [`MIN_CHOP_SECONDS`] of it is left) and the chopper's count in 1…[`MAX_CHOPS`].
    /// `None` when there are more than [`MAX_CHOPS`] chops: silently dropping the tail would
    /// lose work the user can see.
    pub fn sanitized(&self, source_seconds: f64) -> Option<Sampler> {
        if self.chops.len() > MAX_CHOPS {
            return None;
        }
        let duration = ranged(source_seconds, 0.0, f64::MAX, 0.0);
        let chops: Vec<Chop> = self
            .chops
            .iter()
            .filter_map(|c| c.sanitized(duration))
            .collect();
        // At most 64 chops, so this never underflows.
        let top = 127 - chops.len().saturating_sub(1) as u8;
        Some(Sampler {
            source_id: self.source_id,
            base_pitch: self.base_pitch.min(top),
            chops,
            analysis: self.analysis.as_ref().map(|a| a.sanitized(duration)),
            voice: self.voice,
            region: self.region.and_then(|r| r.sanitized(duration)),
            chopper: self.chopper.map(|c| c.sanitized()),
        })
    }

    /// The chop pitch `pitch` plays, if any.
    pub fn chop_for(&self, pitch: u8) -> Option<&Chop> {
        pitch
            .checked_sub(self.base_pitch)
            .and_then(|i| self.chops.get(i as usize))
    }
}

impl SampleRegion {
    /// Held inside a source `duration` seconds long, or `None` when less than
    /// [`MIN_CHOP_SECONDS`] of it is left (or a bound is not a number): too short to cut.
    fn sanitized(self, duration: f64) -> Option<SampleRegion> {
        if !(self.start_seconds.is_finite() && self.end_seconds.is_finite()) {
            return None;
        }
        let start = self.start_seconds.clamp(0.0, duration);
        let end = self.end_seconds.clamp(0.0, duration);
        (end - start >= MIN_CHOP_SECONDS).then_some(SampleRegion {
            start_seconds: start,
            end_seconds: end,
        })
    }
}

/// `count` as a chop count a sampler can hold: 1…[`MAX_CHOPS`].
fn chop_count(count: u32) -> usize {
    (count as usize).clamp(1, MAX_CHOPS)
}

impl Chopper {
    /// The same tool with its count in 1…[`MAX_CHOPS`].
    fn sanitized(self) -> Chopper {
        match self {
            Chopper::Auto { count } => Chopper::Auto {
                count: chop_count(count) as u32,
            },
            Chopper::Equal { count } => Chopper::Equal {
                count: chop_count(count) as u32,
            },
            grid @ Chopper::Grid { .. } => grid,
        }
    }
}

/// Strongest first; among equals, the earlier hit first, so the order never depends on how the
/// input happened to be arranged.
fn sort_strongest_first(onsets: &mut [Onset]) {
    onsets.sort_by(|a, b| {
        b.strength
            .total_cmp(&a.strength)
            .then(a.seconds.total_cmp(&b.seconds))
    });
}

/// Chops between consecutive cut points over a source `duration` seconds long, the last one
/// ending at `duration`. 0 is always a cut point; points outside the source or not a number are
/// ignored, and a point closer than [`MIN_CHOP_SECONDS`] to the one before it (or to the end)
/// is dropped, so no chop comes out shorter than that. At most [`MAX_CHOPS`] chops: past that,
/// the last chop runs to the end. Every field but the bounds is at its default.
pub fn chops_at(points: &[f64], duration: f64) -> Vec<Chop> {
    chops_between(points, 0.0, duration)
}

/// [`chops_at`] over `[start, end)` instead of the whole source: `start` is always a cut, the
/// last chop ends at `end`, and points outside are ignored.
fn chops_between(points: &[f64], start: f64, end: f64) -> Vec<Chop> {
    if !(start.is_finite() && end.is_finite()) || end - start < MIN_CHOP_SECONDS {
        return vec![];
    }
    let mut sorted: Vec<f64> = points
        .iter()
        .copied()
        .filter(|p| p.is_finite() && *p > start && *p < end)
        .collect();
    sorted.sort_by(f64::total_cmp);
    let mut cuts = vec![start];
    for p in sorted {
        if cuts.len() == MAX_CHOPS {
            break;
        }
        let last = cuts[cuts.len() - 1];
        if p - last >= MIN_CHOP_SECONDS && end - p >= MIN_CHOP_SECONDS {
            cuts.push(p);
        }
    }
    let mut chops = Vec::with_capacity(cuts.len());
    for (i, &from) in cuts.iter().enumerate() {
        let to = cuts.get(i + 1).copied().unwrap_or(end);
        chops.push(Chop::new(from, to));
    }
    chops
}

/// Where a fresh link's chops go: from the first strong hit (strength at least
/// [`STRONG_ONSET`], else the source's start) for [`SUGGESTED_REGION_BARS`] bars of
/// `beats_per_bar` at the detected tempo, or [`SUGGESTED_REGION_SECONDS`] without one. Held
/// inside a source `duration` seconds long; when a hit too near the end leaves less than
/// [`MIN_CHOP_SECONDS`], the region slides back to keep its length instead.
pub fn suggested_region(analysis: &SampleAnalysis, duration: f64, beats_per_bar: u32) -> SampleRegion {
    let duration = ranged(duration, 0.0, f64::MAX, 0.0);
    let downbeat = analysis
        .onsets
        .iter()
        .filter(|o| o.seconds.is_finite() && o.strength >= STRONG_ONSET)
        .map(|o| o.seconds)
        .fold(f64::INFINITY, f64::min);
    let downbeat = if downbeat.is_finite() { downbeat.clamp(0.0, duration) } else { 0.0 };
    let length = match analysis.bpm.filter(|b| b.is_finite() && *b > 0.0) {
        Some(bpm) => f64::from(SUGGESTED_REGION_BARS) * f64::from(beats_per_bar.max(1)) * 60.0 / bpm,
        None => SUGGESTED_REGION_SECONDS,
    };
    let end = (downbeat + length).min(duration);
    let start = if end - downbeat >= MIN_CHOP_SECONDS {
        downbeat
    } else {
        (end - length).max(0.0)
    };
    SampleRegion {
        start_seconds: start,
        end_seconds: end,
    }
}

/// What `chopper` cuts `region` into. Every chop starts at the region's start or a cut inside
/// it and the last ends at its end; nothing is shorter than [`MIN_CHOP_SECONDS`].
///
/// - **Auto**: at most `count` chops — the region's start plus the `count - 1` strongest
///   onsets inside it, so a region that starts on a hit does not spend a chop on it twice.
/// - **Equal**: `count` chops of equal length.
/// - **Grid**: a cut every bar (of `beats_per_bar`), half bar or beat at the analysis tempo,
///   counted from the region's start. No tempo gives no chops: the bar tools are disabled then.
///
/// Counts are held in 1…[`MAX_CHOPS`]; a region shorter than [`MIN_CHOP_SECONDS`] gives none.
pub fn chop_region(
    chopper: &Chopper,
    region: &SampleRegion,
    analysis: Option<&SampleAnalysis>,
    beats_per_bar: u32,
) -> Vec<Chop> {
    let (start, end) = (region.start_seconds, region.end_seconds);
    if !(start.is_finite() && end.is_finite()) || end - start < MIN_CHOP_SECONDS {
        return vec![];
    }
    match *chopper {
        Chopper::Auto { count } => {
            let inside: Vec<Onset> = analysis
                .map_or(&[][..], |a| &a.onsets)
                .iter()
                .copied()
                .filter(|o| o.seconds - start >= MIN_CHOP_SECONDS && end - o.seconds >= MIN_CHOP_SECONDS)
                .collect();
            chops_between(&strongest_onsets(&inside, chop_count(count) - 1), start, end)
        }
        Chopper::Equal { count } => {
            let n = chop_count(count);
            let length = end - start;
            let points: Vec<f64> = (1..n).map(|i| start + i as f64 * length / n as f64).collect();
            chops_between(&points, start, end)
        }
        Chopper::Grid { step } => {
            let Some(bpm) = analysis.and_then(|a| a.bpm).filter(|b| b.is_finite() && *b > 0.0) else {
                return vec![];
            };
            let bar = f64::from(beats_per_bar.max(1));
            let beats = match step {
                GridStep::Bar => bar,
                GridStep::Half => bar / 2.0,
                GridStep::Beat => 1.0,
            };
            let period = beats * 60.0 / bpm;
            if period < MIN_CHOP_SECONDS {
                return vec![];
            }
            let points: Vec<f64> = (1..MAX_CHOPS)
                .map(|k| start + k as f64 * period)
                .take_while(|t| *t < end)
                .collect();
            chops_between(&points, start, end)
        }
    }
}

/// `fresh` cuts carrying the settings of `old`: chop *i* keeps the name, gain, reverse, mode,
/// attack, release and choke of old chop *i* where there is one, so moving a region re-cuts
/// the audio without undoing the shaping. Chops past the end of `old` stay at their defaults.
pub fn rechop(old: &[Chop], fresh: Vec<Chop>) -> Vec<Chop> {
    fresh
        .into_iter()
        .enumerate()
        .map(|(i, c)| match old.get(i) {
            Some(o) => Chop {
                name: o.name.clone(),
                gain: o.gain,
                reverse: o.reverse,
                mode: o.mode,
                attack_seconds: o.attack_seconds,
                release_seconds: o.release_seconds,
                choke: o.choke,
                ..c
            },
            None => c,
        })
        .collect()
}

/// The `count` strongest onset times, ascending. Ties go to the earlier hit; onsets that are
/// not numbers are skipped.
pub fn strongest_onsets(onsets: &[Onset], count: usize) -> Vec<f64> {
    let mut picked: Vec<Onset> = onsets
        .iter()
        .copied()
        .filter(|o| o.seconds.is_finite() && o.strength.is_finite())
        .collect();
    sort_strongest_first(&mut picked);
    picked.truncate(count);
    let mut times: Vec<f64> = picked.into_iter().map(|o| o.seconds).collect();
    times.sort_by(f64::total_cmp);
    times
}
