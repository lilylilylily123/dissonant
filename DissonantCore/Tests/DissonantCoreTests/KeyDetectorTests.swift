import XCTest
@testable import DissonantCore

final class KeyDetectorTests: XCTestCase {

    let detector = KeyDetector()

    func score(_ result: KeyDetectionResult, root: Int, scale: ScaleType) -> Double {
        result.candidates.first { $0.rootPitchClass == root && $0.scale == scale }?.score ?? -.infinity
    }

    // A full C-major scale ranks C major first.
    func testFullCMajorScaleRanksCMajor() {
        let result = detector.detect(pitchClasses: [0, 2, 4, 5, 7, 9, 11])
        XCTAssertEqual(result.top?.rootPitchClass, 0)
        XCTAssertEqual(result.top?.scale, .major)
    }

    // An A-tonic-weighted minor passage scores A minor above C major (same notes, different tonic).
    func testAMinorScoresAboveCMajor() {
        // Heavy A, with the A-minor triad (A C E) and minor scale tones present.
        let pcs = [9, 9, 9, 9, 0, 0, 4, 4, 7, 11, 2, 5]
        let result = detector.detect(pitchClasses: pcs)
        XCTAssertGreaterThan(score(result, root: 9, scale: .minor),
                             score(result, root: 0, scale: .major))
    }

    // Sparse input is never confident — proves the engine won't auto-lock (R2).
    func testSparseInputIsLowConfidence() {
        let result = detector.detect(pitchClasses: [0, 4, 7])
        XCTAssertFalse(result.isConfident)
        XCTAssertFalse(result.candidates.isEmpty) // still produces a ranked guess to show
    }

    // Empty input yields no candidate rather than a garbage argmax.
    func testEmptyInputNoCandidate() {
        let result = detector.detect(pitchClasses: [])
        XCTAssertTrue(result.candidates.isEmpty)
        XCTAssertNil(result.top)
        XCTAssertFalse(result.isConfident)
    }

    // Enough notes with a clear winner becomes confident — the threshold works both directions.
    func testEnoughNotesWithClearKeyIsConfident() {
        let twoOctavesOfCMajor = [0, 2, 4, 5, 7, 9, 11, 0, 2, 4, 5, 7, 9, 11]
        let result = detector.detect(pitchClasses: twoOctavesOfCMajor)
        XCTAssertTrue(result.isConfident)
        XCTAssertEqual(result.top?.rootPitchClass, 0)
        XCTAssertEqual(result.top?.scale, .major)
    }
}
