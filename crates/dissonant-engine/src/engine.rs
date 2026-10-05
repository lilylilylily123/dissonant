//! The block processor: transport + sample-accurate event scheduling + mixing.

use crate::channel::{Channel, Master};
use crate::synth::{PolySynth, VoicePreset};
use dissonant_core::{MasterSettings, Sequence, Tempo};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use uuid::Uuid;

pub const MAX_TRACKS: usize = 64;

/// Audition hold bounds. The floor keeps very fine grids audible; the ceiling stops a stray
/// value from leaving a voice ringing.
pub const MIN_AUDITION_SECS: f32 = 0.12;
pub const MAX_AUDITION_SECS: f32 = 4.0;

/// What the real-time side publishes for the UI to read without locks.
#[derive(Debug)]
pub struct Shared {
    position_bits: AtomicU64,
    playing: AtomicBool,
    sample_rate: AtomicU32,
    /// Per-track peak (index = position in the current sequence), as f32 bits.
    track_peaks: [AtomicU32; MAX_TRACKS],
    master_peak: [AtomicU32; 2],
}

impl Default for Shared {
    fn default() -> Self {
        Shared {
            position_bits: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            sample_rate: AtomicU32::new(44_100),
            track_peaks: std::array::from_fn(|_| AtomicU32::new(0)),
            master_peak: [AtomicU32::new(0), AtomicU32::new(0)],
        }
    }
}

impl Shared {
    pub fn position_beats(&self) -> f64 {
        f64::from_bits(self.position_bits.load(Ordering::Relaxed))
    }
    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate.load(Ordering::Relaxed)
    }
    pub fn track_peak(&self, index: usize) -> f32 {
        self.track_peaks
            .get(index)
            .map(|a| f32::from_bits(a.load(Ordering::Relaxed)))
            .unwrap_or(0.0)
    }
    pub fn master_peak(&self) -> (f32, f32) {
        (
            f32::from_bits(self.master_peak[0].load(Ordering::Relaxed)),
            f32::from_bits(self.master_peak[1].load(Ordering::Relaxed)),
        )
    }
}

