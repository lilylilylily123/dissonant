import XCTest
@testable import DissonantCore

final class ChordTrackModelTests: XCTestCase {

    func makeTrack() -> ChordTrackModel {
        ChordTrackModel(chords: [
            ChordEvent(startBeat: 0, lengthBeats: 4, pitchClasses: [0, 4, 7], name: "C"),
            ChordEvent(startBeat: 4, lengthBeats: 4, pitchClasses: [5, 9, 0], name: "F")
        ])
    }

    // Covers R10: half-open intervals — the boundary beat belongs to the next chord.
    func testChordAtBeatBoundaries() {
        let t = makeTrack()
        XCTAssertEqual(t.chord(atBeat: 0)?.name, "C")
        XCTAssertEqual(t.chord(atBeat: 3.99)?.name, "C")
        XCTAssertEqual(t.chord(atBeat: 4)?.name, "F")     // boundary -> next chord
        XCTAssertEqual(t.chord(atBeat: 7.99)?.name, "F")
        XCTAssertNil(t.chord(atBeat: 8))                  // past the end
    }

    // Covers R12 cold-start trigger: empty track returns nil everywhere.
    func testEmptyTrackReturnsNil() {
        let t = ChordTrackModel()
        XCTAssertTrue(t.isEmpty)
        XCTAssertNil(t.chord(atBeat: 0))
        XCTAssertNil(t.chord(atBeat: 100))
    }

    func testGapReturnsNil() {
        let t = ChordTrackModel(chords: [
            ChordEvent(startBeat: 0, lengthBeats: 4, pitchClasses: [0, 4, 7], name: "C"),
            ChordEvent(startBeat: 8, lengthBeats: 4, pitchClasses: [7, 11, 2], name: "G")
        ])
        XCTAssertNil(t.chord(atBeat: 5)) // in the gap
        XCTAssertEqual(t.chord(atBeat: 9)?.name, "G")
    }

    func testOverlapLaterStartingWins() {
        let t = ChordTrackModel(chords: [
            ChordEvent(startBeat: 0, lengthBeats: 8, pitchClasses: [0, 4, 7], name: "C"),
            ChordEvent(startBeat: 4, lengthBeats: 4, pitchClasses: [5, 9, 0], name: "F")
        ])
        XCTAssertEqual(t.chord(atBeat: 5)?.name, "F") // both contain 5; later start wins
        XCTAssertEqual(t.chord(atBeat: 2)?.name, "C")
    }

    func testAddKeepsSortedAndRemoveWorks() {
        var t = ChordTrackModel()
        let g = ChordEvent(startBeat: 8, lengthBeats: 4, pitchClasses: [7, 11, 2], name: "G")
        t.add(g)
        t.add(ChordEvent(startBeat: 0, lengthBeats: 4, pitchClasses: [0, 4, 7], name: "C"))
        XCTAssertEqual(t.chords.map(\.name), ["C", "G"]) // sorted by start despite add order

        t.remove(id: g.id)
        XCTAssertEqual(t.chords.map(\.name), ["C"])
    }

    func testUpdateMovesChord() {
        var t = makeTrack()
        var f = t.chords[1]
        f.startBeat = 1 // move F before C's end
        t.update(f)
        XCTAssertEqual(t.chords.map(\.name), ["C", "F"]) // C starts 0, F now 1 -> still C,F order
        XCTAssertEqual(t.chord(atBeat: 2)?.name, "F")    // F now covers beat 2 (later start wins)
    }
}
