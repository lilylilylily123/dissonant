import Foundation
import DissonantCore

/// Sounds the placed notes as the playhead crosses them. On each playhead update it diffs
/// the set of notes currently under the playhead against what's sounding, triggering note-ons
/// for newly-entered notes and note-offs for ones the playhead has passed. Retriggers
/// naturally on loop, since a note re-enters the active set when the playhead wraps back.
@MainActor
final class NotePlayback {
    /// The melody voice. Swap it (after `releaseAll`) when the user picks a different voice.
    var instrument: Instrument
    private var soundingPitch: [UUID: UInt8] = [:]

    init(instrument: Instrument) {
        self.instrument = instrument
    }

    func update(forBeat beat: Double, notes: [NoteEvent]) {
        let active = notes.filter { beat >= $0.startBeat && beat < $0.startBeat + $0.lengthBeats }
        let activeIDs = Set(active.map(\.id))

        // release notes the playhead has passed
        for (id, pitch) in soundingPitch where !activeIDs.contains(id) {
            instrument.noteOff(pitch)
            soundingPitch[id] = nil
        }
        // trigger newly-entered notes
        for note in active where soundingPitch[note.id] == nil {
            let pitch = UInt8(clamping: note.pitch)
            instrument.noteOn(pitch, velocity: 90)
            soundingPitch[note.id] = pitch
        }
    }

    func releaseAll() {
        for (_, pitch) in soundingPitch {
            instrument.noteOff(pitch)
        }
        soundingPitch = [:]
    }
}
