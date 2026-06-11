import SwiftUI
import AppKit
import UniformTypeIdentifiers
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

    @State private var renamingPattern = false
    @State private var patternDraft = ""
    @FocusState private var patternRenameFocused: Bool

    @State private var masterGain: Double = 1.0
    @State private var reverbWet: Double = 0.0
    @State private var lowCutHz: Double = 20
    @State private var highCutHz: Double = 18_000
    @State private var lowEQ: Double = 1.0
    @State private var midEQ: Double = 1.0
    @State private var highEQ: Double = 1.0

    // Cached flattened song (stable note ids) so song-mode playback doesn't recompute per frame.
    @State private var songNotes: [UUID: [NoteEvent]] = [:]
    @State private var songChords = ChordTrackModel()

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

    // Key inference from the melodic notes in the selected pattern (drums excluded).
    private var keyResult: KeyDetectionResult {
        let drumIDs = Set(document.model.tracks.filter { $0.isDrum }.map { $0.id })
        let pcs = selectedPattern.notesByTrack
            .filter { !drumIDs.contains($0.key) }
            .flatMap { $0.value }
            .map { $0.pitch }
        return KeyDetector().detect(pitchClasses: pcs)
    }
    private func keyName(_ root: Int, _ scale: ScaleType) -> String {
        Harmony.noteName(root) + (scale == .major ? " maj" : " min")
    }

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
                        ChordLaneView(chordTrack: chordsBinding, playheadBeat: playhead, beats: Int(selectedPattern.lengthBeats))
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
                                noteLength: noteLength,
                                beats: Int(selectedPattern.lengthBeats)
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
                        .onChange(of: document.model.arrangement) { _, _ in recomputeSong(); updateLength() }
                    }
                    fxBar
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
        .onChange(of: mode) { _, newMode in
            trackVoices?.releaseAll(); playback?.releaseAll()
            if newMode == .song { recomputeSong() }
            updateLength()
            transport.rewind()
        }
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
            trackVoices?.update(forBeat: beat, tracks: tracks) { songNotes[$0] ?? [] }
            if hearChords { playback?.update(forBeat: beat, in: songChords) }
        }
    }

    /// Flatten the arrangement once (stable per-occurrence note ids), cached for song playback.
    private func recomputeSong() {
        var map: [UUID: [NoteEvent]] = [:]
        for track in document.model.tracks {
            map[track.id] = Arrangement.flattenedNotes(trackID: track.id, patterns: document.model.patterns, arrangement: document.model.arrangement)
        }
        songNotes = map
        songChords = Arrangement.flattenedChords(patterns: document.model.patterns, arrangement: document.model.arrangement)
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
        playback = ChordPlayback(instrument: audio.chordInstrument)
        let voices = TrackVoices(audio: audio)
        voices.sync(tracks: document.model.tracks)   // build the per-track FX graph BEFORE start
        trackVoices = voices
        audio.start()
        selectedTrackID = document.model.tracks.first?.id
        selectedPatternID = document.model.patterns.first?.id
        bpm = Int(document.model.tempo)
        bpmText = String(bpm)
        transport.tempo = Tempo(bpm: document.model.tempo)
        audio.setGain(Float(masterGain))
        audio.setReverb(Float(reverbWet))
        audio.setLowCut(Float(lowCutHz))
        audio.setHighCut(Float(highCutHz))
        audio.setLowEQ(Float(lowEQ))
        audio.setMidEQ(Float(midEQ))
        audio.setHighEQ(Float(highEQ))
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
        rebuildVoices()
        selectedTrackID = track.id
    }

    private func addDrumTrack() {
        let track = Track(name: "drums", isDrum: true)
        document.model.tracks.append(track)
        rebuildVoices()
        selectedTrackID = track.id
    }

    /// Adding a track creates new FX nodes; AVAudioEngine wants the engine stopped for that.
    private func rebuildVoices() {
        audio.stop()
        trackVoices?.sync(tracks: document.model.tracks)
        audio.start()
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

    private func duplicatePattern() {
        let src = selectedPattern
        let copy = SongPattern(name: src.name + " copy", lengthBeats: src.lengthBeats, chords: src.chords, notesByTrack: src.notesByTrack)
        document.model.patterns.insert(copy, at: min(patternIndex + 1, document.model.patterns.count))
        selectedPatternID = copy.id
    }

    private func deletePattern() {
        guard document.model.patterns.count > 1 else { return }
        let id = selPatternID
        document.model.patterns.removeAll { $0.id == id }
        document.model.arrangement.removeAll { $0 == id }
        selectedPatternID = document.model.patterns.first?.id
        recomputeSong()
        updateLength()
    }

    private func setPatternLength(_ bars: Int) {
        guard document.model.patterns.indices.contains(patternIndex) else { return }
        document.model.patterns[patternIndex].lengthBeats = Double(bars * 4)
        updateLength()
        recomputeSong()
    }

    private func startRenamePattern() {
        patternDraft = selectedPattern.name
        renamingPattern = true
        patternRenameFocused = true
    }

    private func commitPatternRename() {
        if !patternDraft.isEmpty, let i = document.model.patterns.firstIndex(where: { $0.id == selPatternID }) {
            document.model.patterns[i].name = patternDraft
        }
        renamingPattern = false
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
        auNames[id] = info.name
        trackVoices?.loadAU(trackID: id, info: info)
    }

    private func togglePlay() {
        if transport.state.isPlaying {
            transport.stop(); playback?.releaseAll(); trackVoices?.releaseAll()
        } else { transport.play() }
    }
    private func rewind() { transport.rewind(); playback?.releaseAll(); trackVoices?.releaseAll() }

    /// Real-time bounce of the full song arrangement to a WAV file.
    private func exportSong() {
        let panel = NSSavePanel()
        panel.allowedContentTypes = [.wav]
        panel.nameFieldStringValue = "song.wav"
        guard panel.runModal() == .OK, let url = panel.url else { return }

        mode = .song
        recomputeSong()
        let total = max(Arrangement.totalLength(patterns: document.model.patterns, arrangement: document.model.arrangement), selectedPattern.lengthBeats)
        transport.setLength(total)
        let seconds = transport.tempo.seconds(forBeats: total) + 1.0

        do { try audio.startRecording(to: url) } catch { return }
        transport.rewind()
        if !transport.state.isPlaying { transport.play() }
        DispatchQueue.main.asyncAfter(deadline: .now() + seconds) {
            transport.stop(); playback?.releaseAll(); trackVoices?.releaseAll()
            audio.stopRecording()
        }
    }

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
                if bpmFocused {
                    ctrlButton("✓") { commitBPM(); bpmFocused = false }
                }
            }

            // mode toggle
            HStack(spacing: 0) {
                modeChip("pattern", on: mode == .pattern) { mode = .pattern }
                modeChip("song", on: mode == .song) { mode = .song }
            }
            .clipShape(RoundedRectangle(cornerRadius: 5))

            keyChip

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
            ctrlButton("⤓ export") { exportSong() }
        }
    }

    @ViewBuilder
    private var keyChip: some View {
        if document.model.key.isLocked, let root = document.model.key.rootPitchClass {
            HStack(spacing: 4) {
                Text("key \(keyName(root, document.model.key.scale))")
                    .font(.custom(Theme.mono, size: 12)).foregroundStyle(Theme.brand)
                Button("×") { document.model.key = .none }
                    .buttonStyle(.plain).font(.custom(Theme.mono, size: 12)).foregroundStyle(Theme.faded)
            }
            .padding(.horizontal, 8).padding(.vertical, 4)
            .background(Theme.panel).clipShape(RoundedRectangle(cornerRadius: 4))
        } else if keyResult.isConfident, let top = keyResult.top {
            Button("looks like \(keyName(top.rootPitchClass, top.scale)) — lock?") {
                document.model.key = KeyState(rootPitchClass: top.rootPitchClass, scale: top.scale, isLocked: true)
            }
            .buttonStyle(.plain)
            .font(.custom(Theme.mono, size: 12)).foregroundStyle(Theme.ink)
            .padding(.horizontal, 8).padding(.vertical, 4)
            .background(Theme.panel).clipShape(RoundedRectangle(cornerRadius: 4))
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
            Divider().frame(height: 14).overlay(Theme.gridLine)
            patBtn("+ new") { addPattern() }
            patBtn("⧉ dup") { duplicatePattern() }
            patBtn("✎ rename") { startRenamePattern() }
            if document.model.patterns.count > 1 { patBtn("× del") { deletePattern() } }
            Divider().frame(height: 14).overlay(Theme.gridLine)
            Text("bars").font(.custom(Theme.mono, size: 10)).foregroundStyle(Theme.faded)
            ForEach([2, 4, 8], id: \.self) { b in
                let sel = Int(selectedPattern.lengthBeats) == b * 4
                Button("\(b)") { setPatternLength(b) }
                    .buttonStyle(.plain).font(.custom(Theme.mono, size: 11))
                    .foregroundStyle(sel ? Theme.surface : Theme.ink)
                    .padding(.horizontal, 7).padding(.vertical, 3)
                    .background(sel ? Theme.brand : Theme.panel)
                    .clipShape(RoundedRectangle(cornerRadius: 4))
            }
            if renamingPattern {
                TextField("", text: $patternDraft)
                    .textFieldStyle(.plain).frame(width: 120)
                    .focused($patternRenameFocused)
                    .font(.custom(Theme.mono, size: 12)).foregroundStyle(Theme.ink)
                    .onSubmit { commitPatternRename() }
                patBtn("✓") { commitPatternRename() }
            }
            Spacer()
        }
    }

    private func patBtn(_ label: String, _ action: @escaping () -> Void) -> some View {
        Button(label, action: action)
            .buttonStyle(.plain)
            .font(.custom(Theme.mono, size: 11)).foregroundStyle(Theme.brand)
            .padding(.horizontal, 6).padding(.vertical, 4)
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

    private var fxBar: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 16) {
                Text("track").font(.custom(Theme.mono, size: 10)).foregroundStyle(Theme.brand)
                fxSlider("vol", value: trackFXBinding(\.volume), range: 0...1.5) { trackVoices?.setVolume(trackID: selTrackID, Float($0)) }
                fxSlider("reverb", value: trackFXBinding(\.reverbSend), range: 0...1) { trackVoices?.setReverb(trackID: selTrackID, Float($0)) }
                fxSlider("tone", value: trackFXBinding(\.tone), range: 800...18_000) { trackVoices?.setTone(trackID: selTrackID, Float($0)) }
                Spacer()
            }
            HStack(spacing: 16) {
                Text("master").font(.custom(Theme.mono, size: 10)).foregroundStyle(Theme.faded)
                fxSlider("gain", value: $masterGain, range: 0...1.5) { audio.setGain(Float($0)) }
                fxSlider("reverb", value: $reverbWet, range: 0...1) { audio.setReverb(Float($0)) }
                fxSlider("low cut", value: $lowCutHz, range: 20...1000) { audio.setLowCut(Float($0)) }
                fxSlider("tone", value: $highCutHz, range: 800...18_000) { audio.setHighCut(Float($0)) }
                Spacer()
            }
            HStack(spacing: 16) {
                Text("eq").font(.custom(Theme.mono, size: 10)).foregroundStyle(Theme.faded)
                fxSlider("low", value: $lowEQ, range: 0...2) { audio.setLowEQ(Float($0)) }
                fxSlider("mid", value: $midEQ, range: 0...2) { audio.setMidEQ(Float($0)) }
                fxSlider("high", value: $highEQ, range: 0...2) { audio.setHighEQ(Float($0)) }
                Spacer()
            }
        }
    }

    private func trackFXBinding(_ keyPath: WritableKeyPath<Track, Double>) -> Binding<Double> {
        Binding(
            get: { document.model.tracks[safe: trackIndex]?[keyPath: keyPath] ?? 0 },
            set: { if document.model.tracks.indices.contains(trackIndex) { document.model.tracks[trackIndex][keyPath: keyPath] = $0 } }
        )
    }

    private func fxSlider(_ label: String, value: Binding<Double>, range: ClosedRange<Double>, apply: @escaping (Double) -> Void) -> some View {
        HStack(spacing: 5) {
            Text(label).font(.custom(Theme.mono, size: 10)).foregroundStyle(Theme.faded)
            Slider(value: Binding(get: { value.wrappedValue }, set: { value.wrappedValue = $0; apply($0) }), in: range)
                .controlSize(.small).frame(width: 104).tint(Theme.brand)
        }
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
