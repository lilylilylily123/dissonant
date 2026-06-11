import XCTest
@testable import Dissonant

/// App-target test bundle. Most logic is tested headlessly in DissonantCore (`swift test`);
/// this target hosts UI/integration tests that need the app context (lands with U8–U9).
final class SmokeTests: XCTestCase {
    func testAppModuleLoads() {
        XCTAssertNotNil(ProjectDocument())
    }
}
