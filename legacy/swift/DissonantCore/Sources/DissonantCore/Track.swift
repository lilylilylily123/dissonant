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
    /// A drum track uses a procedural drum kit and a step grid instead of a synth + piano roll.
    public var isDrum: Bool
    // Per-track FX (each track has its own bus): gain, reverb wet (0–1), tone = high-cut Hz.
    public var volume: Double
    public var reverbSend: Double
    public var tone: Double
    public var pan: Double   // -1 (left) … 1 (right)

    public init(
        id: UUID = UUID(),
        name: String,
        voice: String = "saw",
        muted: Bool = false,
        soloed: Bool = false,
        isDrum: Bool = false,
        volume: Double = 1.0,
        reverbSend: Double = 0.0,
        tone: Double = 18_000,
        pan: Double = 0.0
    ) {
        self.id = id
        self.name = name
        self.voice = voice
        self.muted = muted
        self.soloed = soloed
        self.isDrum = isDrum
        self.volume = volume
        self.reverbSend = reverbSend
        self.tone = tone
        self.pan = pan
    }

    private enum CodingKeys: String, CodingKey {
        case id, name, voice, muted, soloed, isDrum, volume, reverbSend, tone, pan
    }

    // Tolerant decode so older files (missing newer fields) still open.
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(UUID.self, forKey: .id)
        name = try c.decode(String.self, forKey: .name)
        voice = try c.decodeIfPresent(String.self, forKey: .voice) ?? "saw"
        muted = try c.decodeIfPresent(Bool.self, forKey: .muted) ?? false
        soloed = try c.decodeIfPresent(Bool.self, forKey: .soloed) ?? false
        isDrum = try c.decodeIfPresent(Bool.self, forKey: .isDrum) ?? false
        volume = try c.decodeIfPresent(Double.self, forKey: .volume) ?? 1.0
        reverbSend = try c.decodeIfPresent(Double.self, forKey: .reverbSend) ?? 0.0
        tone = try c.decodeIfPresent(Double.self, forKey: .tone) ?? 18_000
        pan = try c.decodeIfPresent(Double.self, forKey: .pan) ?? 0.0
    }
}
