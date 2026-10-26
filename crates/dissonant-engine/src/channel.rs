//! A track channel (instrument → tone → inserts → reverb/gain/pan) and the master chain
//! (master inserts → low-cut → 3-band EQ → high-cut → reverb → gain).

use crate::audio::{AudioLane, BlockContext};
use crate::drums::DrumKit;
use crate::dsp::{linear_to_db, pan_gains, Reverb, StereoBiquad};
use crate::engine::NoteShape;
use crate::synth::{PolySynth, VoicePreset};
use dissonant_core::{MasterSettings, SequenceTrack};
use dissonant_plugins::PluginAudioUnit;
use uuid::Uuid;

/// What makes a track's sound. The built-in voices place themselves in stereo from each note's
/// own pan; a plugin renders its own stereo, so the channel feeds it the track pan rather than
/// applying one.
pub enum Instrument {
    Synth(PolySynth),
    Drums(DrumKit),
    /// A hosted plugin. Boxed because it is an order of magnitude larger than the other two and
    /// every `Channel` would otherwise carry that size whether or not it hosts anything.
    Plugin(Box<PluginAudioUnit>),
    /// An audio track has no note source: its sound comes from its [`AudioLane`]. Every note
    /// call is a no-op and it renders nothing of its own.
    Silent,
}

impl std::fmt::Debug for Instrument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Instrument::Synth(s) => f.debug_tuple("Synth").field(s).finish(),
            Instrument::Drums(d) => f.debug_tuple("Drums").field(d).finish(),
            // A plugin's innards belong to the plugin; naming it is all a log line needs.
            Instrument::Plugin(_) => f.write_str("Plugin(..)"),
            Instrument::Silent => f.write_str("Silent"),
        }
    }
}

impl Instrument {
    pub fn note_on(&mut self, pitch: u8, velocity: u8, shape: NoteShape) {
        match self {
            Instrument::Synth(s) => s.note_on(pitch, velocity, shape),
            Instrument::Drums(d) => d.note_on(pitch, velocity, shape),
            // A plugin owns its own expression: MIDI is all the contract there is.
            Instrument::Plugin(p) => p.note_on(pitch, velocity),
            Instrument::Silent => {}
        }
    }

    /// Tell the built-in voices the track's tone cutoff, which per-note Mod X sweeps around.
    fn set_tone_hz(&mut self, hz: f32) {
        match self {
            Instrument::Synth(s) => s.set_tone_hz(hz),
            Instrument::Drums(d) => d.set_tone_hz(hz),
            Instrument::Plugin(_) | Instrument::Silent => {}
        }
    }
    pub fn note_off(&mut self, pitch: u8) {
        match self {
            Instrument::Synth(s) => s.note_off(pitch),
            Instrument::Plugin(p) => p.note_off(pitch),
            // A drum hit is a one-shot: it has no sustain to release.
            Instrument::Drums(_) | Instrument::Silent => {}
        }
    }
    pub fn all_notes_off(&mut self) {
        match self {
            Instrument::Synth(s) => s.all_notes_off(),
            Instrument::Plugin(p) => p.all_notes_off(),
            Instrument::Drums(_) | Instrument::Silent => {}
        }
    }
    pub fn is_silent(&self) -> bool {
        match self {
            Instrument::Synth(s) => s.is_silent(),
            Instrument::Drums(d) => d.is_silent(),
            Instrument::Plugin(p) => p.is_silent(),
            Instrument::Silent => true,
        }
    }
    fn render_add(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        match self {
            Instrument::Synth(s) => s.render_add(out_l, out_r),
            Instrument::Drums(d) => d.render_add(out_l, out_r),
            // A plugin goes through `Channel::render_add`'s own path.
            Instrument::Plugin(_) | Instrument::Silent => {}
        }
    }
}

/// One effect slot in a chain, on a track or on the master.
///
/// `unit` is `None` when the plugin the project names is not installed: the slot keeps its place
/// so the chain's order (and the name the UI shows) survives a round trip through a machine that
/// cannot load it.
pub struct InsertSlot {
    pub id: Uuid,
    pub enabled: bool,
    pub unit: Option<Box<PluginAudioUnit>>,
}

impl std::fmt::Debug for InsertSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InsertSlot")
            .field("id", &self.id)
            .field("enabled", &self.enabled)
            .field("loaded", &self.unit.is_some())
            .finish()
    }
}

impl InsertSlot {
    /// A slot starts enabled; a bypassed one is set so by its own command right after.
    pub fn new(id: Uuid, unit: Option<Box<PluginAudioUnit>>) -> Self {
        InsertSlot {
            id,
            enabled: true,
            unit,
        }
    }

