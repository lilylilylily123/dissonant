import SwiftUI
import DissonantCore

/// The main window. Left: track sidebar (voices). Right: a pattern bar + either **pattern
/// mode** (chord lane + piano roll editing the selected pattern's selected track) or **song
/// mode** (arrange patterns into a full track). Chords drive guidance, not sound, by default.
struct ContentView: View {
    @Binding var document: ProjectDocument

    enum Mode { case pattern, song }

    @StateObject private var transport = Transport()
    @State private var audio = AudioEngineController()
    @State private var playback: ChordPlayback?
    @State private var trackVoices: TrackVoices?

    @State private var mode: Mode = .pattern
    @State private var selectedTrackID: UUID?
    @State private var selectedPatternID: UUID?
    @State private var auNames: [UUID: String] = [:]

    @State private var hearChords = false
    @State private var showLandscape = false
    @State private var showAUBrowser = false
    @State private var noteLength: Double = 1
    @State private var bpm: Int = 120
    @State private var bpmText: String = "120"
    @FocusState private var bpmFocused: Bool

    private let lengthOptions: [(String, Double)] = [("1/16", 0.25), ("1/8", 0.5), ("1/4", 1.0), ("1/2", 2.0), ("1", 4.0)]

    // MARK: - Derived

    private var selTrackID: UUID { selectedTrackID ?? document.model.tracks.first?.id ?? UUID() }
    private var trackIndex: Int { document.model.tracks.firstIndex { $0.id == selTrackID } ?? 0 }
    private var selectedVoice: VoiceKind { VoiceKind(rawValue: document.model.tracks[safe: trackIndex]?.voice ?? "saw") ?? .saw }

    private var selPatternID: UUID { selectedPatternID ?? document.model.patterns.first?.id ?? UUID() }
    private var patternIndex: Int { document.model.patterns.firstIndex { $0.id == selPatternID } ?? 0 }
    private var selectedPattern: SongPattern { document.model.patterns[safe: patternIndex] ?? SongPattern(name: "—") }
    private var isDrumSelected: Bool { document.model.tracks[safe: trackIndex]?.isDrum == true }

    private var notesBinding: Binding<[NoteEvent]> {
        Binding(
            get: { document.model.patterns[safe: patternIndex]?.notesByTrack[selTrackID] ?? [] },
            set: {
                guard document.model.patterns.indices.contains(patternIndex) else { return }
                document.model.patterns[patternIndex].notesByTrack[selTrackID] = $0
            }
        )
    }

    private var chordsBinding: Binding<ChordTrackModel> {
        Binding(
            get: { document.model.patterns[safe: patternIndex]?.chords ?? ChordTrackModel() },
            set: {
                guard document.model.patterns.indices.contains(patternIndex) else { return }
                document.model.patterns[patternIndex].chords = $0
            }
        )
    }

    private var playhead: Double { transport.state.positionBeats }
    private var currentChordName: String { selectedPattern.chords.chord(atBeat: playhead)?.name ?? "—" }

    var body: some View {
        ZStack {
            Theme.surface.ignoresSafeArea()
            HStack(spacing: 0) {
                TrackSidebarView(
                    tracks: document.model.tracks,
                    selectedTrackID: selTrackID,
                    onAdd: addTrack,
                    onAddDrum: addDrumTrack,
                    onSelect: { selectedTrackID = $0 },
                    onRename: renameTrack,
                    onDelete: deleteTrack,
                    onToggleMute: toggleMute,
                    onToggleSolo: toggleSolo
                )
                VStack(alignment: .leading, spacing: 14) {
                    header
                    patternBar
                    if mode == .pattern {
                        ChordLaneView(chordTrack: chordsBinding, playheadBeat: playhead)
                        if isDrumSelected {
                            DrumGridView(
                                notes: notesBinding,
                                patternLength: selectedPattern.lengthBeats,
                                playheadBeat: playhead,
                                onHit: auditionNote
                            )
                        } else {
                            voicePicker
                            PianoRollView(
                                notes: notesBinding,
                                chordTrack: selectedPattern.chords,
                                key: document.model.key,
                                playheadBeat: playhead,
                                onAudition: auditionNote,
                                showLandscape: showLandscape,
                                noteLength: noteLength
                            )
                            PlayableNowView(chordTrack: selectedPattern.chords, key: document.model.key, playheadBeat: playhead)
                        }
                    } else {
                        SongView(
                            patterns: document.model.patterns,
                            arrangement: $document.model.arrangement,
                            playheadBeat: playhead,
                            patternLength: selectedPattern.lengthBeats
                        )
                        .onChange(of: document.model.arrangement) { _, _ in updateLength() }
                    }
                    Spacer(minLength: 0)
                }
                .padding(18)
            }
        }
        .frame(minWidth: 1120, minHeight: 920)
        .sheet(isPresented: $showAUBrowser) {
            AUBrowserView(onSelect: { selectAU($0) }, onClose: { showAUBrowser = false })
        }
        .onAppear { setup() }
        .onChange(of: transport.state.positionBeats) { _, beat in tick(beat) }
        .onChange(of: hearChords) { _, on in if !on { playback?.releaseAll() } }
        .onChange(of: mode) { _, _ in updateLength() }
        .onChange(of: selectedPatternID) { _, _ in if mode == .pattern { updateLength() } }
        .onChange(of: bpm) { _, value in
            transport.tempo = Tempo(bpm: Double(value)); document.model.tempo = Double(value)
        }
    }

