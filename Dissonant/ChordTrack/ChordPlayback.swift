import Foundation
import DissonantCore

/// Plays the chord track as an audible backing bed: as the playhead moves, it sustains the
/// chord under the playhead and re-triggers on each chord change. v1 voicing is simple
/// root-position in one octave — richer voicing is a later refinement.
@MainActor
final class ChordPlayback {
    private let instrument: Instrument
    private var currentChordID: UUID?
    private var soundingPitches: [UInt8] = []

    /// MIDI octave base for the bed (C4 = 60).
    private let octaveBase: Int = 60

    init(instrument: Instrument) {
        self.instrument = instrument
    }

    /// Call as the playhead advances. Triggers the chord at `beat`, releasing the previous
    /// one when the chord changes. A nil chord (gap / empty track) releases everything.
    func update(forBeat beat: Double, in track: ChordTrackModel) {
        let chord = track.chord(atBeat: beat)
        guard chord?.id != currentChordID else { return }
        releaseAll()
        if let chord { trigger(chord) }
        currentChordID = chord?.id
    }

    /// Audition a single chord immediately (used by the chord-building UI in U6).
    func audition(_ chord: ChordEvent) {
        releaseAll()
        trigger(chord)
        currentChordID = nil // a one-off audition, not the tracked playhead chord
    }

    func releaseAll() {
        for pitch in soundingPitches {
            instrument.noteOff(pitch)
        }
        soundingPitches = []
    }

    private func trigger(_ chord: ChordEvent) {
        soundingPitches = chord.pitchClasses.map { pc in
            UInt8(octaveBase + ((pc % 12) + 12) % 12)
        }
        for pitch in soundingPitches {
            instrument.noteOn(pitch, velocity: 80)
        }
    }
}