    /// The unit this slot would run this block, or `None` when there is nothing to run.
    #[inline]
    fn active(&mut self) -> Option<&mut PluginAudioUnit> {
        if self.enabled {
            self.unit.as_deref_mut()
        } else {
            None
        }
    }

    /// Whether the slot has stopped making sound of its own. A bypassed or empty slot is silent
    /// by definition; a loaded effect is silent once its tail has run out.
    fn is_silent(&self) -> bool {
        match &self.unit {
            Some(unit) if self.enabled => unit.is_silent(),
            _ => true,
        }
    }
}

/// Run a chain in place over one block. A bypassed slot, or one whose plugin is missing, is
/// skipped outright: the signal passes through it untouched and unconverted.
pub fn run_inserts(chain: &mut [InsertSlot], l: &mut [f32], r: &mut [f32]) {
    for slot in chain {
        if let Some(unit) = slot.active() {
            unit.process_replace(l, r);
        }
    }
}

/// Whether every slot in a chain has finished ringing, so the chain can be skipped for a block
/// whose input is already silent.
pub fn chain_is_silent(chain: &[InsertSlot]) -> bool {
    chain.iter().all(InsertSlot::is_silent)
}

/// Not `Clone`: a hosted plugin instance cannot be duplicated, and nothing copies a live
/// channel — the engine moves them between sequences instead.
#[derive(Debug)]
pub struct Channel {
    pub instrument: Instrument,
    voice: String,
    is_drum: bool,
    is_audio: bool,
    /// The track's tone filter. Stereo, because the built-in voices now place themselves from
    /// each note's pan and the two sides are no longer the same signal.
    tone: StereoBiquad,
    tone_hz: f32,
    reverb: Reverb,
    gain: f32,
    pan: (f32, f32),
    sample_rate: f32,
    /// Stereo scratch: the instrument renders here, then the channel strip runs over it.
    stereo: (Vec<f32>, Vec<f32>),
    /// This track's effect chain, pre-fader. Filled by the engine's insert commands, never by
    /// [`Channel::apply`]: loading a plugin is far too slow for the audio deadline, so a
    /// sequence rebuild must leave a live chain exactly where it is.
    pub inserts: Vec<InsertSlot>,
    /// Set on an audio track: where its clips and monitored input are rendered. Boxed for the
    /// same reason as `Instrument::Plugin` — a rig is far larger than the rest of a channel.
    pub audio: Option<Box<AudioLane>>,
    pub peak: f32,
}

impl Channel {
    /// The instrument a track gets from its own settings, with no plugin involved. Shared by
    /// channel creation and by the engine when a plugin is cleared.
    pub fn built_in_instrument(track: &SequenceTrack, sample_rate: f32) -> Instrument {
        if track.is_audio {
            Instrument::Silent
        } else if track.is_drum {
            Instrument::Drums(DrumKit::new(sample_rate))
        } else {
            Instrument::Synth(PolySynth::new(
                VoicePreset::named(&track.voice),
                sample_rate,
                12,
            ))
        }
    }

    pub fn new(track: &SequenceTrack, sample_rate: f32) -> Self {
        let instrument = Self::built_in_instrument(track, sample_rate);
        let mut c = Channel {
            instrument,
            voice: track.voice.clone(),
            is_drum: track.is_drum,
            is_audio: track.is_audio,
            tone: StereoBiquad::identity(),
            tone_hz: 0.0,
            reverb: Reverb::new(sample_rate),
            gain: 1.0,
            pan: pan_gains(0.0),
            sample_rate,
            stereo: (vec![0.0; 8192], vec![0.0; 8192]),
            inserts: Vec::new(),
            audio: track
                .is_audio
                .then(|| Box::new(AudioLane::new(track.id, sample_rate))),
            peak: 0.0,
        };
        c.apply(track);
        c
    }

