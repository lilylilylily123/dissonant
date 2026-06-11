import Foundation

/// One instrument track: a name, a voice, and mute/solo state. Notes live in patterns
/// (keyed by track id), so a track is reused across every pattern. `voice` is a `VoiceKind`
/// raw value (synth presets persist; a hosted Audio Unit is applied at runtime, not saved).
public struct Track: Codable, Equatable, Identifiable, Sendable {
    public var id: UUID
    public var name: String
    public var voice: String
    public var muted: Bool
    public var soloed: Bool

    public init(
        id: UUID = UUID(),
        name: String,
        voice: String = "saw",
        muted: Bool = false,
        soloed: Bool = false
    ) {
        self.id = id
        self.name = name
        self.voice = voice
        self.muted = muted
        self.soloed = soloed
    }

    private enum CodingKeys: String, CodingKey {
        case id, name, voice, muted, soloed
    }

    // Tolerant decode so older files (no mute/solo, or a stale noteEvents key) still open.
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(UUID.self, forKey: .id)
        name = try c.decode(String.self, forKey: .name)
        voice = try c.decodeIfPresent(String.self, forKey: .voice) ?? "saw"
        muted = try c.decodeIfPresent(Bool.self, forKey: .muted) ?? false
        soloed = try c.decodeIfPresent(Bool.self, forKey: .soloed) ?? false
    }
}
