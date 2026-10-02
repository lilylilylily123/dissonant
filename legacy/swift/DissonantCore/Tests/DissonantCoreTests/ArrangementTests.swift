import XCTest
@testable import DissonantCore

final class ArrangementTests: XCTestCase {

    let trackA = UUID()

    func patternA() -> DissonantCore.SongPattern {
        DissonantCore.SongPattern(name: "A", lengthBeats: 16,
                chords: ChordTrackModel(chords: [ChordEvent(startBeat: 0, lengthBeats: 8, pitchClasses: [0, 4, 7], name: "C")]),
                notesByTrack: [trackA: [NoteEvent(startBeat: 2, lengthBeats: 1, pitch: 60)]])
    }
    func patternB() -> DissonantCore.SongPattern {
        DissonantCore.SongPattern(name: "B", lengthBeats: 16,
                chords: ChordTrackModel(chords: [ChordEvent(startBeat: 0, lengthBeats: 8, pitchClasses: [5, 9, 0], name: "F")]),
                notesByTrack: [trackA: [NoteEvent(startBeat: 4, lengthBeats: 1, pitch: 64)]])
    }

    func testTotalLength() {
        let a = patternA(), b = patternB()
        let len = Arrangement.totalLength(patterns: [a, b], arrangement: [a.id, b.id, a.id])
        XCTAssertEqual(len, 48) // 3 × 16
    }

    func testFlattenedNotesOffsetByPatternLength() {
        let a = patternA(), b = patternB()
        let notes = Arrangement.flattenedNotes(trackID: trackA, patterns: [a, b], arrangement: [a.id, b.id])
        // A's note at beat 2, then B's note at beat 4 + 16 offset = 20.
        XCTAssertEqual(notes.map { $0.startBeat }.sorted(), [2, 20])
        XCTAssertEqual(notes.map { $0.pitch }.sorted(), [60, 64])
    }

    func testFlattenedChordsOffset() {
        let a = patternA(), b = patternB()
        let chords = Arrangement.flattenedChords(patterns: [a, b], arrangement: [a.id, b.id])
        XCTAssertEqual(chords.chord(atBeat: 0)?.name, "C")
        XCTAssertEqual(chords.chord(atBeat: 16)?.name, "F") // B starts at 16
    }

    func testRepeatedPatternDuplicatesNotes() {
        let a = patternA()
        let notes = Arrangement.flattenedNotes(trackID: trackA, patterns: [a], arrangement: [a.id, a.id])
        XCTAssertEqual(notes.map { $0.startBeat }.sorted(), [2, 18]) // beat 2, then 2 + 16
    }

    func testEmptyArrangement() {
        let a = patternA()
        XCTAssertEqual(Arrangement.totalLength(patterns: [a], arrangement: []), 0)
        XCTAssertTrue(Arrangement.flattenedNotes(trackID: trackA, patterns: [a], arrangement: []).isEmpty)
    }
}