    /// Update params from the track; swaps the instrument if the voice/kind changed.
    ///
    /// A plugin is never swapped away here: it is installed by `EngineCommand::SetInstrument`
    /// from the app thread, because loading one is far too slow for the audio deadline, and a
    /// sequence rebuild must not silently replace it with the track's fallback voice.
    pub fn apply(&mut self, track: &SequenceTrack) {
        let hosts_plugin = matches!(self.instrument, Instrument::Plugin(_));
        let kind_changed = track.is_drum != self.is_drum || track.is_audio != self.is_audio;
        if !hosts_plugin
            && (kind_changed || (!track.is_drum && !track.is_audio && track.voice != self.voice))
        {
            self.instrument = Self::built_in_instrument(track, self.sample_rate);
        }
        self.is_drum = track.is_drum;
        self.is_audio = track.is_audio;
        self.voice = track.voice.clone();
        match (track.is_audio, &mut self.audio) {
            (true, Some(lane)) => lane.apply(track),
            (true, slot @ None) => {
                let mut lane = Box::new(AudioLane::new(track.id, self.sample_rate));
                lane.apply(track);
                *slot = Some(lane);
            }
            (false, slot) => *slot = None,
        }
        let tone_hz = track.tone.clamp(200.0, 20_000.0) as f32;
        if (tone_hz - self.tone_hz).abs() > 0.5 {
            self.tone
                .configure(|b| b.lowpass(self.sample_rate, tone_hz, 0.707));
            self.tone_hz = tone_hz;
        }
        // A note's Mod X is an offset from the track's own voicing, so the voices need to know
        // what that voicing currently is. Pushed on every apply, including the one that just
        // built a fresh instrument above.
        self.instrument.set_tone_hz(self.tone_hz);
        self.reverb.wet = track.reverb_send.clamp(0.0, 1.0) as f32;
        self.gain = track.volume.clamp(0.0, 1.5) as f32;
        self.pan = pan_gains(track.pan as f32);
    }

    /// Render `frames` samples and add them into the stereo mix buffers.
    pub fn render_add(&mut self, mix_l: &mut [f32], mix_r: &mut [f32], ctx: &BlockContext<'_>) {
        let frames = mix_l.len();

        // An audio track's sound comes from its lane, not from an instrument. It takes the
        // plugin path's treatment — inserts, gain, pan and reverb on both sides, then peak
        // tracking — so it meters, pans and mutes exactly like any other track.
        if self.audio.is_some() {
            self.clear_scratch(frames);
            let wrote = {
                let (al, ar) = (&mut self.stereo.0[..frames], &mut self.stereo.1[..frames]);
                // `is_some` above, so the lane is there; taking it out of the `if let` keeps the
                // scratch borrow and the lane borrow from overlapping.
                match &mut self.audio {
                    Some(lane) => lane.render_add(al, ar, ctx, self.sample_rate),
                    None => false,
                }
            };
            // Nothing played, no send tail to flush and no insert still ringing: let the meter
            // fall and stop.
            if !wrote && self.reverb.wet <= 0.0001 && chain_is_silent(&self.inserts) {
                self.hold_peak(0.0, frames);
                return;
            }
            self.strip(frames, mix_l, mix_r);
            return;
        }

        if self.instrument.is_silent()
            && self.reverb.wet <= 0.0001
            && chain_is_silent(&self.inserts)
        {
            // Nothing to render, but the meter still has to fall back to zero — otherwise it
            // freezes at whatever the last audible block measured and the UI stays lit.
            self.hold_peak(0.0, frames);
            return;
        }
        self.clear_scratch(frames);
        if let Instrument::Plugin(plugin) = &mut self.instrument {
            // A plugin produces its own stereo image, so panning scales the two sides it gives
            // us rather than placing a mono source. The channel's tone filter is left for the
            // built-in voices.
            plugin.render_add(&mut self.stereo.0[..frames], &mut self.stereo.1[..frames]);
            self.strip(frames, mix_l, mix_r);
            return;
        }
        {
            let (il, ir) = (&mut self.stereo.0[..frames], &mut self.stereo.1[..frames]);
            self.instrument.render_add(il, ir);
            // Per-note pan has already placed the voice between the two sides; the tone filter
            // is stereo so the two sides are filtered independently.
            for i in 0..frames {
                let (tl, tr) = self.tone.process(il[i], ir[i]);
                il[i] = tl;
                ir[i] = tr;
            }
        }
        self.strip(frames, mix_l, mix_r);
    }

    /// Grow the stereo scratch to `frames` and clear it.
    fn clear_scratch(&mut self, frames: usize) {
        if self.stereo.0.len() < frames {
            self.stereo.0.resize(frames, 0.0);
            self.stereo.1.resize(frames, 0.0);
        }
        self.stereo.0[..frames].fill(0.0);
        self.stereo.1[..frames].fill(0.0);
    }

    /// The rest of the channel strip over whatever is in the scratch: inserts → gain → pan →
    /// reverb send → meter, added into the mix. Inserts sit here, pre-fader, which is what an
    /// insert means in every other DAW: the chain hears the instrument at its own level.
    fn strip(&mut self, frames: usize, mix_l: &mut [f32], mix_r: &mut [f32]) {
        let peak = {
            let (gl, gr) = self.pan;
            let gain = self.gain;
            let (sl, sr) = (&mut self.stereo.0[..frames], &mut self.stereo.1[..frames]);
            run_inserts(&mut self.inserts, sl, sr);
            let reverb = &mut self.reverb;
            let mut peak = 0.0f32;
            for i in 0..frames {
                let (l, r) = reverb.process(sl[i] * gain * gl, sr[i] * gain * gr);
                mix_l[i] += l;
                mix_r[i] += r;
                peak = peak.max(l.abs().max(r.abs()));
            }
            peak
        };
        self.hold_peak(peak, frames);
    }

