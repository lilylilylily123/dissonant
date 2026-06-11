import Foundation
import DissonantCore

/// Manages a per-track instrument + note-playback so tracks layer together, each through its
/// own voice. Tracks are matched by id; switching a track's voice swaps its instrument.
@MainActor
final class TrackVoices {
    private let audio: AudioEngineController
    private var entries: [UUID: Entry] = [:]

    @MainActor
    private final class Entry {
        var instrument: MidiPlayable
        let playback: NotePlayback
        init(instrument: MidiPlayable) {
            self.instrument = instrument
            self.playback = NotePlayback(instrument: instrument)
        }
    }

    init(audio: AudioEngineController) {
        self.audio = audio
    }

    /// Ensure every track has a voice (creating synth voices for new tracks).
    func sync(tracks: [Track]) {
        for track in tracks where entries[track.id] == nil {
            let inst = audio.makeSynthVoice(VoiceKind(rawValue: track.voice) ?? .saw)
            entries[track.id] = Entry(instrument: inst)
        }
    }

    func setSynth(trackID: UUID, voice: VoiceKind) {
        guard let entry = entries[trackID] else { return }
        entry.playback.releaseAll()
        let inst = audio.makeSynthVoice(voice)
        entry.instrument = inst
        entry.playback.instrument = inst
    }

    func setAU(trackID: UUID, instrument: MidiPlayable) {
        guard let entry = entries[trackID] else { return }
        entry.playback.releaseAll()
        entry.instrument = instrument
        entry.playback.instrument = instrument
    }

    /// Advance every audible track's playback for the current beat, pulling each track's notes
    /// from `notesForTrack` (the selected pattern in pattern mode, the flattened arrangement in
    /// song mode). Honors mute/solo; inaudible tracks are released so notes don't hang.
    func update(forBeat beat: Double, tracks: [Track], notesForTrack: (UUID) -> [NoteEvent]) {
        let anySolo = tracks.contains { $0.soloed }
        for track in tracks {
            let audible = anySolo ? track.soloed : !track.muted
            if audible {
                entries[track.id]?.playback.update(forBeat: beat, notes: notesForTrack(track.id))
            } else {
                entries[track.id]?.playback.releaseAll()
            }
        }
    }

    func releaseAll() {
        for entry in entries.values { entry.playback.releaseAll() }
    }

    func instrument(trackID: UUID) -> MidiPlayable? {
        entries[trackID]?.instrument
    }
}
