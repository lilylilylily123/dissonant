import Foundation

/// One drum in the kit — a display name and the MIDI note it's triggered by. Drum hits are
/// stored as ordinary `NoteEvent`s (pitch = this note), so they persist, play, and arrange
/// through the same machinery as melodic notes. The grid and the audio engine share this list.
public struct DrumVoice: Equatable, Sendable, Identifiable {
    public let name: String
    public let pitch: Int
    public var id: Int { pitch }
    public init(name: String, pitch: Int) {
        self.name = name
        self.pitch = pitch
    }
}

public enum DrumKit {
    /// General-MIDI-ish drum map. Top-to-bottom display order.
    public static let voices: [DrumVoice] = [
        DrumVoice(name: "hat",   pitch: 42),
        DrumVoice(name: "clap",  pitch: 39),
        DrumVoice(name: "snare", pitch: 38),
        DrumVoice(name: "kick",  pitch: 36)
    ]

    public static let pitches: [Int] = voices.map(\.pitch)
}
