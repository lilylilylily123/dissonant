import Foundation

/// A diatonic scale family. Expanded in later units (modes, harmonic minor, etc.).
public enum ScaleType: String, Codable, CaseIterable, Sendable {
    case major
    case minor
}

/// The project's current key. `rootPitchClass == nil` means no key is locked yet —
/// the cold-start state (R1, R12). `isLocked` distinguishes a user-confirmed key
/// from a merely-inferred suggestion.
public struct KeyState: Codable, Equatable, Sendable {
    /// Pitch class of the tonic, 0 = C ... 11 = B. `nil` when no key is set.
    public var rootPitchClass: Int?
    public var scale: ScaleType
    public var isLocked: Bool

    public init(rootPitchClass: Int? = nil, scale: ScaleType = .major, isLocked: Bool = false) {
        self.rootPitchClass = rootPitchClass
        self.scale = scale
        self.isLocked = isLocked
    }

    /// No key set — the cold-start default.
    public static let none = KeyState()
}

/// One chord on the chord track. The chord is stored as bare pitch classes (0–11) in v1,
/// which keeps the model free of any music-theory dependency; `name` is an optional
/// display label and is never required (R9).
public struct ChordEvent: Codable, Equatable, Identifiable, Sendable {
    public var id: UUID
    public var startBeat: Double
    public var lengthBeats: Double
    public var pitchClasses: [Int]
    public var name: String?

    public init(id: UUID = UUID(), startBeat: Double, lengthBeats: Double, pitchClasses: [Int], name: String? = nil) {
        self.id = id
        self.startBeat = startBeat
        self.lengthBeats = lengthBeats
        self.pitchClasses = pitchClasses
        self.name = name
    }
}

/// One note in the piano roll. `pitch` is a MIDI note number (0–127).
public struct NoteEvent: Codable, Equatable, Identifiable, Sendable {
    public var id: UUID
    public var startBeat: Double
    public var lengthBeats: Double
    public var pitch: Int

    public init(id: UUID = UUID(), startBeat: Double, lengthBeats: Double, pitch: Int) {
        self.id = id
        self.startBeat = startBeat
        self.lengthBeats = lengthBeats
        self.pitch = pitch
    }
}

/// The full serializable project. `schemaVersion` lets later versions migrate forward.
///
/// Decoding is tolerant: a JSON document missing any field (e.g. one added in a later
/// version) decodes to that field's default rather than throwing, which keeps old
/// project files openable as the schema grows (R14).
public struct ProjectModel: Codable, Equatable, Sendable {
    public var schemaVersion: Int
    public var tempo: Double
    public var key: KeyState
    /// The project's instrument tracks (voices). Notes live in patterns, keyed by track id.
    public var tracks: [Track]
    /// Reusable loops. Each pattern has its own chords and per-track notes.
    public var patterns: [SongPattern]
    /// The song: an ordered list of pattern ids, played back to back.
    public var arrangement: [UUID]

    public init(
        schemaVersion: Int = ProjectModel.currentSchemaVersion,
        tempo: Double = 120,
        key: KeyState = .none,
        tracks: [Track] = [Track(name: "melody")],
        patterns: [SongPattern] = [SongPattern(name: "pattern 1")],
        arrangement: [UUID] = []
    ) {
        self.schemaVersion = schemaVersion
        self.tempo = tempo
        self.key = key
        self.tracks = tracks
        self.patterns = patterns
        self.arrangement = arrangement
    }

    public static let currentSchemaVersion = 2

    /// A brand-new, empty project: one track, one empty pattern.
    public static let empty = ProjectModel()

    /// A new project pre-seeded with a I–IV–V–vi progression in its first pattern.
    public static let starter: ProjectModel = {
        let chords = ChordTrackModel(chords: [
            ChordEvent(startBeat: 0, lengthBeats: 4, pitchClasses: [0, 4, 7], name: "C"),
            ChordEvent(startBeat: 4, lengthBeats: 4, pitchClasses: [5, 9, 0], name: "F"),
            ChordEvent(startBeat: 8, lengthBeats: 4, pitchClasses: [7, 11, 2], name: "G"),
            ChordEvent(startBeat: 12, lengthBeats: 4, pitchClasses: [9, 0, 4], name: "Am")
        ])
        let pattern = SongPattern(name: "pattern 1", chords: chords)
        return ProjectModel(tracks: [Track(name: "melody")], patterns: [pattern], arrangement: [pattern.id])
    }()

    private enum CodingKeys: String, CodingKey {
        case schemaVersion, tempo, key, tracks, patterns, arrangement
    }

    // Pre-pattern (v1) fields, read when migrating older files into a single pattern.
    private enum LegacyKeys: String, CodingKey {
        case chordTrack, noteEvents
    }

    private struct LegacyTrack: Decodable {
        var id: UUID
        var name: String
        var voice: String?
        var muted: Bool?
        var soloed: Bool?
        var noteEvents: [NoteEvent]?
    }

    // Tolerant decode + migration: a v1 file (tracks-with-notes + a flat chordTrack, or an
    // even older flat noteEvents) folds into a single "pattern 1".
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        schemaVersion = try c.decodeIfPresent(Int.self, forKey: .schemaVersion) ?? ProjectModel.currentSchemaVersion
        tempo = try c.decodeIfPresent(Double.self, forKey: .tempo) ?? 120
        key = try c.decodeIfPresent(KeyState.self, forKey: .key) ?? .none

        if let patterns = try c.decodeIfPresent([SongPattern].self, forKey: .patterns), !patterns.isEmpty {
            // v2+ file
            tracks = try c.decodeIfPresent([Track].self, forKey: .tracks) ?? [Track(name: "melody")]
            self.patterns = patterns
            arrangement = try c.decodeIfPresent([UUID].self, forKey: .arrangement) ?? []
        } else {
            // Migrate v1 → v2
            let legacy = try decoder.container(keyedBy: LegacyKeys.self)
            let legacyTracks = (try c.decodeIfPresent([LegacyTrack].self, forKey: .tracks)) ?? []
            let chordTrack = try legacy.decodeIfPresent(ChordTrackModel.self, forKey: .chordTrack) ?? ChordTrackModel()

            var notesByTrack: [UUID: [NoteEvent]] = [:]
            var newTracks: [Track] = []
            if legacyTracks.isEmpty {
                // very old: a flat noteEvents on one implicit track
                let flat = try legacy.decodeIfPresent([NoteEvent].self, forKey: .noteEvents) ?? []
                let t = Track(name: "melody")
                newTracks = [t]
                notesByTrack[t.id] = flat
            } else {
                for lt in legacyTracks {
                    let t = Track(id: lt.id, name: lt.name, voice: lt.voice ?? "saw", muted: lt.muted ?? false, soloed: lt.soloed ?? false)
                    newTracks.append(t)
                    notesByTrack[t.id] = lt.noteEvents ?? []
                }
            }
            tracks = newTracks
            let pattern = SongPattern(name: "pattern 1", chords: chordTrack, notesByTrack: notesByTrack)
            patterns = [pattern]
            arrangement = [pattern.id]
        }
    }
}
