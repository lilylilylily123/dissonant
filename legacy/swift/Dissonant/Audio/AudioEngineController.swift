import Foundation
import AVFoundation
import AudioKit
import AudioKitEX
import SoundpipeAudioKit

/// Owns the AudioKit engine and the master FX chain. Each track is wired through its own FX
/// bus (built in `TrackVoices`) into `masterMixer`; the chord bed goes straight to master.
@MainActor
final class AudioEngineController {
    let engine = AudioEngine()
    let masterMixer = Mixer()

    // Master FX chain: masterMixer → low-cut → 3-band EQ → high-cut (tone) → reverb → gain → out.
    private let lowCut: HighPassFilter
    private let eqLow: LowShelfParametricEqualizerFilter
    private let eqMid: PeakingParametricEqualizerFilter
    private let eqHigh: HighShelfParametricEqualizerFilter
    private let highCut: LowPassFilter
    private let reverb: Reverb
    private let masterFader: Fader

    /// Dedicated voice for the (optional) chord bed.
    let chordInstrument: Instrument

    var avEngine: AVAudioEngine { engine.avEngine }

    init() {
        chordInstrument = WaveformSynthInstrument(table: VoiceKind.pad.table, preset: VoiceKind.pad.preset)
        masterMixer.addInput(chordInstrument.node)

        lowCut = HighPassFilter(masterMixer, cutoffFrequency: 20)
        eqLow = LowShelfParametricEqualizerFilter(lowCut, cornerFrequency: 120, gain: 1.0, q: 0.7)
        eqMid = PeakingParametricEqualizerFilter(eqLow, centerFrequency: 1_000, gain: 1.0, q: 0.7)
        eqHigh = HighShelfParametricEqualizerFilter(eqMid, centerFrequency: 6_000, gain: 1.0, q: 0.7)
        highCut = LowPassFilter(eqHigh, cutoffFrequency: 18_000)
        reverb = Reverb(highCut)
        reverb.dryWetMix = 0          // fully dry by default
        masterFader = Fader(reverb, gain: 1)
        engine.output = masterFader
    }

    // MARK: - Master FX

    func setGain(_ gain: Float) { masterFader.gain = AUValue(gain) }
    func setReverb(_ wet: Float) { reverb.dryWetMix = AUValue(min(max(wet, 0), 1)) }
    func setLowCut(_ hz: Float) { lowCut.cutoffFrequency = AUValue(hz) }
    func setHighCut(_ hz: Float) { highCut.cutoffFrequency = AUValue(hz) }
    /// 3-band EQ gains (linear, 1.0 = flat).
    func setLowEQ(_ gain: Float) { eqLow.gain = AUValue(gain) }
    func setMidEQ(_ gain: Float) { eqMid.gain = AUValue(gain) }
    func setHighEQ(_ gain: Float) { eqHigh.gain = AUValue(gain) }

    // MARK: - Voice factories (not connected — TrackVoices wires them into a per-track bus)

    func makeSynthVoice(_ kind: VoiceKind) -> Instrument {
        WaveformSynthInstrument(table: kind.table, preset: kind.preset)
    }

    func makeDrumVoice() -> DrumInstrument {
        DrumInstrument()
    }

    /// Instantiate an installed AU instrument (not connected) on the main thread. Loaded
    /// out-of-process: the plugin runs in a separate system process, so it can't be loaded
    /// into (or crash) this app, and AUv3 app-extension units — which can't load in-process
    /// at all — work too.
    func instantiateAU(_ info: AUInstrumentInfo, completion: @escaping (AVAudioUnit?) -> Void) {
        AVAudioUnit.instantiate(with: info.componentDescription, options: [.loadOutOfProcess]) { unit, _ in
            DispatchQueue.main.async { completion(unit) }
        }
    }

    func start() {
        do { try engine.start() }
        catch { print("dissonant: audio engine failed to start — \(error)") }
    }

    func stop() { engine.stop() }

    // MARK: - Recording (real-time bounce to a file)

    private var recordFile: AVAudioFile?

    func startRecording(to url: URL) throws {
        let node = masterFader.avAudioNode
        let format = node.outputFormat(forBus: 0)
        recordFile = try AVAudioFile(forWriting: url, settings: format.settings)
        node.installTap(onBus: 0, bufferSize: 4096, format: format) { [weak self] buffer, _ in
            try? self?.recordFile?.write(from: buffer)
        }
    }

    func stopRecording() {
        masterFader.avAudioNode.removeTap(onBus: 0)
        recordFile = nil
    }

    /// Play a note briefly through any voice — used for placement audition and the test tone.
    func audition(_ pitch: UInt8, on playable: MidiPlayable) {
        playable.noteOn(pitch, velocity: 100)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) {
            playable.noteOff(pitch)
        }
    }
}
