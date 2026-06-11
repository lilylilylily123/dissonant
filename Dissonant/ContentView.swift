import SwiftUI
import DissonantCore

/// The main window. A track sidebar on the left; the chord lane, voice/length controls,
/// piano roll, and playable-now readout on the right. The roll edits the *selected* track;
/// playback layers every track through its own voice. Chords drive guidance, not sound.
struct ContentView: View {
    @Binding var document: ProjectDocument

    @StateObject private var transport = Transport()
    @State private var audio = AudioEngineController()
    @State private var playback: ChordPlayback?
    @State private var trackVoices: TrackVoices?

    @State private var selectedTrackID: UUID?
    @State private var auNames: [UUID: String] = [:]   // runtime AU name per track (not persisted)

    @State private var hearChords = false
    @State private var showLandscape = false
    @State private var showAUBrowser = false
    @State private var noteLength: Double = 1
    @State private var bpm: Int = 120

    private let lengthOptions: [(String, Double)] = [("1/16", 0.25), ("1/8", 0.5), ("1/4", 1.0), ("1/2", 2.0), ("1", 4.0)]

    // MARK: - Derived

    private var selID: UUID { selectedTrackID ?? document.model.tracks.first?.id ?? UUID() }
    private var selectedIndex: Int { document.model.tracks.firstIndex { $0.id == selID } ?? 0 }
    private var selectedVoice: VoiceKind { VoiceKind(rawValue: document.model.tracks[safe: selectedIndex]?.voice ?? "keys") ?? .keys }

    private var notesBinding: Binding<[NoteEvent]> {
        Binding(
            get: { document.model.tracks[safe: selectedIndex]?.noteEvents ?? [] },
            set: { if document.model.tracks.indices.contains(selectedIndex) { document.model.tracks[selectedIndex].noteEvents = $0 } }
        )
    }

    private var playhead: Double { transport.state.positionBeats }
    private var currentChordName: String { document.model.chordTrack.chord(atBeat: playhead)?.name ?? "—" }

    var body: some View {
        ZStack {
            Theme.surface.ignoresSafeArea()
            HStack(spacing: 0) {
                TrackSidebarView(
                    tracks: $document.model.tracks,
                    selectedTrackID: selID,
                    onAdd: addTrack,
                    onSelect: { selectedTrackID = $0 },
                    onDelete: deleteTrack
                )
                VStack(alignment: .leading, spacing: 14) {
                    header
                    ChordLaneView(chordTrack: $document.model.chordTrack, playheadBeat: playhead)
                    voicePicker
                    PianoRollView(
                        notes: notesBinding,
                        chordTrack: document.model.chordTrack,
                        key: document.model.key,
                        playheadBeat: playhead,
                        onAudition: auditionNote,
                        showLandscape: showLandscape,
                        noteLength: noteLength
                    )
                    PlayableNowView(chordTrack: document.model.chordTrack, key: document.model.key, playheadBeat: playhead)
                    Spacer(minLength: 0)
                }
                .padding(18)
            }
        }
        .frame(minWidth: 1080, minHeight: 900)
        .sheet(isPresented: $showAUBrowser) {
            AUBrowserView(onSelect: { selectAU($0) }, onClose: { showAUBrowser = false })
        }
        .onAppear { setup() }
        .onChange(of: transport.state.positionBeats) { _, beat in
            guard transport.state.isPlaying else { return }
            trackVoices?.update(forBeat: beat, tracks: document.model.tracks)
            if hearChords { playback?.update(forBeat: beat, in: document.model.chordTrack) }
        }
        .onChange(of: hearChords) { _, on in if !on { playback?.releaseAll() } }
        .onChange(of: bpm) { _, value in
            transport.tempo = Tempo(bpm: Double(value))
            document.model.tempo = Double(value)
        }
    }

    // MARK: - Lifecycle

