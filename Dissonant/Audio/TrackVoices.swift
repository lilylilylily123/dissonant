import Foundation
import AVFoundation
import AudioKit
import AudioKitEX
import DissonantCore

/// Manages each track's instrument and its own FX bus (tone → reverb → gain → master), so
/// tracks layer with independent volume/reverb/tone. Tracks are matched by id.
@MainActor
final class TrackVoices {
    private let audio: AudioEngineController
    private var entries: [UUID: Entry] = [:]

    @MainActor
    private final class Entry {
        let bus: Mixer
        let tone: LowPassFilter
        let reverb: Reverb
        let fader: Fader
        var instrument: MidiPlayable
        let playback: NotePlayback
        init(bus: Mixer, tone: LowPassFilter, reverb: Reverb, fader: Fader, instrument: MidiPlayable) {
            self.bus = bus
            self.tone = tone
            self.reverb = reverb
            self.fader = fader
            self.instrument = instrument
            self.playback = NotePlayback(instrument: instrument)
        }
    }

    init(audio: AudioEngineController) {
        self.audio = audio
    }

    /// Ensure every track has a voice + FX bus.
    func sync(tracks: [Track]) {
        for track in tracks where entries[track.id] == nil {
            entries[track.id] = makeEntry(for: track)
        }
    }

    private func makeEntry(for track: Track) -> Entry {
        // Build the FX chain into the master first, so the bus is in the engine graph before
        // we connect the instrument into it.
        let bus = Mixer()
        let tone = LowPassFilter(bus, cutoffFrequency: AUValue(track.tone))
        let reverb = Reverb(tone)
        reverb.dryWetMix = AUValue(min(max(track.reverbSend, 0), 1))
        let fader = Fader(reverb, gain: AUValue(track.volume))
        audio.masterMixer.addInput(fader)

        let instrument: MidiPlayable
        if track.isDrum {
            let drum = audio.makeDrumVoice()
            drum.attach(to: audio.avEngine, mixer: bus.avAudioNode)
            drum.start()
            instrument = drum
        } else {
            let synth = audio.makeSynthVoice(VoiceKind(rawValue: track.voice) ?? .saw)
            bus.addInput(synth.node)
            instrument = synth
        }
        return Entry(bus: bus, tone: tone, reverb: reverb, fader: fader, instrument: instrument)
    }

    func setSynth(trackID: UUID, voice: VoiceKind) {
        guard let entry = entries[trackID] else { return }
        entry.playback.releaseAll()
        let synth = audio.makeSynthVoice(voice)
        entry.bus.addInput(synth.node)
        entry.instrument = synth
        entry.playback.instrument = synth
    }

    func loadAU(trackID: UUID, info: AUInstrumentInfo) {
        guard let entry = entries[trackID] else { return }
        audio.instantiateAU(info) { [weak self] unit in
            guard self != nil, let unit else { return }
            self?.audio.avEngine.attach(unit)
            self?.audio.avEngine.connect(unit, to: entry.bus.avAudioNode, format: nil)
            let host = AUHostInstrument(avAudioUnit: unit)
            entry.playback.releaseAll()
            entry.instrument = host
            entry.playback.instrument = host
        }
    }

    // MARK: - Per-track FX

    func setVolume(trackID: UUID, _ gain: Float) { entries[trackID]?.fader.gain = AUValue(gain) }
    func setReverb(trackID: UUID, _ wet: Float) { entries[trackID]?.reverb.dryWetMix = AUValue(min(max(wet, 0), 1)) }
    func setTone(trackID: UUID, _ hz: Float) { entries[trackID]?.tone.cutoffFrequency = AUValue(hz) }

    // MARK: - Playback

    func update(forBeat beat: Double, tracks: [Track], notesForTrack: (UUID) -> [NoteEvent]) {
        let anySolo = tracks.contains { $0.soloed }
        for track in tracks {
            let audible = anySolo ? track.soloed : !track.muted
            if audible {
                entries[track.id]?.playback.update(forBeat: beat, notes: notesForTrack(track.id))
            } else {
                entries[track.id]?.playback.releaseAll()
            }
        }
    }

    func releaseAll() {
        for entry in entries.values { entry.playback.releaseAll() }
    }

    func instrument(trackID: UUID) -> MidiPlayable? {
        entries[trackID]?.instrument
    }
}
