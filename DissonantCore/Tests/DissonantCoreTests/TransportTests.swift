import XCTest
@testable import DissonantCore

final class TempoTests: XCTestCase {

    // Covers U2: beats<->seconds conversion at representative tempos.
    func testConversionAt120BPM() {
        let t = Tempo(bpm: 120)
        XCTAssertEqual(t.seconds(forBeats: 1), 0.5, accuracy: 1e-9)
        XCTAssertEqual(t.beats(forSeconds: 0.5), 1, accuracy: 1e-9)
    }

    func testConversionRoundTrips() {
        for bpm in [60.0, 90.0, 140.0, 174.0] {
            let t = Tempo(bpm: bpm)
            let beats = 3.25
            XCTAssertEqual(t.beats(forSeconds: t.seconds(forBeats: beats)), beats, accuracy: 1e-9)
        }
    }
}

final class TransportStateTests: XCTestCase {

    func testInitialState() {
        let s = TransportState()
        XCTAssertFalse(s.isPlaying)
        XCTAssertEqual(s.positionBeats, 0)
    }

    // Covers U2: stopped -> playing -> stopped transitions and position behavior.
    func testPlayStopAndAdvance() {
        var s = TransportState()
        s.advance(byBeats: 2)            // stopped: no movement
        XCTAssertEqual(s.positionBeats, 0)

        s.play()
        XCTAssertTrue(s.isPlaying)
        s.advance(byBeats: 2)
        XCTAssertEqual(s.positionBeats, 2, accuracy: 1e-9)

        s.stop()
        XCTAssertFalse(s.isPlaying)
        s.advance(byBeats: 5)            // stopped again: frozen
        XCTAssertEqual(s.positionBeats, 2, accuracy: 1e-9)
    }

    func testSeekClampsNegative() {
        var s = TransportState()
        s.seek(toBeat: -4)
        XCTAssertEqual(s.positionBeats, 0)
        s.seek(toBeat: 7.5)
        XCTAssertEqual(s.positionBeats, 7.5, accuracy: 1e-9)
    }

    // Covers U2: position wraps at the loop boundary.
    func testLoopWrap() {
        var s = TransportState(isPlaying: true, positionBeats: 7, loop: LoopRegion(startBeat: 4, endBeat: 8))
        s.advance(byBeats: 2)            // 7 + 2 = 9, past end 8 -> wrap: (9-4) % 4 = 1 -> 4 + 1 = 5
        XCTAssertEqual(s.positionBeats, 5, accuracy: 1e-9)
    }

    func testLoopWrapHandlesMultipleLengths() {
        var s = TransportState(isPlaying: true, positionBeats: 4, loop: LoopRegion(startBeat: 0, endBeat: 4))
        s.advance(byBeats: 10)           // 14 over 4-beat loop -> (14-0)%4 = 2
        XCTAssertEqual(s.positionBeats, 2, accuracy: 1e-9)
    }
}
