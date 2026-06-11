import XCTest
@testable import DissonantCore

final class ProjectPersistenceTests: XCTestCase {

    // A populated project survives an encode/decode round-trip unchanged.
    func testRoundTripPreservesAllState() throws {
        let track = Track(name: "melody", voice: "saw", muted: false, soloed: true)
        let pattern = DissonantCore.SongPattern(
            name: "verse",
            chords: ChordTrackModel(chords: [
                ChordEvent(startBeat: 0, lengthBeats: 4, pitchClasses: [0, 4, 7], name: "C")
            ]),
            notesByTrack: [track.id: [NoteEvent(startBeat: 0, lengthBeats: 1, pitch: 60)]]
        )
        let original = ProjectModel(
            tempo: 96,
            key: KeyState(rootPitchClass: 0, scale: .major, isLocked: true),
            tracks: [track],
            patterns: [pattern],
            arrangement: [pattern.id, pattern.id]
        )

        let data = try JSONEncoder().encode(original)
        let decoded = try JSONDecoder().decode(ProjectModel.self, from: data)
        XCTAssertEqual(decoded, original)
    }

    // A new/empty project has one track and one empty pattern.
    func testEmptyProjectDefaults() {
        let empty = ProjectModel.empty
        XCTAssertEqual(empty.tracks.count, 1)
        XCTAssertEqual(empty.patterns.count, 1)
        XCTAssertTrue(empty.patterns[0].notesByTrack.isEmpty)
        XCTAssertTrue(empty.patterns[0].chords.isEmpty)
        XCTAssertEqual(empty.tempo, 120)
    }

    func testDecodingMissingFieldsDoesNotThrow() throws {
        let json = #"{ "tempo": 140 }"#.data(using: .utf8)!
        let decoded = try JSONDecoder().decode(ProjectModel.self, from: json)
        XCTAssertEqual(decoded.tempo, 140)
        XCTAssertEqual(decoded.tracks.count, 1)
        XCTAssertEqual(decoded.patterns.count, 1)
    }

    func testDecodingEmptyObject() throws {
        let decoded = try JSONDecoder().decode(ProjectModel.self, from: "{}".data(using: .utf8)!)
        XCTAssertEqual(decoded.tempo, 120)
        XCTAssertEqual(decoded.tracks.count, 1)
        XCTAssertEqual(decoded.patterns.count, 1)
        XCTAssertEqual(decoded.key, .none)
    }

    // A pre-pattern (v1) file migrates: tracks-with-notes + flat chordTrack fold into one pattern.
    func testMigratesLegacyV1File() throws {
        let trackID = "11111111-1111-1111-1111-111111111111"
        let json = """
        {
          "tempo": 100,
          "tracks": [
            { "id": "\(trackID)", "name": "lead", "voice": "saw",
              "noteEvents": [ { "id": "22222222-2222-2222-2222-222222222222", "startBeat": 0, "lengthBeats": 1, "pitch": 60 } ] }
          ],
          "chordTrack": { "chords": [ { "id": "33333333-3333-3333-3333-333333333333", "startBeat": 0, "lengthBeats": 4, "pitchClasses": [0, 4, 7], "name": "C" } ] }
        }
        """.data(using: .utf8)!

        let decoded = try JSONDecoder().decode(ProjectModel.self, from: json)
        XCTAssertEqual(decoded.tempo, 100)
        XCTAssertEqual(decoded.tracks.count, 1)
        XCTAssertEqual(decoded.tracks[0].name, "lead")
        XCTAssertEqual(decoded.patterns.count, 1)
        XCTAssertEqual(decoded.patterns[0].chords.chords.first?.name, "C")
        let migratedNotes = decoded.patterns[0].notes(for: decoded.tracks[0].id)
        XCTAssertEqual(migratedNotes.count, 1)
        XCTAssertEqual(migratedNotes.first?.pitch, 60)
        XCTAssertEqual(decoded.arrangement, [decoded.patterns[0].id])
    }
}
