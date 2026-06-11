import SwiftUI
import DissonantCore

/// The main window: the guidance experience. The chord progression drives the live tier
/// highlighting; by default you don't *hear* the chords. The melody plays through a
/// selectable voice — a built-in synth or any installed Audio Unit instrument.
struct ContentView: View {
    @Binding var document: ProjectDocument

    @StateObject private var transport = Transport()
    @State private var audio = AudioEngineController()
    @State private var playback: ChordPlayback?
    @State private var notePlayback: NotePlayback?

    @State private var hearChords = false
    @State private var showLandscape = false

    // Melody voice: a synth preset, or a hosted Audio Unit (overrides the synth when set).
    @State private var melodyVoice: VoiceKind = .keys
    @State private var melodyAU: AUHostInstrument?
    @State private var auName: String?
    @State private var showAUBrowser = false

    private var currentMelody: MidiPlayable { melodyAU ?? audio.instrument(for: melodyVoice) }
    private var playhead: Double { transport.state.positionBeats }
    private var currentChordName: String { document.model.chordTrack.chord(atBeat: playhead)?.name ?? "—" }

    var body: some View {
        ZStack {
            Theme.surface.ignoresSafeArea()
            VStack(alignment: .leading, spacing: 14) {
                header
                ChordLaneView(chordTrack: $document.model.chordTrack, playheadBeat: playhead)
                voicePicker
                PianoRollView(
                    notes: $document.model.noteEvents,
                    chordTrack: document.model.chordTrack,
                    key: document.model.key,
                    playheadBeat: playhead,
                    onAudition: { pitch in audio.audition(UInt8(clamping: pitch), on: currentMelody) },
                    showLandscape: showLandscape
                )
                PlayableNowView(chordTrack: document.model.chordTrack, key: document.model.key, playheadBeat: playhead)
                Spacer(minLength: 0)
            }
            .padding(18)
        }
        .frame(minWidth: 880, minHeight: 740)
        .sheet(isPresented: $showAUBrowser) {
            AUBrowserView(onSelect: { selectAU($0) }, onClose: { showAUBrowser = false })
        }
        .onAppear {
            audio.start()
            playback = ChordPlayback(instrument: audio.chordInstrument)
            notePlayback = NotePlayback(instrument: currentMelody)
            transport.tempo = Tempo(bpm: document.model.tempo)
            if document.model.chordTrack.isEmpty {
                document.model.chordTrack = ProjectModel.starter.chordTrack
            }
        }
        .onChange(of: transport.state.positionBeats) { _, beat in
            guard transport.state.isPlaying else { return }
            notePlayback?.update(forBeat: beat, notes: document.model.noteEvents)
            if hearChords { playback?.update(forBeat: beat, in: document.model.chordTrack) }
        }
        .onChange(of: hearChords) { _, on in if !on { playback?.releaseAll() } }
    }

    // MARK: - Voice selection

    private func selectSynth(_ voice: VoiceKind) {
        melodyVoice = voice
        melodyAU = nil
        auName = nil
        notePlayback?.releaseAll()
        notePlayback?.instrument = audio.instrument(for: voice)
    }

    private func selectAU(_ info: AUInstrumentInfo) {
        showAUBrowser = false
        audio.loadAudioUnit(info) { host in
            guard let host else { return }
            melodyAU = host
            auName = info.name
            notePlayback?.releaseAll()
            notePlayback?.instrument = host
        }
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

    // MARK: - Chrome

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
            ctrlButton("test tone") { audio.audition(60, on: currentMelody) }
        }
    }

    private var voicePicker: some View {
        HStack(spacing: 6) {
            Text("voice")
                .font(.custom(Theme.mono, size: 10)).foregroundStyle(Theme.faded)
            ForEach(VoiceKind.allCases) { voice in
                let selected = melodyAU == nil && voice == melodyVoice
                voiceChip(voice.label, selected: selected) { selectSynth(voice) }
            }
            Divider().frame(height: 16).overlay(Theme.gridLine)
            voiceChip(auName ?? "AU…", selected: melodyAU != nil) { showAUBrowser = true }
        }
    }

    private func voiceChip(_ label: String, selected: Bool, _ action: @escaping () -> Void) -> some View {
        Button(label, action: action)
            .buttonStyle(.plain)
            .font(.custom(Theme.mono, size: 11))
            .foregroundStyle(selected ? Theme.surface : Theme.ink)
            .padding(.horizontal, 9).padding(.vertical, 4)
            .background(selected ? Theme.brand : Theme.panel)
            .clipShape(RoundedRectangle(cornerRadius: 4))
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
