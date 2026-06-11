import XCTest
@testable import DissonantCore

final class HighlightEngineTests: XCTestCase {

    let engine = HighlightEngine()

    let track = ChordTrackModel(chords: [
        ChordEvent(startBeat: 0, lengthBeats: 4, pitchClasses: [0, 4, 7], name: "C"),  // C E G
        ChordEvent(startBeat: 4, lengthBeats: 4, pitchClasses: [5, 9, 0], name: "F")   // F A C
    ])

    // Covers R10: full per-chord context matches the classifier for the chord at the beat.
    func testFullContextMatchesChord() {
        let map = engine.tierMap(atBeat: 0, chordTrack: track, key: .none)
        XCTAssertEqual(map[0], .chordTone) // C
        XCTAssertEqual(map[4], .chordTone) // E
        XCTAssertEqual(map[5], .dissonance) // F is a half step above E
    }

    // Covers R10: crossing a chord boundary recolors a fixed pitch class.
    func testTierChangesAcrossChordBoundary() {
        // E (4): chord tone of C, but over F (F A C) it's neither chord tone nor half-step...
        let overC = engine.tierMap(atBeat: 1, chordTrack: track, key: .none)[4]
        let overF = engine.tierMap(atBeat: 5, chordTrack: track, key: .none)[4]
        XCTAssertEqual(overC, .chordTone)
        XCTAssertNotEqual(overC, overF) // E re-tiers when the chord changes
    }

    // Covers R12: no chord + locked key -> scale-level (in-key solid, out-of-key dissonance).
    func testColdStartScaleLevel() {
        let key = KeyState(rootPitchClass: 0, scale: .major, isLocked: true) // C major
        let empty = ChordTrackModel()
        let map = engine.tierMap(atBeat: 0, chordTrack: empty, key: key)
        XCTAssertEqual(map[0], .chordTone)   // C in key
        XCTAssertEqual(map[2], .chordTone)   // D in key
        XCTAssertEqual(map[1], .dissonance)  // C# out of key
    }

    // Covers R12: no chord + no key -> neutral (no guidance).
    func testNeutralWhenNoChordNoKey() {
        let map = engine.tierMap(atBeat: 0, chordTrack: ChordTrackModel(), key: .none)
        XCTAssertEqual(map, Array(repeating: Tier?.none, count: 12))
    }

    // Covers R13: the single-pitch readout agrees with the full map (one source of truth).
    func testSinglePitchAgreesWithMap() {
        let beat = 5.0
        let map = engine.tierMap(atBeat: beat, chordTrack: track, key: .none)
        for midi in [60, 64, 65, 67] {
            XCTAssertEqual(engine.tier(forPitch: midi, atBeat: beat, chordTrack: track, key: .none),
                           map[midi % 12])
        }
    }
}
