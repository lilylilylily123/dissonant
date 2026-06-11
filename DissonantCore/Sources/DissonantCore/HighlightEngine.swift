import Foundation

/// Produces the per-pitch-class tier map that colors the piano roll, keyed to the chord
/// under the playhead. This is the heart of the guidance experience (R10) and its
/// graceful degradation when there's no chord track yet (R12 cold-start).
///
/// Three states:
///  - **Full context** — a chord is sounding: every pitch class is classified chordTone /
///    tension / dissonance against that chord (and the key, if locked).
///  - **Cold-start (scale-level)** — no chord but a key is set: in-key pitches read as the
///    solid tier, out-of-key as dissonance. Two-tier guidance until a progression exists.
///  - **Neutral** — no chord and no key: `nil` everywhere, no guidance imposed.
public struct HighlightEngine: Sendable {
    private let classifier = TierClassifier()

    public init() {}

    /// Tier for every pitch class 0–11 at `beat`. `nil` entries mean "no guidance" (neutral).
    public func tierMap(atBeat beat: Double, chordTrack: ChordTrackModel, key: KeyState) -> [Tier?] {
        let keyPitchClasses = key.rootPitchClass.map { Harmony.scalePitchClasses(root: $0, scale: key.scale) }

        if let chord = chordTrack.chord(atBeat: beat) {
            return classifier
                .tierMap(chordPitchClasses: chord.pitchClasses, keyPitchClasses: keyPitchClasses)
                .map { Optional($0) }
        }

        if let keyPitchClasses {
            let scale = Set(keyPitchClasses)
            return (0..<12).map { scale.contains($0) ? .chordTone : .dissonance }
        }

        return Array(repeating: nil, count: 12)
    }

    /// Convenience for a single pitch (MIDI note number) — used by the roll and the
    /// "notes you could play now" readout so both read from the same source (R13).
    public func tier(forPitch pitch: Int, atBeat beat: Double, chordTrack: ChordTrackModel, key: KeyState) -> Tier? {
        tierMap(atBeat: beat, chordTrack: chordTrack, key: key)[TierClassifier.normalize(pitch)]
    }
}
