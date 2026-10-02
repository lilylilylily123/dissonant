import Foundation

/// How a note relates to the chord sounding right now — the product's core mechanic.
public enum Tier: String, Sendable, Equatable, Codable {
    /// A note of the chord itself (root/3rd/5th/7th). Rock-solid.
    case chordTone
    /// In-key and at least a whole step from every chord tone. Spicy but good.
    case tension
    /// A half step from a chord tone, or out of key. Flagged — never blocked.
    case dissonance
}

/// Classifies pitch classes against a chord (and optional key). Pure integer arithmetic on
/// pitch classes 0–11 — no music-theory library needed, which keeps it trivially testable
/// and fast enough to recompute per chord boundary.
///
/// Rule (per plan KTD): chord tones win; otherwise a pitch a half step from any chord tone
/// is dissonance; an out-of-key pitch is dissonance; everything else (in-key, ≥ whole step
/// from every chord tone) is a tension.
public struct TierClassifier: Sendable {

    public init() {}

    /// Classify one pitch class against the given chord, optionally constrained to a key's scale.
    /// `chordPitchClasses` and `keyPitchClasses` may contain any integers; they're normalized to 0–11.
    public func tier(pitchClass pc: Int, chordPitchClasses chord: [Int], keyPitchClasses key: [Int]? = nil) -> Tier {
        let p = Self.normalize(pc)
        let chordSet = Set(chord.map(Self.normalize))

        if chordSet.contains(p) {
            return .chordTone
        }

        let nearestChordTone = chordSet.map { Self.semitoneDistance(p, $0) }.min() ?? 12
        if nearestChordTone == 1 {
            return .dissonance
        }

        if let key {
            let keySet = Set(key.map(Self.normalize))
            if !keySet.contains(p) {
                return .dissonance
            }
        }

        return .tension
    }

    /// Precomputed tier for every pitch class 0–11. U9 builds one of these per chord and
    /// then each note on the roll looks up its tier by `pitch % 12` — cheap recompute.
    public func tierMap(chordPitchClasses chord: [Int], keyPitchClasses key: [Int]? = nil) -> [Tier] {
        (0..<12).map { tier(pitchClass: $0, chordPitchClasses: chord, keyPitchClasses: key) }
    }

    // MARK: - Pitch-class math

    static func normalize(_ pc: Int) -> Int {
        ((pc % 12) + 12) % 12
    }

    /// Shortest distance between two pitch classes around the 12-tone circle (0–6).
    static func semitoneDistance(_ a: Int, _ b: Int) -> Int {
        let d = abs(a - b) % 12
        return min(d, 12 - d)
    }
}
