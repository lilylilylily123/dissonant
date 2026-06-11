import SwiftUI
import DissonantCore

@main
struct DissonantApp: App {
    var body: some Scene {
        DocumentGroup(newDocument: ProjectDocument(model: .starter)) { configuration in
            ContentView(document: configuration.$document)
        }
    }
}
