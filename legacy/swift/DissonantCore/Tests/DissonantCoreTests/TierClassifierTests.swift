import XCTest
@testable import DissonantCore

final class TierClassifierTests: XCTestCase {

    let c = TierClassifier()
    // Pitch classes: C=0 D=2 E=4 F=5 G=7 A=9 B=11
    let cMajorTriad = [0, 4, 7]
    let cMajor7 = [0, 4, 7, 11]
    let cMajorScale = [0, 2, 4, 5, 7, 9, 11]

    // Covers R10: chord tones, tensions, and dissonance against a bare C major triad.
    func testCMajorTriadNoKey() {
        XCTAssertEqual(c.tier(pitchClass: 0, chordPitchClasses: cMajorTriad), .chordTone) // C
        XCTAssertEqual(c.tier(pitchClass: 4, chordPitchClasses: cMajorTriad), .chordTone) // E
        XCTAssertEqual(c.tier(pitchClass: 7, chordPitchClasses: cMajorTriad), .chordTone) // G
        XCTAssertEqual(c.tier(pitchClass: 2, chordPitchClasses: cMajorTriad), .tension)   // D
        XCTAssertEqual(c.tier(pitchClass: 9, chordPitchClasses: cMajorTriad), .tension)   // A
        XCTAssertEqual(c.tier(pitchClass: 5, chordPitchClasses: cMajorTriad), .dissonance) // F (½ above E)
    }

    // Covers R10: F over Cmaj7 is dissonant (♭9 over the 3rd); B becomes a chord tone.
    func testCMajor7() {
        XCTAssertEqual(c.tier(pitchClass: 11, chordPitchClasses: cMajor7), .chordTone)   // B
        XCTAssertEqual(c.tier(pitchClass: 2, chordPitchClasses: cMajor7), .tension)      // D
        XCTAssertEqual(c.tier(pitchClass: 5, chordPitchClasses: cMajor7), .dissonance)   // F
    }

    func testMinorAndDominantChordTones() {
        let aMinor = [9, 0, 4]      // A C E
        XCTAssertEqual(c.tier(pitchClass: 9, chordPitchClasses: aMinor), .chordTone)
        XCTAssertEqual(c.tier(pitchClass: 0, chordPitchClasses: aMinor), .chordTone)
        XCTAssertEqual(c.tier(pitchClass: 4, chordPitchClasses: aMinor), .chordTone)

        let g7 = [7, 11, 2, 5]      // G B D F
        for pc in g7 {
            XCTAssertEqual(c.tier(pitchClass: pc, chordPitchClasses: g7), .chordTone)
        }
    }

    // Covers R10: the key filter turns an otherwise-tension into dissonance when out of key.
    func testInKeyFilter() {
        // F# (6) is a whole step from G(7) and E... distance to chord tones of C triad:
        // to G(7)=1 -> actually ½ step, so dissonance regardless. Use D#(3) instead:
        // D#(3): to E(4)=1 -> dissonance by the half-step rule anyway.
        // Use A#(10): to G(7)=3, to C(0)=2, to E(4)=... min 2 -> tension without key,
        // but A# is not in C major -> dissonance with key.
        XCTAssertEqual(c.tier(pitchClass: 10, chordPitchClasses: cMajorTriad), .tension)
        XCTAssertEqual(c.tier(pitchClass: 10, chordPitchClasses: cMajorTriad, keyPitchClasses: cMajorScale), .dissonance)
    }

    // Every pitch class resolves to exactly one tier — the classifier is total.
    func testTotalFunctionOverAllPitchClasses() {
        let map = c.tierMap(chordPitchClasses: cMajorTriad)
        XCTAssertEqual(map.count, 12)
    }

    // Out-of-range and negative pitch classes normalize correctly.
    func testNormalizationOfOutOfRangePitchClasses() {
        XCTAssertEqual(c.tier(pitchClass: 12, chordPitchClasses: cMajorTriad), .chordTone)  // 12 -> C
        XCTAssertEqual(c.tier(pitchClass: -12, chordPitchClasses: cMajorTriad), .chordTone) // -12 -> C
        XCTAssertEqual(c.tier(pitchClass: 17, chordPitchClasses: cMajorTriad), .dissonance) // 17 -> F
    }
}
