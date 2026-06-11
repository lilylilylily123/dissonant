import SwiftUI
import DissonantCore

/// The main window: the guidance experience. A guided chord lane drives the live tier
/// highlighting on a piano roll; the playable-now readout mirrors it. Play sweeps the
/// playhead, recoloring the roll per chord and sounding the backing bed.
struct ContentView: View {
    @Binding var document: ProjectDocument

    @StateObject private var transport = Transport()
    @State private var audio = AudioEngineController()
    @State private var playback: ChordPlayback?

    @State private var chordTrack = ChordTrackModel(chords: [
        ChordEvent(startBeat: 0, lengthBeats: 4, pitchClasses: [0, 4, 7], name: "C"),
        ChordEvent(startBeat: 4, lengthBeats: 4, pitchClasses: [5, 9, 0], name: "F"),
        ChordEvent(startBeat: 8, lengthBeats: 4, pitchClasses: [7, 11, 2], name: "G"),
        ChordEvent(startBeat: 12, lengthBeats: 4, pitchClasses: [9, 0, 4], name: "Am")
    ])

    // Key inference wiring lands in U10; cold-start neutral for now.
    @State private var key: KeyState = .none

    private var playhead: Double { transport.state.positionBeats }
    private var currentChordName: String { chordTrack.chord(atBeat: playhead)?.name ?? "—" }

    var body: some View {
        ZStack {
            Theme.surface.ignoresSafeArea()
            VStack(alignment: .leading, spacing: 14) {
                header
                ChordLaneView(chordTrack: $chordTrack, playheadBeat: playhead)
                PianoRollView(
                    notes: $document.model.noteEvents,
                    chordTrack: chordTrack,
                    key: key,
                    playheadBeat: playhead,
                    onAudition: { pitch in audio.playTestNote(UInt8(clamping: pitch)) }
                )
                PlayableNowView(chordTrack: chordTrack, key: key, playheadBeat: playhead)
                Spacer(minLength: 0)
            }
            .padding(18)
        }
        .frame(minWidth: 860, minHeight: 660)
        .onAppear {
            audio.start()
            playback = ChordPlayback(instrument: audio.instrument)
        }
        .onChange(of: transport.state.positionBeats) { _, beat in
            if transport.state.isPlaying { playback?.update(forBeat: beat, in: chordTrack) }
        }
    }

    private var header: some View {
        HStack(alignment: .firstTextBaseline, spacing: 18) {
            Text("dissonant")
                .font(.custom(Theme.mono, size: 26)).bold()
                .foregroundStyle(Theme.ink)

            Text("\(Int(transport.tempo.bpm)) bpm")
                .font(.custom(Theme.mono, size: 12)).foregroundStyle(Theme.faded)

            Text("chord \(currentChordName)")
                .font(.custom(Theme.mono, size: 14)).foregroundStyle(Theme.brand)

            Spacer()

            Button("test tone") { audio.playTestNote() }
                .buttonStyle(.plain)
                .font(.custom(Theme.mono, size: 12)).foregroundStyle(Theme.faded)

            Button(transport.state.isPlaying ? "⏹ stop" : "▶ play") {
                if transport.state.isPlaying {
                    transport.stop(); playback?.releaseAll()
                } else {
                    transport.play()
                }
            }
            .buttonStyle(.plain)
            .font(.custom(Theme.mono, size: 14)).bold()
            .foregroundStyle(Theme.surface)
            .padding(.horizontal, 12).padding(.vertical, 6)
            .background(Theme.brand)
            .clipShape(RoundedRectangle(cornerRadius: 5))
        }
    }
}
