import SwiftUI
import DissonantCore

/// Placeholder shell. U2 added the audio smoke-test + transport; U5 adds an audible chord
/// bed driven by the playhead. The piano roll and guidance UI land in later units (U6–U10).
struct ContentView: View {
    @Binding var document: ProjectDocument

    @StateObject private var transport = Transport()
    @State private var audio = AudioEngineController()
    @State private var playback: ChordPlayback?

    // A default I–IV–V–vi progression in C so "play" demonstrates the backing bed (U5).
    @State private var chordTrack = ChordTrackModel(chords: [
        ChordEvent(startBeat: 0, lengthBeats: 4, pitchClasses: [0, 4, 7], name: "C"),
        ChordEvent(startBeat: 4, lengthBeats: 4, pitchClasses: [5, 9, 0], name: "F"),
        ChordEvent(startBeat: 8, lengthBeats: 4, pitchClasses: [7, 11, 2], name: "G"),
        ChordEvent(startBeat: 12, lengthBeats: 4, pitchClasses: [9, 0, 4], name: "Am")
    ])

    private var currentChordName: String {
        chordTrack.chord(atBeat: transport.state.positionBeats)?.name ?? "—"
    }

    var body: some View {
        VStack(spacing: 20) {
            Text("dissonant")
                .font(.system(size: 48, weight: .heavy, design: .monospaced))

            Text("tempo \(Int(document.model.tempo)) bpm  ·  playhead \(transport.state.positionBeats, specifier: "%.2f")")
                .font(.system(.body, design: .monospaced))
                .foregroundStyle(.secondary)

            Text("chord  \(currentChordName)")
                .font(.system(size: 28, weight: .bold, design: .monospaced))

            HStack(spacing: 16) {
                Button("▶ test tone") { audio.playTestNote() }
                Button(transport.state.isPlaying ? "⏹ stop" : "▶ play") {
                    if transport.state.isPlaying {
                        transport.stop()
                        playback?.releaseAll()
                    } else {
                        transport.play()
                    }
                }
            }
            .font(.system(.body, design: .monospaced))
        }
        .frame(minWidth: 640, minHeight: 420)
        .padding()
        .onAppear {
            audio.start()
            playback = ChordPlayback(instrument: audio.instrument)
        }
        .onChange(of: transport.state.positionBeats) { _, beat in
            if transport.state.isPlaying {
                playback?.update(forBeat: beat, in: chordTrack)
            }
        }
    }
}
