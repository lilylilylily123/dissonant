import SwiftUI
import DissonantCore

/// The main window: the guidance experience. The chord progression drives the live tier
/// highlighting; by default you don't *hear* the chords — you see which notes fit. Play
/// sweeps the playhead and recolors the roll per chord. Notes you place are audible.
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
    // The chord progression is guidance context — off by default, opt-in to hear it.
    @State private var hearChords = false

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
        .frame(minWidth: 860, minHeight: 680)
        .onAppear {
            audio.start()
            playback = ChordPlayback(instrument: audio.instrument)
        }
        .onChange(of: transport.state.positionBeats) { _, beat in
            if transport.state.isPlaying && hearChords {
                playback?.update(forBeat: beat, in: chordTrack)
            }
        }
        .onChange(of: hearChords) { _, on in
            if !on { playback?.releaseAll() }
        }
    }

    private func togglePlay() {
        if transport.state.isPlaying {
            transport.stop(); playback?.releaseAll()
        } else {
            transport.play()
        }
    }

    private func rewind() {
        transport.rewind(); playback?.releaseAll()
    }

    private var header: some View {
        HStack(alignment: .firstTextBaseline, spacing: 16) {
            Text("dissonant")
                .font(.custom(Theme.mono, size: 26)).bold()
                .foregroundStyle(Theme.ink)
            Text("\(Int(transport.tempo.bpm)) bpm")
                .font(.custom(Theme.mono, size: 12)).foregroundStyle(Theme.faded)
            Text("chord \(currentChordName)")
                .font(.custom(Theme.mono, size: 14)).foregroundStyle(Theme.brand)

            Spacer()

            // transport
            ctrlButton("⏮") { rewind() }
                .keyboardShortcut("r", modifiers: [])
            Button(transport.state.isPlaying ? "⏹ stop" : "▶ play") { togglePlay() }
                .buttonStyle(.plain)
                .font(.custom(Theme.mono, size: 14)).bold()
                .foregroundStyle(Theme.surface)
                .padding(.horizontal, 12).padding(.vertical, 6)
                .background(Theme.brand)
                .clipShape(RoundedRectangle(cornerRadius: 5))
                .keyboardShortcut(.space, modifiers: [])

            // utilities
            ctrlButton(hearChords ? "♪ chords on" : "♪ chords off") { hearChords.toggle() }
                .foregroundStyle(hearChords ? Theme.brand : Theme.faded)
            ctrlButton("clear") { document.model.noteEvents.removeAll() }
            ctrlButton("test tone") { audio.playTestNote() }
        }
    }

    private func ctrlButton(_ label: String, _ action: @escaping () -> Void) -> some View {
        Button(label, action: action)
            .buttonStyle(.plain)
            .font(.custom(Theme.mono, size: 12))
            .foregroundStyle(Theme.faded)
            .padding(.horizontal, 8).padding(.vertical, 5)
            .background(Theme.panel)
            .clipShape(RoundedRectangle(cornerRadius: 4))
    }
}
