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
    public var chordTrack: ChordTrackModel
    public var tracks: [Track]
    public var key: KeyState

    public init(
        schemaVersion: Int = ProjectModel.currentSchemaVersion,
        tempo: Double = 120,
        chordTrack: ChordTrackModel = ChordTrackModel(),
        tracks: [Track] = [Track(name: "melody")],
        key: KeyState = .none
    ) {
        self.schemaVersion = schemaVersion
        self.tempo = tempo
        self.chordTrack = chordTrack
        self.tracks = tracks
        self.key = key
    }

    public static let currentSchemaVersion = 1

    /// A brand-new, empty project: no key, no chords, no notes, default tempo.
    public static let empty = ProjectModel()

    /// A new project pre-seeded with a I–IV–V–vi progression so there's guidance to play with
    /// immediately. The user can clear or change it.
    public static let starter = ProjectModel(chordTrack: ChordTrackModel(chords: [
        ChordEvent(startBeat: 0, lengthBeats: 4, pitchClasses: [0, 4, 7], name: "C"),
        ChordEvent(startBeat: 4, lengthBeats: 4, pitchClasses: [5, 9, 0], name: "F"),
        ChordEvent(startBeat: 8, lengthBeats: 4, pitchClasses: [7, 11, 2], name: "G"),
        ChordEvent(startBeat: 12, lengthBeats: 4, pitchClasses: [9, 0, 4], name: "Am")
    ]))

    private enum CodingKeys: String, CodingKey {
        case schemaVersion, tempo, chordTrack, tracks, key
    }

    // Legacy single-track field, read when migrating older files.
    private enum LegacyKeys: String, CodingKey {
        case noteEvents
    }

    // Custom decode so a file missing any key falls back to a default rather than throwing,
    // and so pre-multitrack files (a flat `noteEvents`) migrate into a single melody track.
    // (encode(to:) is synthesized and uses CodingKeys, which no longer includes noteEvents.)
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        schemaVersion = try c.decodeIfPresent(Int.self, forKey: .schemaVersion) ?? ProjectModel.currentSchemaVersion
        tempo = try c.decodeIfPresent(Double.self, forKey: .tempo) ?? 120
        chordTrack = try c.decodeIfPresent(ChordTrackModel.self, forKey: .chordTrack) ?? ChordTrackModel()
        key = try c.decodeIfPresent(KeyState.self, forKey: .key) ?? .none

        if let tracks = try c.decodeIfPresent([Track].self, forKey: .tracks), !tracks.isEmpty {
            self.tracks = tracks
        } else {
            let legacy = try decoder.container(keyedBy: LegacyKeys.self)
            let notes = try legacy.decodeIfPresent([NoteEvent].self, forKey: .noteEvents) ?? []
            self.tracks = [Track(name: "melody", noteEvents: notes)]
        }
    }
}
