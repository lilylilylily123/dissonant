import XCTest
@testable import InKeyCore

final class ProjectPersistenceTests: XCTestCase {

    // Covers R14: a populated project survives an encode/decode round-trip unchanged.
    func testRoundTripPreservesAllState() throws {
        let original = ProjectModel(
            tempo: 96,
            chordEvents: [
                ChordEvent(startBeat: 0, lengthBeats: 4, pitchClasses: [0, 4, 7], name: "C"),
                ChordEvent(startBeat: 4, lengthBeats: 4, pitchClasses: [5, 9, 0], name: "F")
            ],
            noteEvents: [
                NoteEvent(startBeat: 0, lengthBeats: 1, pitch: 60),
                NoteEvent(startBeat: 1, lengthBeats: 1, pitch: 64)
            ],
            key: KeyState(rootPitchClass: 0, scale: .major, isLocked: true)
        )

        let data = try JSONEncoder().encode(original)
        let decoded = try JSONDecoder().decode(ProjectModel.self, from: data)

        XCTAssertEqual(decoded, original)
    }

    // Covers R14: a new/empty project has sensible defaults — no key, empty tracks, default tempo.
    func testEmptyProjectDefaults() {
        let empty = ProjectModel.empty
        XCTAssertNil(empty.key.rootPitchClass)
        XCTAssertFalse(empty.key.isLocked)
        XCTAssertTrue(empty.chordEvents.isEmpty)
        XCTAssertTrue(empty.noteEvents.isEmpty)
        XCTAssertEqual(empty.tempo, 120)
    }

    // Covers R14 forward-compat guard: decoding a document missing fields does not throw;
    // absent fields take their defaults. Simulates opening an older/newer file.
    func testDecodingMissingFieldsDoesNotThrow() throws {
        // JSON with only tempo present — no schemaVersion, chords, notes, or key.
        let json = #"{ "tempo": 140 }"#.data(using: .utf8)!

        let decoded = try JSONDecoder().decode(ProjectModel.self, from: json)

        XCTAssertEqual(decoded.tempo, 140)
        XCTAssertEqual(decoded.schemaVersion, ProjectModel.currentSchemaVersion)
        XCTAssertTrue(decoded.chordEvents.isEmpty)
        XCTAssertTrue(decoded.noteEvents.isEmpty)
        XCTAssertEqual(decoded.key, .none)
    }

    // Empty object decodes to a fully-default project.
    func testDecodingEmptyObject() throws {
        let json = "{}".data(using: .utf8)!
        let decoded = try JSONDecoder().decode(ProjectModel.self, from: json)
        XCTAssertEqual(decoded, .empty)
    }
}
