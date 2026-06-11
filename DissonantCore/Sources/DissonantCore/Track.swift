import Foundation

/// One instrument track: a name, a voice, and its own notes. Tracks layer together on the
/// shared timeline — the melody, a bass, a lead, etc. `voice` is a `VoiceKind` raw value
/// (synth presets persist; a hosted Audio Unit is applied at runtime and not saved here).
public struct Track: Codable, Equatable, Identifiable, Sendable {
    public var id: UUID
    public var name: String
    public var voice: String
    public var noteEvents: [NoteEvent]

    public init(id: UUID = UUID(), name: String, voice: String = "keys", noteEvents: [NoteEvent] = []) {
        self.id = id
        self.name = name
        self.voice = voice
        self.noteEvents = noteEvents
    }
}
