//! The stock effects, as data: what they are called, what they can be set to, and what they
//! are set to when nobody has touched them.
//!
//! A fresh machine has no plugins, so an insert chain that can only hold a CLAP effect can hold
//! nothing at all. These six close that gap. They live here rather than in the engine because
//! three layers need the same table and none of them may disagree about it: the document stores
//! the values, the engine's `effects.rs` renders them, and the UI draws a knob per entry.
//!
//! # How a native effect sits next to a plugin in the model
//!
//! An [`crate::Insert`] is still one struct, not an enum: a native effect is a slot whose `uid`
//! is `native:<slug>` instead of a plugin uid, with its parameter values in `Insert::params`.
//! The alternative — an enum with a plugin variant and a native variant — would have forked
//! every place that already carries a chain around (sanitising, reordering, the host's
//! reconcile, the render rig, the UI's rows) for a distinction only two of them care about, and
//! would have needed a schema bump to serialize. The uid is already the "what effect is this"
//! key everywhere, and it is already opaque to this crate; `native:` simply reserves a corner
//! of it. Plugin uids are bundle paths plus a plugin id and cannot collide with that prefix.
//!
//! # Parameter ids
//!
//! Ids are `u32` and stable forever, because [`crate::automation::AutoTarget::Insert`] stores
//! one in a project file. Each effect owns a block of a hundred — EQ 100-, compressor 200- and
//! so on — so an id names its own effect on sight and a lane left over from a slot that was
//! replaced by a different effect is recognisably stale rather than silently landing on some
//! other knob. Append within a block; never renumber, never reuse.

use serde::{Deserialize, Serialize};

/// One stock effect. The ordering is the order the UI offers them in, which is the order a mix
/// is usually built in rather than alphabetical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeKind {
    Eq,
    Compressor,
    Limiter,
    Saturator,
    Delay,
    Reverb,
}

/// The prefix that tells a native effect's uid from a plugin's.
pub const NATIVE_PREFIX: &str = "native:";

impl NativeKind {
    pub const ALL: [NativeKind; 6] = [
        NativeKind::Eq,
        NativeKind::Compressor,
        NativeKind::Limiter,
        NativeKind::Saturator,
        NativeKind::Delay,
        NativeKind::Reverb,
    ];

    /// The stable half of the uid, after `native:`. Part of the file format.
    pub fn slug(self) -> &'static str {
        match self {
            NativeKind::Eq => "eq",
            NativeKind::Compressor => "compressor",
            NativeKind::Limiter => "limiter",
            NativeKind::Saturator => "saturator",
            NativeKind::Delay => "delay",
            NativeKind::Reverb => "reverb",
        }
    }

    /// The uid an [`crate::Insert`] of this effect carries.
    pub fn uid(self) -> String {
        format!("{NATIVE_PREFIX}{}", self.slug())
    }

    /// What the chain row calls it.
    pub fn display_name(self) -> &'static str {
        match self {
            NativeKind::Eq => "EQ",
            NativeKind::Compressor => "compressor",
            NativeKind::Limiter => "limiter",
            NativeKind::Saturator => "saturator",
            NativeKind::Delay => "delay",
            NativeKind::Reverb => "reverb",
        }
    }

    /// One line about what reaching for it does, for the picker.
    pub fn blurb(self) -> &'static str {
        match self {
            NativeKind::Eq => "cut, shelve and sweep",
            NativeKind::Compressor => "hold the level down",
            NativeKind::Limiter => "a hard ceiling, lookahead",
            NativeKind::Saturator => "tube warmth and grit",
            NativeKind::Delay => "repeats, in time",
            NativeKind::Reverb => "space around it",
        }
    }

    /// The effect a uid names, or `None` for a plugin uid.
    pub fn from_uid(uid: &str) -> Option<NativeKind> {
        let slug = uid.strip_prefix(NATIVE_PREFIX)?;
        NativeKind::ALL.into_iter().find(|k| k.slug() == slug)
    }

    /// The first id of this effect's block of a hundred.
    pub fn base_id(self) -> u32 {
        match self {
            NativeKind::Eq => 100,
            NativeKind::Compressor => 200,
            NativeKind::Limiter => 300,
            NativeKind::Saturator => 400,
            NativeKind::Delay => 500,
            NativeKind::Reverb => 600,
        }
    }

    /// Every parameter, in the order the UI lays them out.
    pub fn params(self) -> &'static [NativeParam] {
        match self {
            NativeKind::Eq => EQ_PARAMS,
            NativeKind::Compressor => COMPRESSOR_PARAMS,
            NativeKind::Limiter => LIMITER_PARAMS,
            NativeKind::Saturator => SATURATOR_PARAMS,
            NativeKind::Delay => DELAY_PARAMS,
            NativeKind::Reverb => REVERB_PARAMS,
        }
    }

    pub fn param(self, id: u32) -> Option<&'static NativeParam> {
        self.params().iter().find(|p| p.id == id)
    }

    /// What `id` is set to, given a slot's stored values: the stored one held inside its range,
    /// or the default when nothing was ever written. The one place a value is read, so a file
    /// hand-edited out of range cannot reach the DSP.
    pub fn value_of(self, params: &ParamMap, id: u32) -> f64 {
        match self.param(id) {
            Some(p) => params.get(&id).map_or(p.default, |v| p.clamp(*v)),
            None => 0.0,
        }
    }
}

