import Foundation

/// A suggestable chord, expressed as absolute pitch classes plus display labels.
/// Names are available but never required to use the chord (R9).
public struct ChordSuggestion: Equatable, Sendable {
    public var pitchClasses: [Int]
    public var name: String
    public var romanNumeral: String

    public init(pitchClasses: [Int], name: String, romanNumeral: String) {
        self.pitchClasses = pitchClasses
        self.name = name
        self.romanNumeral = romanNumeral
    }
}

/// Scale, diatonic-chord, and naming helpers. v1 covers major/minor with hand-rolled
/// interval math (dependency-free, fully testable). Richer chord types and exotic scales
/// can layer on later (the point where a theory library earns its keep).
public enum Harmony {

    static let noteNames = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]

    static let majorSteps = [0, 2, 4, 5, 7, 9, 11]
    static let minorSteps = [0, 2, 3, 5, 7, 8, 10] // natural minor

    public static func scaleSteps(_ scale: ScaleType) -> [Int] {
        scale == .major ? majorSteps : minorSteps
    }

    /// Display name of a pitch class (sharps).
    public static func noteName(_ pitchClass: Int) -> String {
        noteNames[TierClassifier.normalize(pitchClass)]
    }

    /// The seven pitch classes of a key's scale, in scale order starting from the root.
    public static func scalePitchClasses(root: Int, scale: ScaleType) -> [Int] {
        scaleSteps(scale).map { TierClassifier.normalize(root + $0) }
    }

    /// The seven diatonic triads of a key, built by stacking scale thirds.
    public static func diatonicChords(root: Int, scale: ScaleType) -> [ChordSuggestion] {
        let scalePCs = scalePitchClasses(root: root, scale: scale)
        return (0..<7).map { degree in
            let pcs = [degree, degree + 2, degree + 4].map { scalePCs[$0 % 7] }
            let quality = triadQuality(pcs)
            let rootName = noteName(pcs[0])
            return ChordSuggestion(
                pitchClasses: pcs,
                name: rootName + quality.nameSuffix,
                romanNumeral: quality.roman(forDegree: degree)
            )
        }
    }

    // MARK: - Triad quality

    enum TriadQuality {
        case major, minor, diminished, augmented, other

        var nameSuffix: String {
            switch self {
            case .major: return ""
            case .minor: return "m"
            case .diminished: return "dim"
            case .augmented: return "aug"
            case .other: return "?"
            }
        }

        func roman(forDegree degree: Int) -> String {
            let numerals = ["I", "II", "III", "IV", "V", "VI", "VII"]
            let base = numerals[degree % 7]
            switch self {
            case .major, .augmented: return base + (self == .augmented ? "+" : "")
            case .minor: return base.lowercased()
            case .diminished: return base.lowercased() + "°"
            case .other: return base + "?"
            }
        }
    }

    static func triadQuality(_ pcs: [Int]) -> TriadQuality {
        guard pcs.count == 3 else { return .other }
        let third = TierClassifier.normalize(pcs[1] - pcs[0])
        let fifth = TierClassifier.normalize(pcs[2] - pcs[0])
        switch (third, fifth) {
        case (4, 7): return .major
        case (3, 7): return .minor
        case (3, 6): return .diminished
        case (4, 8): return .augmented
        default: return .other
        }
    }
}
