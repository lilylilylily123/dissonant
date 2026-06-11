import SwiftUI
import InKeyCore

/// Placeholder shell. U2 adds an audio smoke-test (test tone) and transport play/stop so
/// the audio path is verifiable by ear. The piano roll, chord track, and guidance UI land
/// in later units (U6–U10).
struct ContentView: View {
    @Binding var document: ProjectDocument

    @StateObject private var transport = Transport()
    @State private var audio = AudioEngineController()

    var body: some View {
        VStack(spacing: 20) {
            Text("in key")
                .font(.system(size: 48, weight: .heavy, design: .monospaced))

            Text("tempo \(Int(document.model.tempo)) bpm  ·  playhead \(transport.state.positionBeats, specifier: "%.2f")")
                .font(.system(.body, design: .monospaced))
                .foregroundStyle(.secondary)

            HStack(spacing: 16) {
                Button("▶ test tone") { audio.playTestNote() }
                Button(transport.state.isPlaying ? "⏹ stop" : "▶ play") {
                    transport.state.isPlaying ? transport.stop() : transport.play()
                }
            }
            .font(.system(.body, design: .monospaced))
        }
        .frame(minWidth: 640, minHeight: 420)
        .padding()
        .onAppear { audio.start() }
    }
}