/// Stored parameter values, keyed by [`NativeParam::id`]. Ordered so a saved file is stable.
pub type ParamMap = std::collections::BTreeMap<u32, f64>;

/// One knob on one native effect.
///
/// The shape deliberately matches `dissonant_plugins::ParamInfo`, so an insert's controls are
/// fetched and drawn by exactly the same UI whether the effect behind them is native or hosted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeParam {
    pub id: u32,
    /// Includes the unit (`"attack ms"`), which is the only unit information a knob ever gets
    /// — a CLAP parameter carries none either.
    pub name: &'static str,
    /// The group heading the knob sits under. Groups are drawn in first-appearance order.
    pub module: &'static str,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    /// Integral values only: a choice, not a continuum.
    pub stepped: bool,
}

impl NativeParam {
    /// Hold a value inside the knob's range, rounding a stepped one. A non-finite value is not
    /// a position on the knob at all, so it falls back to the default.
    pub fn clamp(&self, value: f64) -> f64 {
        if !value.is_finite() {
            return self.default;
        }
        let v = value.clamp(self.min, self.max);
        if self.stepped {
            v.round()
        } else {
            v
        }
    }
}

const fn p(
    id: u32,
    name: &'static str,
    module: &'static str,
    min: f64,
    max: f64,
    default: f64,
) -> NativeParam {
    NativeParam {
        id,
        name,
        module,
        min,
        max,
        default,
        stepped: false,
    }
}

const fn stepped(
    id: u32,
    name: &'static str,
    module: &'static str,
    min: f64,
    max: f64,
    default: f64,
) -> NativeParam {
    NativeParam {
        id,
        name,
        module,
        min,
        max,
        default,
        stepped: true,
    }
}

/// High-pass, low shelf, three sweepable peaks, high shelf, low-pass.
///
/// Both cuts default to the end of their range that does nothing, and every gain defaults to
/// zero, so a freshly added EQ is audibly a wire until a knob moves.
pub const EQ_PARAMS: &[NativeParam] = &[
    p(100, "high-pass hz", "cut", 20.0, 2_000.0, 20.0),
    p(114, "low-pass hz", "cut", 500.0, 20_000.0, 20_000.0),
    p(101, "low shelf hz", "low", 40.0, 1_000.0, 120.0),
    p(102, "low shelf dB", "low", -18.0, 18.0, 0.0),
    p(103, "band 1 hz", "band 1", 40.0, 18_000.0, 220.0),
    p(104, "band 1 dB", "band 1", -18.0, 18.0, 0.0),
    p(105, "band 1 q", "band 1", 0.3, 6.0, 1.0),
    p(106, "band 2 hz", "band 2", 40.0, 18_000.0, 1_200.0),
    p(107, "band 2 dB", "band 2", -18.0, 18.0, 0.0),
    p(108, "band 2 q", "band 2", 0.3, 6.0, 1.0),
    p(109, "band 3 hz", "band 3", 40.0, 18_000.0, 5_000.0),
    p(110, "band 3 dB", "band 3", -18.0, 18.0, 0.0),
    p(111, "band 3 q", "band 3", 0.3, 6.0, 1.0),
    p(112, "high shelf hz", "high", 1_000.0, 18_000.0, 6_000.0),
    p(113, "high shelf dB", "high", -18.0, 18.0, 0.0),
];

/// Threshold, ratio, the two times, the knee and makeup gain. The knee is a parameter rather
/// than the rig's fixed 6 dB because a drum bus and a vocal want opposite ends of it.
pub const COMPRESSOR_PARAMS: &[NativeParam] = &[
    p(200, "threshold dB", "gain", -60.0, 0.0, -18.0),
    p(201, "ratio", "gain", 1.0, 20.0, 4.0),
    p(204, "knee dB", "gain", 0.0, 18.0, 6.0),
    p(205, "makeup dB", "gain", -12.0, 24.0, 0.0),
    p(202, "attack ms", "time", 0.1, 50.0, 10.0),
    p(203, "release ms", "time", 20.0, 800.0, 150.0),
];

