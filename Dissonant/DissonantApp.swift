import SwiftUI

@main
struct DissonantApp: App {
    var body: some Scene {
        DocumentGroup(newDocument: ProjectDocument()) { configuration in
            ContentView(document: configuration.$document)
        }
    }
}