    // MARK: - Playback

    private func tick(_ beat: Double) {
        guard transport.state.isPlaying else { return }
        let tracks = document.model.tracks
        if mode == .pattern {
            let pattern = selectedPattern
            trackVoices?.update(forBeat: beat, tracks: tracks) { pattern.notes(for: $0) }
            if hearChords { playback?.update(forBeat: beat, in: pattern.chords) }
        } else {
            let patterns = document.model.patterns
            let arrangement = document.model.arrangement
            trackVoices?.update(forBeat: beat, tracks: tracks) {
                Arrangement.flattenedNotes(trackID: $0, patterns: patterns, arrangement: arrangement)
            }
            if hearChords {
                playback?.update(forBeat: beat, in: Arrangement.flattenedChords(patterns: patterns, arrangement: arrangement))
            }
        }
    }

    private func updateLength() {
        let length: Double
        if mode == .pattern {
            length = selectedPattern.lengthBeats
        } else {
            length = max(Arrangement.totalLength(patterns: document.model.patterns, arrangement: document.model.arrangement), selectedPattern.lengthBeats)
        }
        transport.setLength(length)
    }

    // MARK: - Lifecycle

    private func setup() {
        audio.start()
        playback = ChordPlayback(instrument: audio.chordInstrument)
        let voices = TrackVoices(audio: audio)
        voices.sync(tracks: document.model.tracks)
        trackVoices = voices
        selectedTrackID = document.model.tracks.first?.id
        selectedPatternID = document.model.patterns.first?.id
        bpm = Int(document.model.tempo)
        bpmText = String(bpm)
        transport.tempo = Tempo(bpm: document.model.tempo)
        updateLength()
    }

    private func auditionNote(_ pitch: Int) {
        guard let inst = trackVoices?.instrument(trackID: selTrackID) else { return }
        audio.audition(UInt8(clamping: pitch), on: inst)
    }

    // MARK: - Tracks

    private func addTrack() {
        let track = Track(name: "track \(document.model.tracks.count + 1)")
        document.model.tracks.append(track)
        trackVoices?.sync(tracks: document.model.tracks)
        selectedTrackID = track.id
    }

    private func addDrumTrack() {
        let track = Track(name: "drums", isDrum: true)
        document.model.tracks.append(track)
        trackVoices?.sync(tracks: document.model.tracks)
        selectedTrackID = track.id
    }

    private func deleteTrack(_ id: UUID) {
        guard document.model.tracks.count > 1 else { return }
        document.model.tracks.removeAll { $0.id == id }
        for i in document.model.patterns.indices { document.model.patterns[i].notesByTrack[id] = nil }
        if selTrackID == id { selectedTrackID = document.model.tracks.first?.id }
    }

    private func renameTrack(_ id: UUID, _ name: String) {
        if let i = document.model.tracks.firstIndex(where: { $0.id == id }) { document.model.tracks[i].name = name }
    }
    private func toggleMute(_ id: UUID) {
        if let i = document.model.tracks.firstIndex(where: { $0.id == id }) { document.model.tracks[i].muted.toggle() }
    }
    private func toggleSolo(_ id: UUID) {
        if let i = document.model.tracks.firstIndex(where: { $0.id == id }) { document.model.tracks[i].soloed.toggle() }
    }

    // MARK: - Patterns

    private func addPattern() {
        var pattern = SongPattern(name: "pattern \(document.model.patterns.count + 1)")
        pattern.chords = ProjectModel.starter.patterns.first?.chords ?? ChordTrackModel()
        document.model.patterns.append(pattern)
        selectedPatternID = pattern.id
        if mode == .pattern { updateLength() }
    }

    // MARK: - Voice

    private func selectSynth(_ voice: VoiceKind) {
        guard document.model.tracks.indices.contains(trackIndex) else { return }
        document.model.tracks[trackIndex].voice = voice.rawValue
        auNames[selTrackID] = nil
        trackVoices?.setSynth(trackID: selTrackID, voice: voice)
    }

    private func selectAU(_ info: AUInstrumentInfo) {
        showAUBrowser = false
        let id = selTrackID
        audio.loadAudioUnit(info) { host in
            guard let host else { return }
            auNames[id] = info.name
            trackVoices?.setAU(trackID: id, instrument: host)
        }
    }

    private func togglePlay() {
        if transport.state.isPlaying {
            transport.stop(); playback?.releaseAll(); trackVoices?.releaseAll()
        } else { transport.play() }
    }
    private func rewind() { transport.rewind(); playback?.releaseAll(); trackVoices?.releaseAll() }

    private func setBPM(_ value: Int) { bpm = min(240, max(40, value)); bpmText = String(bpm) }
    private func commitBPM() {
        if let v = Int(bpmText.trimmingCharacters(in: .whitespaces)) { setBPM(v) } else { bpmText = String(bpm) }
    }