/// Drive into a ceiling that is guaranteed, not asked for. The same lookahead design the master
/// bus runs, reachable anywhere in a chain.
pub const LIMITER_PARAMS: &[NativeParam] = &[
    p(301, "drive dB", "limit", 0.0, 24.0, 0.0),
    p(300, "ceiling dB", "limit", -24.0, 0.0, -0.3),
    p(302, "release ms", "limit", 10.0, 500.0, 60.0),
];

/// A 12AX7 gain stage, oversampled. `bias` is what makes it a tube and not a clipper: offset
/// from the operating point is where the even harmonics come from.
pub const SATURATOR_PARAMS: &[NativeParam] = &[
    p(400, "drive", "tube", 1.0, 20.0, 3.0),
    p(401, "bias", "tube", -1.0, 1.0, -0.2),
    p(402, "emphasis hz", "tube", 0.0, 4_000.0, 0.0),
    p(403, "mix", "out", 0.0, 1.0, 1.0),
    p(404, "output dB", "out", -24.0, 12.0, 0.0),
];

/// `delay 1/x` is the note value the repeats land on against the song's own tempo: 4 is a
/// quarter note, 8 an eighth, 1 a whole bar of 4/4. A number rather than a list of labels
/// because a knob's readout is a number, and "8" under "delay 1/x" reads as 1/8 without one.
pub const DELAY_PARAMS: &[NativeParam] = &[
    stepped(500, "delay 1/x", "time", 1.0, 32.0, 8.0),
    p(501, "feedback", "repeats", 0.0, 0.95, 0.35),
    p(502, "mix", "repeats", 0.0, 1.0, 0.3),
];

/// Mix, tail length and a damping cut into the tank, plus a pre-delay — the gap before the
/// first reflection, which is what keeps a reverb off the front of the sound.
pub const REVERB_PARAMS: &[NativeParam] = &[
    p(600, "mix", "space", 0.0, 1.0, 0.25),
    p(601, "decay", "space", 0.0, 1.0, 0.5),
    p(602, "pre-delay ms", "space", 0.0, 200.0, 20.0),
    p(603, "low-pass hz", "tone", 1_000.0, 20_000.0, 12_000.0),
];

/// Drop values no parameter of this effect owns and hold the rest inside their ranges. Run when
/// a project is opened, so nothing downstream has to guard a stored value again.
pub fn sanitize_params(kind: NativeKind, params: &mut ParamMap) {
    params.retain(|id, value| match kind.param(*id) {
        Some(p) => {
            *value = p.clamp(*value);
            true
        }
        None => false,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uids_round_trip_and_plugin_uids_are_not_native() {
        for kind in NativeKind::ALL {
            assert_eq!(NativeKind::from_uid(&kind.uid()), Some(kind));
        }
        assert_eq!(NativeKind::from_uid("/Library/Audio/CLAP/x.clap:comp"), None);
        assert_eq!(NativeKind::from_uid("native:nope"), None);
    }

    #[test]
    fn param_ids_are_unique_and_inside_their_effects_block() {
        let mut seen: Vec<u32> = Vec::new();
        for kind in NativeKind::ALL {
            let base = kind.base_id();
            for param in kind.params() {
                assert!(
                    (base..base + 100).contains(&param.id),
                    "{} param {} is outside its block",
                    kind.slug(),
                    param.id
                );
                assert!(!seen.contains(&param.id), "duplicate param id {}", param.id);
                assert!(param.min < param.max);
                assert!((param.min..=param.max).contains(&param.default));
                seen.push(param.id);
            }
        }
    }

    #[test]
    fn values_fall_back_to_the_default_and_are_held_in_range() {
        let kind = NativeKind::Compressor;
        let mut params = ParamMap::new();
        assert_eq!(kind.value_of(&params, 201), 4.0);
        params.insert(201, 900.0);
        assert_eq!(kind.value_of(&params, 201), 20.0);
        params.insert(201, f64::NAN);
        assert_eq!(kind.value_of(&params, 201), 4.0);
    }

    #[test]
    fn sanitize_drops_foreign_ids_and_rounds_stepped_values() {
        let mut params = ParamMap::from([(500, 7.6), (200, -12.0), (999, 1.0)]);
        sanitize_params(NativeKind::Delay, &mut params);
        assert_eq!(params, ParamMap::from([(500, 8.0)]));
    }
}