    fn hold_peak(&mut self, block_peak: f32, frames: usize) {
        self.peak = hold(block_peak, self.peak, frames, self.sample_rate);
    }

    /// Let the meter fall for a block this channel didn't render at all (muted and finished
    /// releasing); without it the meter freezes lit at whatever the last audible block measured.
    pub fn decay_meter(&mut self, frames: usize) {
        self.hold_peak(0.0, frames);
    }
}

/// How much of a peak survives one second of silence (≈ -26 dB/s). The decay is per unit time,
/// not per call: the engine renders a sub-block per note event and the device picks the buffer
/// size, so a per-call factor makes the meters fall between 7× faster and slower for identical
/// audio.
const METER_DECAY_PER_SECOND: f32 = 0.05;

/// Below this the UI already draws an empty meter (`peakDb` floors at -60 dB), so snap to zero
/// rather than asymptote — the meter lands on 0 and the audio thread never grinds on denormals.
const METER_FLOOR: f32 = 1e-3;

#[inline]
fn hold(block_peak: f32, previous: f32, frames: usize, sample_rate: f32) -> f32 {
    let decay = METER_DECAY_PER_SECOND.powf(frames as f32 / sample_rate);
    let held = block_peak.max(previous * decay);
    if held < METER_FLOOR {
        0.0
    } else {
        held
    }
}

#[derive(Debug, Clone)]
pub struct Master {
    low_cut: StereoBiquad,
    eq_low: StereoBiquad,
    eq_mid: StereoBiquad,
    eq_high: StereoBiquad,
    high_cut: StereoBiquad,
    reverb: Reverb,
    gain: f32,
    sample_rate: f32,
    settings: MasterSettings,
    pub peak_l: f32,
    pub peak_r: f32,
}

impl Master {
    pub fn new(sample_rate: f32, settings: &MasterSettings) -> Self {
        let mut m = Master {
            low_cut: StereoBiquad::identity(),
            eq_low: StereoBiquad::identity(),
            eq_mid: StereoBiquad::identity(),
            eq_high: StereoBiquad::identity(),
            high_cut: StereoBiquad::identity(),
            reverb: Reverb::new(sample_rate),
            gain: 1.0,
            sample_rate,
            settings: MasterSettings {
                gain: f64::NAN,
                ..settings.clone()
            },
            peak_l: 0.0,
            peak_r: 0.0,
        };
        m.apply(settings);
        m
    }

    pub fn apply(&mut self, s: &MasterSettings) {
        if *s == self.settings {
            return;
        }
        let sr = self.sample_rate;
        self.low_cut
            .configure(|b| b.highpass(sr, s.low_cut_hz as f32, 0.707));
        self.eq_low
            .configure(|b| b.low_shelf(sr, 120.0, linear_to_db(s.low_eq as f32), 0.7));
        self.eq_mid
            .configure(|b| b.peaking(sr, 1_000.0, linear_to_db(s.mid_eq as f32), 0.7));
        self.eq_high
            .configure(|b| b.high_shelf(sr, 6_000.0, linear_to_db(s.high_eq as f32), 0.7));
        self.high_cut
            .configure(|b| b.lowpass(sr, s.high_cut_hz as f32, 0.707));
        self.reverb.wet = s.reverb_wet.clamp(0.0, 1.0) as f32;
        self.gain = s.gain.clamp(0.0, 1.5) as f32;
        self.settings = s.clone();
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let (mut pl, mut pr) = (0.0f32, 0.0f32);
        for i in 0..l.len() {
            let (mut a, mut b) = self.low_cut.process(l[i], r[i]);
            (a, b) = self.eq_low.process(a, b);
            (a, b) = self.eq_mid.process(a, b);
            (a, b) = self.eq_high.process(a, b);
            (a, b) = self.high_cut.process(a, b);
            (a, b) = self.reverb.process(a, b);
            a = (a * self.gain).clamp(-1.0, 1.0);
            b = (b * self.gain).clamp(-1.0, 1.0);
            l[i] = a;
            r[i] = b;
            pl = pl.max(a.abs());
            pr = pr.max(b.abs());
        }
        let frames = l.len();
        self.peak_l = hold(pl, self.peak_l, frames, self.sample_rate);
        self.peak_r = hold(pr, self.peak_r, frames, self.sample_rate);
    }
}