    // MARK: - Chrome

    private var header: some View {
        HStack(alignment: .firstTextBaseline, spacing: 16) {
            Text("dissonant")
                .font(.custom(Theme.mono, size: 26)).bold().foregroundStyle(Theme.ink)
            HStack(spacing: 5) {
                ctrlButton("−") { setBPM(bpm - 1) }
                TextField("", text: $bpmText)
                    .textFieldStyle(.plain).frame(width: 36).multilineTextAlignment(.center)
                    .focused($bpmFocused)
                    .font(.custom(Theme.mono, size: 14)).foregroundStyle(Theme.ink)
                    .onSubmit { commitBPM() }
                    .onChange(of: bpmFocused) { _, f in if !f { commitBPM() } }
                Text("bpm").font(.custom(Theme.mono, size: 11)).foregroundStyle(Theme.faded)
                ctrlButton("+") { setBPM(bpm + 1) }
            }

            // mode toggle
            HStack(spacing: 0) {
                modeChip("pattern", on: mode == .pattern) { mode = .pattern }
                modeChip("song", on: mode == .song) { mode = .song }
            }
            .clipShape(RoundedRectangle(cornerRadius: 5))

            Spacer()

            ctrlButton("⏮") { rewind() }.keyboardShortcut("r", modifiers: [])
            Button(transport.state.isPlaying ? "⏹ stop" : "▶ play") { togglePlay() }
                .buttonStyle(.plain)
                .font(.custom(Theme.mono, size: 14)).bold().foregroundStyle(Theme.surface)
                .padding(.horizontal, 12).padding(.vertical, 6)
                .background(Theme.brand).clipShape(RoundedRectangle(cornerRadius: 5))
                .keyboardShortcut(.space, modifiers: [])
            ctrlButton(showLandscape ? "◆ map on" : "◆ map") { showLandscape.toggle() }
                .foregroundStyle(showLandscape ? Theme.brand : Theme.faded)
            ctrlButton(hearChords ? "♪ chords on" : "♪ chords off") { hearChords.toggle() }
                .foregroundStyle(hearChords ? Theme.brand : Theme.faded)
            if mode == .pattern {
                ctrlButton("clear") { notesBinding.wrappedValue.removeAll() }
            }
        }
    }

    private func modeChip(_ label: String, on: Bool, _ action: @escaping () -> Void) -> some View {
        Button(label, action: action)
            .buttonStyle(.plain)
            .font(.custom(Theme.mono, size: 12)).bold()
            .foregroundStyle(on ? Theme.surface : Theme.faded)
            .padding(.horizontal, 12).padding(.vertical, 6)
            .background(on ? Theme.brand : Theme.panel)
    }

    private var patternBar: some View {
        HStack(spacing: 6) {
            Text("pattern")
                .font(.custom(Theme.mono, size: 10)).foregroundStyle(Theme.faded)
            ForEach(document.model.patterns) { pattern in
                let selected = pattern.id == selPatternID
                Button(pattern.name) { selectedPatternID = pattern.id }
                    .buttonStyle(.plain)
                    .font(.custom(Theme.mono, size: 12))
                    .foregroundStyle(selected ? Theme.surface : Theme.ink)
                    .padding(.horizontal, 10).padding(.vertical, 5)
                    .background(selected ? Theme.brand : Theme.panel)
                    .clipShape(RoundedRectangle(cornerRadius: 4))
            }
            Button("+ pattern") { addPattern() }
                .buttonStyle(.plain)
                .font(.custom(Theme.mono, size: 12)).foregroundStyle(Theme.brand)
                .padding(.horizontal, 8).padding(.vertical, 5)
        }
    }

    private var voicePicker: some View {
        HStack(spacing: 6) {
            Text("voice").font(.custom(Theme.mono, size: 10)).foregroundStyle(Theme.faded)
            ForEach(VoiceKind.allCases) { voice in
                let selected = auNames[selTrackID] == nil && voice == selectedVoice
                voiceChip(voice.label, selected: selected) { selectSynth(voice) }
            }
            Divider().frame(height: 16).overlay(Theme.gridLine)
            voiceChip(auNames[selTrackID] ?? "AU…", selected: auNames[selTrackID] != nil) { showAUBrowser = true }
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
            .font(.custom(Theme.mono, size: 13))
            .foregroundStyle(selected ? Theme.surface : Theme.ink)
            .padding(.horizontal, 11).padding(.vertical, 6)
            .background(selected ? Theme.brand : Theme.panel)
            .clipShape(RoundedRectangle(cornerRadius: 5))
    }

    private func ctrlButton(_ label: String, _ action: @escaping () -> Void) -> some View {
        Button(label, action: action)
            .buttonStyle(.plain)
            .font(.custom(Theme.mono, size: 13))
            .foregroundStyle(Theme.faded)
            .padding(.horizontal, 10).padding(.vertical, 7)
            .background(Theme.panel)
            .clipShape(RoundedRectangle(cornerRadius: 5))
    }
}

private extension Array {
    subscript(safe index: Int) -> Element? {
        indices.contains(index) ? self[index] : nil
    }
}