    private func setup() {
        audio.start()
        playback = ChordPlayback(instrument: audio.chordInstrument)
        let voices = TrackVoices(audio: audio)
        if document.model.chordTrack.isEmpty {
            document.model.chordTrack = ProjectModel.starter.chordTrack
        }
        voices.sync(tracks: document.model.tracks)
        trackVoices = voices
        selectedTrackID = document.model.tracks.first?.id
        bpm = Int(document.model.tempo)
        transport.tempo = Tempo(bpm: document.model.tempo)
    }

    private func auditionNote(_ pitch: Int) {
        guard let inst = trackVoices?.instrument(trackID: selID) else { return }
        audio.audition(UInt8(clamping: pitch), on: inst)
    }

    // MARK: - Tracks

    private func addTrack() {
        let n = document.model.tracks.count + 1
        let track = Track(name: "track \(n)")
        document.model.tracks.append(track)
        trackVoices?.sync(tracks: document.model.tracks)
        selectedTrackID = track.id
    }

    private func deleteTrack(_ id: UUID) {
        guard document.model.tracks.count > 1 else { return }
        document.model.tracks.removeAll { $0.id == id }
        if selID == id { selectedTrackID = document.model.tracks.first?.id }
    }

    // MARK: - Voice

    private func selectSynth(_ voice: VoiceKind) {
        guard document.model.tracks.indices.contains(selectedIndex) else { return }
        document.model.tracks[selectedIndex].voice = voice.rawValue
        auNames[selID] = nil
        trackVoices?.setSynth(trackID: selID, voice: voice)
    }

    private func selectAU(_ info: AUInstrumentInfo) {
        showAUBrowser = false
        let id = selID
        audio.loadAudioUnit(info) { host in
            guard let host else { return }
            auNames[id] = info.name
            trackVoices?.setAU(trackID: id, instrument: host)
        }
    }

    private func togglePlay() {
        if transport.state.isPlaying {
            transport.stop(); playback?.releaseAll(); trackVoices?.releaseAll()
        } else {
            transport.play()
        }
    }

    private func rewind() {
        transport.rewind(); playback?.releaseAll(); trackVoices?.releaseAll()
    }

    // MARK: - Chrome

    private var header: some View {
        HStack(alignment: .firstTextBaseline, spacing: 16) {
            Text("dissonant")
                .font(.custom(Theme.mono, size: 26)).bold()
                .foregroundStyle(Theme.ink)
            HStack(spacing: 4) {
                ctrlButton("−") { bpm = max(40, bpm - 1) }
                TextField("", value: $bpm, format: .number)
                    .textFieldStyle(.plain).frame(width: 30)
                    .multilineTextAlignment(.center)
                    .font(.custom(Theme.mono, size: 12)).foregroundStyle(Theme.ink)
                Text("bpm").font(.custom(Theme.mono, size: 10)).foregroundStyle(Theme.faded)
                ctrlButton("+") { bpm = min(240, bpm + 1) }
            }
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
            ctrlButton("clear") { notesBinding.wrappedValue.removeAll() }
        }
    }

    private var voicePicker: some View {
        HStack(spacing: 6) {
            Text("voice")
                .font(.custom(Theme.mono, size: 10)).foregroundStyle(Theme.faded)
            ForEach(VoiceKind.allCases) { voice in
                let selected = auNames[selID] == nil && voice == selectedVoice
                voiceChip(voice.label, selected: selected) { selectSynth(voice) }
            }
            Divider().frame(height: 16).overlay(Theme.gridLine)
            voiceChip(auNames[selID] ?? "AU…", selected: auNames[selID] != nil) { showAUBrowser = true }

            Spacer()

            Text("len").font(.custom(Theme.mono, size: 10)).foregroundStyle(Theme.faded)
            ForEach(lengthOptions, id: \.0) { option in
                voiceChip(option.0, selected: noteLength == option.1) { noteLength = option.1 }
            }
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

private extension Array {
    subscript(safe index: Int) -> Element? {
        indices.contains(index) ? self[index] : nil
    }
}
