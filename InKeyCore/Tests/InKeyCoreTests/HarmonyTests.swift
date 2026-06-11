import XCTest
@testable import InKeyCore

final class HarmonyTests: XCTestCase {

    func testMajorScalePitchClasses() {
        XCTAssertEqual(Harmony.scalePitchClasses(root: 0, scale: .major), [0, 2, 4, 5, 7, 9, 11]) // C major
        XCTAssertEqual(Harmony.scalePitchClasses(root: 7, scale: .major), [7, 9, 11, 0, 2, 4, 6]) // G major
    }

    func testMinorScalePitchClasses() {
        XCTAssertEqual(Harmony.scalePitchClasses(root: 9, scale: .minor), [9, 11, 0, 2, 4, 5, 7]) // A minor
    }

    // Diatonic triads of C major: C Dm Em F G Am Bdim.
    func testDiatonicChordsCMajor() {
        let chords = Harmony.diatonicChords(root: 0, scale: .major)
        XCTAssertEqual(chords.map(\.name), ["C", "Dm", "Em", "F", "G", "Am", "Bdim"])
        XCTAssertEqual(chords.map(\.romanNumeral), ["I", "ii", "iii", "IV", "V", "vi", "vii°"])
        XCTAssertEqual(chords[0].pitchClasses, [0, 4, 7])  // C E G
        XCTAssertEqual(chords[1].pitchClasses, [2, 5, 9])  // D F A
        XCTAssertEqual(chords[6].pitchClasses, [11, 2, 5]) // B D F
    }

    // Diatonic triads of A minor: Am Bdim C Dm Em F G.
    func testDiatonicChordsAMinor() {
        let chords = Harmony.diatonicChords(root: 9, scale: .minor)
        XCTAssertEqual(chords.map(\.name), ["Am", "Bdim", "C", "Dm", "Em", "F", "G"])
    }

    func testNoteNames() {
        XCTAssertEqual(Harmony.noteName(0), "C")
        XCTAssertEqual(Harmony.noteName(6), "F#")
        XCTAssertEqual(Harmony.noteName(13), "C#") // normalizes
    }
}
