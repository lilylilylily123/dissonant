import Foundation

/// The chord track: an ordered set of chords over time, and the lookup the rest of the
/// engine depends on — "what chord is sounding at beat T". This is the authoritative
/// harmonic context that drives the piano-roll tiering (R4, R10).
///
/// Chords occupy half-open beat intervals `[startBeat, startBeat + lengthBeats)`. Where two
/// chords overlap, the later-starting one wins (so a freshly dropped chord takes precedence).
public struct ChordTrackModel: Codable, Equatable, Sendable {
    public private(set) var chords: [ChordEvent]

    public init(chords: [ChordEvent] = []) {
        self.chords = chords.sorted { $0.startBeat < $1.startBeat }
    }

    /// The chord sounding at `beat`, or `nil` in a gap or empty track (drives cold-start, R12).
    public func chord(atBeat beat: Double) -> ChordEvent? {
        chords.last { contains(beat, $0) }
    }

    public var isEmpty: Bool { chords.isEmpty }

    // MARK: - Editing (keeps `chords` sorted by start)

    public mutating func add(_ chord: ChordEvent) {
        chords.append(chord)
        resort()
    }

    public mutating func remove(id: UUID) {
        chords.removeAll { $0.id == id }
    }

    /// Replace a chord (matched by id) in place; re-sorts if its start moved.
    public mutating func update(_ chord: ChordEvent) {
        guard let idx = chords.firstIndex(where: { $0.id == chord.id }) else { return }
        chords[idx] = chord
        resort()
    }

    private func contains(_ beat: Double, _ chord: ChordEvent) -> Bool {
        beat >= chord.startBeat && beat < chord.startBeat + chord.lengthBeats
    }

    private mutating func resort() {
        chords.sort { $0.startBeat < $1.startBeat }
    }
}
