import Foundation

/// A loop — a named section like "verse" or "chorus". Holds its own chord progression (so
/// guidance is per-section) and the notes for each track, keyed by track id. Patterns are
/// reused: the song arrangement sequences them into a full track.
public struct SongPattern: Codable, Equatable, Identifiable, Sendable {
    public var id: UUID
    public var name: String
    public var lengthBeats: Double
    public var chords: ChordTrackModel
    public var notesByTrack: [UUID: [NoteEvent]]

    public init(
        id: UUID = UUID(),
        name: String,
        lengthBeats: Double = 16,
        chords: ChordTrackModel = ChordTrackModel(),
        notesByTrack: [UUID: [NoteEvent]] = [:]
    ) {
        self.id = id
        self.name = name
        self.lengthBeats = lengthBeats
        self.chords = chords
        self.notesByTrack = notesByTrack
    }

    public func notes(for trackID: UUID) -> [NoteEvent] {
        notesByTrack[trackID] ?? []
    }
}

/// Flattens a pattern arrangement into a single continuous timeline — the song. Each helper
/// concatenates patterns in arrangement order, offsetting note/chord beats by the running
/// length. Pure and testable; song playback and the song-mode playhead read from these.
public enum Arrangement {
    public static func totalLength(patterns: [SongPattern], arrangement: [UUID]) -> Double {
        arrangement.reduce(0) { sum, pid in
            sum + (patterns.first { $0.id == pid }?.lengthBeats ?? 0)
        }
    }

    public static func flattenedNotes(trackID: UUID, patterns: [SongPattern], arrangement: [UUID]) -> [NoteEvent] {
        var result: [NoteEvent] = []
        var offset = 0.0
        for pid in arrangement {
            guard let pattern = patterns.first(where: { $0.id == pid }) else { continue }
            for note in pattern.notes(for: trackID) {
                result.append(NoteEvent(startBeat: note.startBeat + offset, lengthBeats: note.lengthBeats, pitch: note.pitch))
            }
            offset += pattern.lengthBeats
        }
        return result
    }

    public static func flattenedChords(patterns: [SongPattern], arrangement: [UUID]) -> ChordTrackModel {
        var chords: [ChordEvent] = []
        var offset = 0.0
        for pid in arrangement {
            guard let pattern = patterns.first(where: { $0.id == pid }) else { continue }
            for chord in pattern.chords.chords {
                chords.append(ChordEvent(startBeat: chord.startBeat + offset, lengthBeats: chord.lengthBeats, pitchClasses: chord.pitchClasses, name: chord.name))
            }
            offset += pattern.lengthBeats
        }
        return ChordTrackModel(chords: chords)
    }
}