/// Messages from the application to the audio thread.
#[derive(Debug, Clone)]
pub enum EngineCommand {
    Play,
    Stop,
    Seek { beat: f64 },
    SetLooping(bool),
    /// Replace what's playing. The old `Arc` is dropped on the audio thread; sequences are
    /// small so this is acceptable for now (a return channel is the real-time-pure fix).
    SetSequence(Arc<Sequence>),
    SetHearChords(bool),
    SetMaster(MasterSettings),
    /// Sound a note on a track right now (placement audition / typing keyboard). `seconds` is
    /// how long to hold it before the automatic release — the roll passes the length of the note
    /// being previewed, so auditions don't outlast the note the user is drawing.
    Audition { track_id: Uuid, pitch: u8, velocity: u8, seconds: f32 },
    /// Release an audition note early (typing keyboard key-up).
    AuditionOff { track_id: Uuid, pitch: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Event {
    offset: usize,
    kind: EventKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum EventKind {
    NoteOn { track: usize, pitch: u8, velocity: u8 },
    NoteOff { track: usize, pitch: u8 },
    ChordOn { pitch: u8 },
    ChordOff { pitch: u8 },
    ReleaseAll,
}

struct Audition {
    track_id: Uuid,
    pitch: u8,
    samples_left: usize,
}

pub struct Engine {
    sample_rate: f32,
    shared: Arc<Shared>,
    sequence: Arc<Sequence>,
    channels: Vec<Channel>,
    channel_ids: Vec<Uuid>,
    chord_bed: PolySynth,
    hear_chords: bool,
    master: Master,
    playing: bool,
    looping: bool,
    position: f64,
    tempo: Tempo,
    mix_l: Vec<f32>,
    mix_r: Vec<f32>,
    events: Vec<Event>,
    sounding_chord: Option<Uuid>,
    auditions: Vec<Audition>,
    /// Set by Play/Seek: the next block starts any note the playhead is sitting inside.
    resume_notes: bool,
}

impl Engine {
    pub fn new(sample_rate: f32, shared: Arc<Shared>, master: &MasterSettings) -> Self {
        shared.sample_rate.store(sample_rate as u32, Ordering::Relaxed);
        Engine {
            sample_rate,
            shared,
            sequence: Arc::new(Sequence::default()),
            channels: Vec::new(),
            channel_ids: Vec::new(),
            chord_bed: PolySynth::new(VoicePreset::named("pad"), sample_rate, 8),
            hear_chords: false,
            master: Master::new(sample_rate, master),
            playing: false,
            looping: true,
            position: 0.0,
            tempo: Tempo::default(),
            mix_l: vec![0.0; 8192],
            mix_r: vec![0.0; 8192],
            events: Vec::with_capacity(512),
            sounding_chord: None,
            auditions: Vec::with_capacity(32),
            resume_notes: false,
        }
    }

    pub fn position(&self) -> f64 {
        self.position
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn handle(&mut self, cmd: EngineCommand) {
        match cmd {
            EngineCommand::Play => {
                self.playing = true;
                // Starting mid-note (after a stop or a seek) must sound that note, not wait for
                // the next one.
                self.resume_notes = true;
            }
            EngineCommand::Stop => {
                self.playing = false;
                self.release_all();
            }
            EngineCommand::Seek { beat } => {
                self.release_all();
                self.position = beat.max(0.0);
                if self.looping && self.position >= self.sequence.length_beats {
                    self.position = 0.0;
                }
                self.resume_notes = true;
            }
            EngineCommand::SetLooping(on) => self.looping = on,
            EngineCommand::SetSequence(seq) => self.set_sequence(seq),
            EngineCommand::SetHearChords(on) => {
                self.hear_chords = on;
                if !on {
                    self.chord_bed.all_notes_off();
                    self.sounding_chord = None;
                }
            }
            EngineCommand::SetMaster(settings) => self.master.apply(&settings),
            EngineCommand::Audition { track_id, pitch, velocity, seconds } => {
                if let Some(ch) = self.channel_mut(&track_id) {
                    ch.instrument.note_on(pitch, velocity);
                    let hold = (self.sample_rate * seconds.clamp(MIN_AUDITION_SECS, MAX_AUDITION_SECS)) as usize;
                    if let Some(a) = self.auditions.iter_mut().find(|a| a.track_id == track_id && a.pitch == pitch) {
                        a.samples_left = hold;
                    } else {
                        self.auditions.push(Audition { track_id, pitch, samples_left: hold });
                    }
                }
            }
            EngineCommand::AuditionOff { track_id, pitch } => {
                if let Some(ch) = self.channel_mut(&track_id) {
                    ch.instrument.note_off(pitch);
                }
                self.auditions.retain(|a| !(a.track_id == track_id && a.pitch == pitch));
            }
        }
        self.publish();
    }

    fn channel_mut(&mut self, id: &Uuid) -> Option<&mut Channel> {
        let idx = self.channel_ids.iter().position(|c| c == id)?;
        self.channels.get_mut(idx)
    }

    fn set_sequence(&mut self, seq: Arc<Sequence>) {
        // Rebuild the channel list in sequence order, keeping existing channels (and their
        // sounding voices) for tracks that are still present.
        let mut new_channels = Vec::with_capacity(seq.tracks.len());
        let mut new_ids = Vec::with_capacity(seq.tracks.len());
        let old_ids = std::mem::take(&mut self.channel_ids);
        let mut old = std::mem::take(&mut self.channels).into_iter().map(Some).collect::<Vec<_>>();
        for track in &seq.tracks {
            let existing = old_ids.iter().position(|id| id == &track.id).and_then(|i| old[i].take());
            let mut ch = match existing {
                Some(ch) => ch,
                None => Channel::new(track, self.sample_rate),
            };
            ch.apply(track);
            new_channels.push(ch);
            new_ids.push(track.id);
        }
        // Notes that vanished or moved under the playhead: release everything that is
        // sounding on a track whose note list changed, so nothing hangs.
        let old_seq = std::mem::replace(&mut self.sequence, seq);
        for (i, track) in self.sequence.tracks.iter().enumerate() {
            let changed = old_seq
                .tracks
                .iter()
                .find(|t| t.id == track.id)
                .map(|t| t.notes != track.notes || t.audible(old_seq.any_solo()) != track.audible(self.sequence.any_solo()))
                .unwrap_or(true);
            if changed {
                new_channels[i].instrument.all_notes_off();
            }
        }
        if old_seq.chords != self.sequence.chords {
            self.chord_bed.all_notes_off();
            self.sounding_chord = None;
        }
        self.channels = new_channels;
        self.channel_ids = new_ids;
        self.tempo = Tempo::new(self.sequence.tempo_bpm);
        if self.looping && self.position >= self.sequence.length_beats {
            self.position = 0.0;
        }
    }

    fn release_all(&mut self) {
        for ch in &mut self.channels {
            ch.instrument.all_notes_off();
        }
        self.chord_bed.all_notes_off();
        self.sounding_chord = None;
        self.auditions.clear();
    }

    fn publish(&self) {
        self.shared.position_bits.store(self.position.to_bits(), Ordering::Relaxed);
        self.shared.playing.store(self.playing, Ordering::Relaxed);
        for (i, ch) in self.channels.iter().enumerate().take(MAX_TRACKS) {
            self.shared.track_peaks[i].store(ch.peak.to_bits(), Ordering::Relaxed);
        }
        self.shared.master_peak[0].store(self.master.peak_l.to_bits(), Ordering::Relaxed);
        self.shared.master_peak[1].store(self.master.peak_r.to_bits(), Ordering::Relaxed);
    }

    /// Render one block of interleaved stereo. `out.len()` must be even.
    pub fn process(&mut self, out: &mut [f32]) {
        let frames = out.len() / 2;
        if self.mix_l.len() < frames {
            self.mix_l.resize(frames, 0.0);
            self.mix_r.resize(frames, 0.0);
        }
        self.mix_l[..frames].fill(0.0);
        self.mix_r[..frames].fill(0.0);

        self.collect_events(frames);
        self.tick_auditions(frames);

        // Render sub-blocks between events so every note starts on its exact sample.
        let mut cursor = 0usize;
        let mut ev_idx = 0usize;
        while cursor < frames {
            let next = self.events.get(ev_idx).map(|e| e.offset.min(frames)).unwrap_or(frames);
            if next > cursor {
                self.render_range(cursor, next);
                cursor = next;
            }
            while let Some(e) = self.events.get(ev_idx).filter(|e| e.offset.min(frames) <= cursor) {
                self.fire(e.kind);
                ev_idx += 1;
            }
        }

        let (l, r) = (&mut self.mix_l[..frames], &mut self.mix_r[..frames]);
        self.master.process(l, r);
        for i in 0..frames {
            out[2 * i] = l[i];
            out[2 * i + 1] = r[i];
        }

        if self.playing {
            self.position = self.advance(self.position, frames);
        }
        self.publish();
    }

    fn render_range(&mut self, from: usize, to: usize) {
        let (l, r) = (&mut self.mix_l[from..to], &mut self.mix_r[from..to]);
        let any_solo = self.sequence.any_solo();
        for (ch, track) in self.channels.iter_mut().zip(self.sequence.tracks.iter()) {
            if track.audible(any_solo) || !ch.instrument.is_silent() {
                ch.render_add(l, r);
            } else {
                // Muted and finished releasing: nothing to mix, but the meter still has to fall
                // instead of staying lit at the level it had when the track was muted.
                ch.decay_meter(to - from);
            }
        }
        if !self.chord_bed.is_silent() {
            let frames = to - from;
            // Render the bed mono into a scratch slice of mix_l's tail? No: reuse a small
            // stack buffer in chunks to avoid allocation.
            let mut buf = [0.0f32; 256];
            let mut done = 0;
            while done < frames {
                let n = (frames - done).min(256);
                let chunk = &mut buf[..n];
                chunk.fill(0.0);
                self.chord_bed.render_add(chunk);
                for i in 0..n {
                    let s = chunk[i] * 0.6;
                    l[done + i] += s;
                    r[done + i] += s;
                }
                done += n;
            }
        }
    }

    fn fire(&mut self, kind: EventKind) {
        match kind {
            EventKind::NoteOn { track, pitch, velocity } => {
                if let Some(ch) = self.channels.get_mut(track) {
                    ch.instrument.note_on(pitch, velocity);
                }
            }
            EventKind::NoteOff { track, pitch } => {
                if let Some(ch) = self.channels.get_mut(track) {
                    ch.instrument.note_off(pitch);
                }
            }
            EventKind::ChordOn { pitch } => self.chord_bed.note_on(pitch, 80),
            EventKind::ChordOff { pitch } => self.chord_bed.note_off(pitch),
            EventKind::ReleaseAll => {
                for ch in &mut self.channels {
                    ch.instrument.all_notes_off();
                }
                self.chord_bed.all_notes_off();
                self.sounding_chord = None;
            }
        }
    }

    fn advance(&self, position: f64, frames: usize) -> f64 {
        let beats = self.tempo.beats_for_samples(frames as f64, self.sample_rate as f64);
        let mut p = position + beats;
        let len = self.sequence.length_beats;
        if self.looping && len > 0.0 && p >= len {
            p = (p - len) % len;
        }
        p
    }

    /// Fill `self.events` with everything that happens during the next `frames` samples.
    fn collect_events(&mut self, frames: usize) {
        self.events.clear();
        if !self.playing {
            return;
        }
        let spb = self.tempo.samples_per_beat(self.sample_rate as f64);
        let block_beats = frames as f64 / spb;
        let len = self.sequence.length_beats;
        let start = self.position;
        let end = start + block_beats;

        if std::mem::take(&mut self.resume_notes) {
            self.resume_notes_under(start);
        }

        if self.looping && len > 0.0 && end >= len {
            // Two segments: [start, len) then [0, end - len), with a release at the seam.
            let seam = ((len - start) * spb).round() as usize;
            self.collect_segment(start, len, 0, spb);
            self.events.push(Event { offset: seam.min(frames), kind: EventKind::ReleaseAll });
            self.collect_segment(0.0, end - len, seam, spb);
        } else {
            self.collect_segment(start, end, 0, spb);
        }
        self.events.sort_by_key(|e| e.offset);
    }

    /// Notes already sounding at `from` — the ones the playhead landed inside. Scheduling only
    /// fires a note at its start beat, so without this, seeking into (or starting playback in)
    /// the middle of a long note gives silence until the next note begins. The chord bed has
    /// always resumed this way; notes now match it.
    fn resume_notes_under(&mut self, from: f64) {
        let any_solo = self.sequence.any_solo();
        let seq = Arc::clone(&self.sequence);
        for (ti, track) in seq.tracks.iter().enumerate() {
            // A drum hit is a one-shot: re-firing it mid-note would retrigger the sample.
            if !track.audible(any_solo) || track.is_drum {
                continue;
            }
            for note in &track.notes {
                if note.start_beat < from && note.end_beat().min(seq.length_beats) > from {
                    self.events.push(Event {
                        offset: 0,
                        kind: EventKind::NoteOn {
                            track: ti,
                            pitch: note.pitch.clamp(0, 127) as u8,
                            velocity: note.velocity.clamp(1, 127) as u8,
                        },
                    });
                }
            }
        }
    }

    fn collect_segment(&mut self, from: f64, to: f64, base_offset: usize, spb: f64) {
        let any_solo = self.sequence.any_solo();
        let seq = Arc::clone(&self.sequence);
        let offset_of = |beat: f64| base_offset + ((beat - from) * spb).round().max(0.0) as usize;

        for (ti, track) in seq.tracks.iter().enumerate() {
            if !track.audible(any_solo) {
                continue;
            }
            for note in &track.notes {
                let s = note.start_beat;
                let e = note.end_beat().min(seq.length_beats);
                if s >= from && s < to {
                    self.events.push(Event {
                        offset: offset_of(s),
                        kind: EventKind::NoteOn {
                            track: ti,
                            pitch: note.pitch.clamp(0, 127) as u8,
                            velocity: note.velocity.clamp(1, 127) as u8,
                        },
                    });
                }
                if e > s && e >= from && e < to && !track.is_drum {
                    self.events.push(Event {
                        offset: offset_of(e),
                        kind: EventKind::NoteOff {
                            track: ti,
                            pitch: note.pitch.clamp(0, 127) as u8,
                        },
                    });
                }
            }
        }

        if self.hear_chords {
            for chord in seq.chords.chords() {
                let s = chord.start_beat;
                let e = chord.end_beat().min(seq.length_beats);
                if s >= from && s < to {
                    for &pc in &chord.pitch_classes {
                        self.events.push(Event {
                            offset: offset_of(s),
                            kind: EventKind::ChordOn { pitch: (60 + pc.rem_euclid(12)) as u8 },
                        });
                    }
                }
                if e > s && e >= from && e < to {
                    for &pc in &chord.pitch_classes {
                        self.events.push(Event {
                            offset: offset_of(e),
                            kind: EventKind::ChordOff { pitch: (60 + pc.rem_euclid(12)) as u8 },
                        });
                    }
                }
            }
            // Seeking into the middle of a chord: start it now.
            if let Some(chord) = seq.chords.chord_at(from) {
                if self.sounding_chord != Some(chord.id) && chord.start_beat < from {
                    for &pc in &chord.pitch_classes {
                        self.events.push(Event {
                            offset: base_offset,
                            kind: EventKind::ChordOn { pitch: (60 + pc.rem_euclid(12)) as u8 },
                        });
                    }
                }
                self.sounding_chord = Some(chord.id);
            } else {
                self.sounding_chord = None;
            }
        }
    }

    fn tick_auditions(&mut self, frames: usize) {
        let mut expired: Vec<(Uuid, u8)> = Vec::new();
        for a in &mut self.auditions {
            if a.samples_left <= frames {
                expired.push((a.track_id, a.pitch));
                a.samples_left = 0;
            } else {
                a.samples_left -= frames;
            }
        }
        if !expired.is_empty() {
            for (track_id, pitch) in expired {
                if let Some(ch) = self.channel_mut(&track_id) {
                    ch.instrument.note_off(pitch);
                }
            }
            self.auditions.retain(|a| a.samples_left > 0);
        }
    }

    /// Convenience for tests and offline rendering: build a lookup from track id to index.
    pub fn track_index(&self) -> HashMap<Uuid, usize> {
        self.channel_ids.iter().enumerate().map(|(i, id)| (*id, i)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dissonant_core::{NoteEvent, ProjectModel};

    const SR: f32 = 44_100.0;

    fn sequence_with_note(start: f64, len_beats: f64, loop_len: f64) -> Arc<Sequence> {
        let mut model = ProjectModel::empty();
        model.tracks[0].voice = "sine".into();
        let (tid, pid) = (model.tracks[0].id, model.patterns[0].id);
        model.patterns[0].length_beats = loop_len;
        model.patterns[0]
            .notes_by_track
            .insert(tid, vec![NoteEvent::new(start, len_beats, 69)]);
        Arc::new(Sequence::from_pattern(&model, &pid).unwrap())
    }

    fn render(engine: &mut Engine, total_frames: usize, block: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(total_frames);
        let mut buf = vec![0.0f32; block * 2];
        let mut done = 0;
        while done < total_frames {
            let n = (total_frames - done).min(block);
            engine.process(&mut buf[..n * 2]);
            out.extend(buf[..n * 2].iter().step_by(2));
            done += n;
        }
        out
    }

    fn first_onset(samples: &[f32]) -> Option<usize> {
        samples.iter().position(|s| s.abs() > 1e-6)
    }

    #[test]
    fn note_starts_on_the_exact_sample_regardless_of_block_size() {
        for block in [64usize, 480, 1024, 4096] {
            let mut e = Engine::new(SR, Arc::new(Shared::default()), &MasterSettings::default());
            e.handle(EngineCommand::SetSequence(sequence_with_note(1.0, 1.0, 16.0)));
            e.handle(EngineCommand::Play);
            let left = render(&mut e, 30_000, block);
            let onset = first_onset(&left).expect("note should sound");
            // 120 bpm → beat 1 = 22050 samples; the first non-zero sample is one after onset.
            assert!((onset as i64 - 22_050).abs() <= 2, "block {block}: onset {onset}");
        }
    }

    #[test]
    fn note_releases_and_loop_retriggers() {
        let mut e = Engine::new(SR, Arc::new(Shared::default()), &MasterSettings::default());
        e.handle(EngineCommand::SetSequence(sequence_with_note(0.0, 0.5, 2.0)));
        e.handle(EngineCommand::Play);
        let left = render(&mut e, 44_100 * 2, 512);
        // One loop = 2 beats = 44100 samples. The second onset must land at the loop start.
        let second_half = &left[44_100 - 10..];
        let onset = first_onset(second_half).unwrap() + 44_100 - 10;
        assert!((onset as i64 - 44_100).abs() <= 2, "second onset {onset}");
        // Between the release tail (0.5 beat = 11025 samples + 0.2 s release) and the loop, silence.
        let quiet = &left[11_025 + 10_000..44_000];
        assert!(quiet.iter().all(|s| s.abs() < 1e-3));
    }

    #[test]
    fn stopped_engine_is_silent_and_position_holds() {
        let mut e = Engine::new(SR, Arc::new(Shared::default()), &MasterSettings::default());
        e.handle(EngineCommand::SetSequence(sequence_with_note(0.0, 1.0, 4.0)));
        let left = render(&mut e, 10_000, 256);
        assert!(left.iter().all(|s| *s == 0.0));
        assert_eq!(e.position(), 0.0);
    }

    #[test]
    fn mute_and_solo_are_honored() {
        let mut model = ProjectModel::empty();
        model.tracks[0].voice = "sine".into();
        model.tracks[0].muted = true;
        let (tid, pid) = (model.tracks[0].id, model.patterns[0].id);
        model.patterns[0].notes_by_track.insert(tid, vec![NoteEvent::new(0.0, 1.0, 69)]);
        let mut e = Engine::new(SR, Arc::new(Shared::default()), &MasterSettings::default());
        e.handle(EngineCommand::SetSequence(Arc::new(Sequence::from_pattern(&model, &pid).unwrap())));
        e.handle(EngineCommand::Play);
        assert!(render(&mut e, 8_000, 256).iter().all(|s| s.abs() < 1e-6));
    }

    #[test]
    fn drums_and_chord_bed_make_sound() {
        let mut model = ProjectModel::starter();
        let drums = model.tracks[1].id;
        let pid = model.patterns[0].id;
        model.patterns[0].notes_by_track.insert(drums, vec![NoteEvent::new(0.0, 0.5, 36)]);
        let shared = Arc::new(Shared::default());
        let mut e = Engine::new(SR, shared.clone(), &MasterSettings::default());
        e.handle(EngineCommand::SetSequence(Arc::new(Sequence::from_pattern(&model, &pid).unwrap())));
        e.handle(EngineCommand::SetHearChords(true));
        e.handle(EngineCommand::Play);
        let left = render(&mut e, 20_000, 512);
        assert!(left.iter().any(|s| s.abs() > 0.05));
        assert!(shared.position_beats() > 0.0);
        assert!(shared.is_playing());
        assert!(shared.master_peak().0 > 0.0);
    }

    #[test]
    fn audition_plays_then_stops_by_itself() {
        let model = ProjectModel::empty();
        let (tid, pid) = (model.tracks[0].id, model.patterns[0].id);
        let mut e = Engine::new(SR, Arc::new(Shared::default()), &MasterSettings::default());
        e.handle(EngineCommand::SetSequence(Arc::new(Sequence::from_pattern(&model, &pid).unwrap())));
        e.handle(EngineCommand::Audition { track_id: tid, pitch: 60, velocity: 100, seconds: 0.8 });
        let left = render(&mut e, 44_100 * 2, 512);
        assert!(left[..4000].iter().any(|s| s.abs() > 0.01));
        assert!(left[70_000..].iter().all(|s| s.abs() < 1e-4));
    }

    /// A preview must not outlast the note being drawn: at a 1/16 grid (0.125 s at 120 bpm) the
    /// voice has to be releasing long before the fixed 0.8 s hold this used to apply, otherwise
    /// painting a fast run stacks overlapping voices into a wash that sounds like reverb.
    #[test]
    fn audition_honors_the_requested_length() {
        let model = ProjectModel::empty();
        let (tid, pid) = (model.tracks[0].id, model.patterns[0].id);
        let mut e = Engine::new(SR, Arc::new(Shared::default()), &MasterSettings::default());
        e.handle(EngineCommand::SetSequence(Arc::new(Sequence::from_pattern(&model, &pid).unwrap())));
        e.handle(EngineCommand::Audition { track_id: tid, pitch: 60, velocity: 100, seconds: 0.125 });
        let left = render(&mut e, SR as usize, 512);
        let sr = SR as usize;
        let loud = |from: usize, to: usize| left[from..to].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(loud(0, sr / 20) > 0.01, "the preview must speak");
        // Held 0.125 s, then the voice's release (~0.14 s) runs out: silent well before 0.5 s.
        assert!(loud(sr / 2, sr) < 1e-4, "preview outlived the note it previewed");
    }

    /// A grid finer than the floor still has to be audible rather than clicking off instantly.
    #[test]
    fn audition_length_is_clamped_to_a_floor() {
        let model = ProjectModel::empty();
        let (tid, pid) = (model.tracks[0].id, model.patterns[0].id);
        let mut e = Engine::new(SR, Arc::new(Shared::default()), &MasterSettings::default());
        e.handle(EngineCommand::SetSequence(Arc::new(Sequence::from_pattern(&model, &pid).unwrap())));
        e.handle(EngineCommand::Audition { track_id: tid, pitch: 60, velocity: 100, seconds: 0.0 });
        let left = render(&mut e, SR as usize, 512);
        let at_floor = (SR * MIN_AUDITION_SECS * 0.8) as usize;
        assert!(left[..at_floor].iter().any(|s| s.abs() > 0.01), "a sub-floor preview must still speak");
    }

    #[test]
    fn seek_wraps_into_loop_and_sequence_swap_keeps_channels() {
        let mut e = Engine::new(SR, Arc::new(Shared::default()), &MasterSettings::default());
        let seq = sequence_with_note(0.0, 1.0, 4.0);
        e.handle(EngineCommand::SetSequence(seq.clone()));
        e.handle(EngineCommand::Seek { beat: 9.0 });
        assert_eq!(e.position(), 0.0);
        e.handle(EngineCommand::Seek { beat: 2.5 });
        assert_eq!(e.position(), 2.5);
        e.handle(EngineCommand::SetSequence(seq.clone()));
        assert_eq!(e.track_index().len(), 1);
    }
}
