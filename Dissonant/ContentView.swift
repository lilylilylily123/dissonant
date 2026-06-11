import SwiftUI
import DissonantCore

/// The main window: the guidance experience. The chord progression drives the live tier
/// highlighting; by default you don't *hear* the chords — you see which notes fit. All
/// content (chords, notes, key) lives in the document, so it saves and reopens (U11).
struct ContentView: View {
    @Binding var document: ProjectDocument

    @StateObject private var transport = Transport()
    @State private var audio = AudioEngineController()
    @State private var playback: ChordPlayback?
    @State private var notePlayback: NotePlayback?

    @State private var hearChords = false
    @State private var showLandscape = false

    private var playhead: Double { transport.state.positionBeats }
    private var currentChordName: String { document.model.chordTrack.chord(atBeat: playhead)?.name ?? "—" }

    var body: some View {
        ZStack {
            Theme.surface.ignoresSafeArea()
            VStack(alignment: .leading, spacing: 14) {
                header
                ChordLaneView(chordTrack: $document.model.chordTrack, playheadBeat: playhead)
                PianoRollView(
                    notes: $document.model.noteEvents,
                    chordTrack: document.model.chordTrack,
                    key: document.model.key,
                    playheadBeat: playhead,
                    onAudition: { pitch in audio.playTestNote(UInt8(clamping: pitch)) },
                    showLandscape: showLandscape
                )
                PlayableNowView(chordTrack: document.model.chordTrack, key: document.model.key, playheadBeat: playhead)
                Spacer(minLength: 0)
            }
            .padding(18)
        }
        .frame(minWidth: 880, minHeight: 720)
        .onAppear {
            audio.start()
            playback = ChordPlayback(instrument: audio.instrument)
            notePlayback = NotePlayback(instrument: audio.instrument)
            transport.tempo = Tempo(bpm: document.model.tempo)
        }
        .onChange(of: transport.state.positionBeats) { _, beat in
            guard transport.state.isPlaying else { return }
            notePlayback?.update(forBeat: beat, notes: document.model.noteEvents)
            if hearChords { playback?.update(forBeat: beat, in: document.model.chordTrack) }
        }
        .onChange(of: hearChords) { _, on in if !on { playback?.releaseAll() } }
    }

    private func togglePlay() {
        if transport.state.isPlaying {
            transport.stop(); playback?.releaseAll(); notePlayback?.releaseAll()
        } else {
            transport.play()
        }
    }

    private func rewind() {
        transport.rewind(); playback?.releaseAll(); notePlayback?.releaseAll()
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

            ctrlButton(showLandscape ? "◆ map on" : "◆ map") { showLandscape.toggle() }
                .foregroundStyle(showLandscape ? Theme.brand : Theme.faded)
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
