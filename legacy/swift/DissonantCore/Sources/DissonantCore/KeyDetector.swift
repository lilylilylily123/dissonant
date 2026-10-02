import Foundation

/// One ranked key guess.
public struct KeyCandidate: Equatable, Sendable {
    public var rootPitchClass: Int
    public var scale: ScaleType
    public var score: Double

    public init(rootPitchClass: Int, scale: ScaleType, score: Double) {
        self.rootPitchClass = rootPitchClass
        self.scale = scale
        self.score = score
    }
}

/// Result of a detection pass: candidates ranked best-first, plus whether the guess is
/// confident enough to *suggest* a lock. Below the note threshold (or with a thin margin
/// over the runner-up) it is deliberately not confident — the engine suggests, the user locks.
public struct KeyDetectionResult: Equatable, Sendable {
    public var candidates: [KeyCandidate]
    public var isConfident: Bool

    public var top: KeyCandidate? { candidates.first }

    public init(candidates: [KeyCandidate], isConfident: Bool) {
        self.candidates = candidates
        self.isConfident = isConfident
    }
}

/// Krumhansl–Schmuckler key finding: build a 12-bin pitch-class histogram and Pearson-correlate
/// it against all 24 major/minor key profiles, ranking by correlation. Accurate on lots of notes,
/// unreliable on a handful — so confidence gates on note count and the top-two margin, and the
/// caller must never auto-lock an unconfident result.
public struct KeyDetector: Sendable {

    /// Minimum number of placed notes before a result can be confident (~10–12).
    public var minNotesForConfidence: Int
    /// Minimum correlation margin between the top two candidates.
    public var minScoreGap: Double

    public init(minNotesForConfidence: Int = 10, minScoreGap: Double = 0.04) {
        self.minNotesForConfidence = minNotesForConfidence
        self.minScoreGap = minScoreGap
    }

    // Krumhansl–Kessler profiles.
    static let majorProfile = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88]
    static let minorProfile = [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17]

    public func detect(pitchClasses: [Int]) -> KeyDetectionResult {
        guard !pitchClasses.isEmpty else {
            return KeyDetectionResult(candidates: [], isConfident: false)
        }

        var histogram = [Double](repeating: 0, count: 12)
        for pc in pitchClasses {
            histogram[TierClassifier.normalize(pc)] += 1
        }

        var candidates: [KeyCandidate] = []
        for root in 0..<12 {
            for scale in [ScaleType.major, ScaleType.minor] {
                let profile = rotatedProfile(scale, root: root)
                let score = Self.pearson(histogram, profile)
                candidates.append(KeyCandidate(rootPitchClass: root, scale: scale, score: score))
            }
        }
        candidates.sort { $0.score > $1.score }

        let gap = candidates.count >= 2 ? candidates[0].score - candidates[1].score : 1.0
        let confident = pitchClasses.count >= minNotesForConfidence && gap >= minScoreGap

        return KeyDetectionResult(candidates: candidates, isConfident: confident)
    }

    // MARK: - Helpers

    func rotatedProfile(_ scale: ScaleType, root: Int) -> [Double] {
        let base = scale == .major ? Self.majorProfile : Self.minorProfile
        return (0..<12).map { base[TierClassifier.normalize($0 - root)] }
    }

    static func pearson(_ x: [Double], _ y: [Double]) -> Double {
        let n = Double(x.count)
        let mx = x.reduce(0, +) / n
        let my = y.reduce(0, +) / n
        var num = 0.0, dx2 = 0.0, dy2 = 0.0
        for i in 0..<x.count {
            let a = x[i] - mx
            let b = y[i] - my
            num += a * b
            dx2 += a * a
            dy2 += b * b
        }
        let den = (dx2 * dy2).squareRoot()
        return den == 0 ? 0 : num / den
    }
}
