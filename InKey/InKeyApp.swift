import SwiftUI

@main
struct InKeyApp: App {
    var body: some Scene {
        DocumentGroup(newDocument: ProjectDocument()) { configuration in
            ContentView(document: configuration.$document)
        }
    }
}
