import SwiftUI
import InKeyCore

/// Placeholder shell for U1 — proves the document wiring builds and binds.
/// The piano roll, chord track, and guidance UI land in later units (U6–U10).
struct ContentView: View {
    @Binding var document: ProjectDocument

    var body: some View {
        VStack(spacing: 12) {
            Text("in key")
                .font(.system(size: 48, weight: .heavy, design: .monospaced))
            Text("tempo \(Int(document.model.tempo)) bpm")
                .font(.system(.body, design: .monospaced))
                .foregroundStyle(.secondary)
        }
        .frame(minWidth: 640, minHeight: 420)
        .padding()
    }
}
