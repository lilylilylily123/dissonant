import Foundation
import AVFoundation
import AudioKit
import AudioKitEX

/// Owns the AudioKit engine and the master FX chain. Each track is wired through its own FX
/// bus (built in `TrackVoices`) into `masterMixer`; the chord bed goes straight to master.
@MainActor
final class AudioEngineController {
    let engine = AudioEngine()
    let masterMixer = Mixer()

    // Master FX chain: masterMixer → low-cut → high-cut (tone) → reverb → gain → output.
    private let lowCut: HighPassFilter
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
        highCut = LowPassFilter(lowCut, cutoffFrequency: 18_000)
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

    // MARK: - Voice factories (not connected — TrackVoices wires them into a per-track bus)

    func makeSynthVoice(_ kind: VoiceKind) -> Instrument {
        WaveformSynthInstrument(table: kind.table, preset: kind.preset)
    }

    func makeDrumVoice() -> DrumInstrument {
        DrumInstrument()
    }

    /// Instantiate an installed AU instrument (not connected) on the main thread.
    func instantiateAU(_ info: AUInstrumentInfo, completion: @escaping (AVAudioUnit?) -> Void) {
        AVAudioUnit.instantiate(with: info.componentDescription, options: []) { unit, _ in
            DispatchQueue.main.async { completion(unit) }
        }
    }

    func start() {
        do { try engine.start() }
        catch { print("dissonant: audio engine failed to start — \(error)") }
    }

    func stop() { engine.stop() }

    /// Play a note briefly through any voice — used for placement audition and the test tone.
    func audition(_ pitch: UInt8, on playable: MidiPlayable) {
        playable.noteOn(pitch, velocity: 100)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) {
            playable.noteOff(pitch)
        }
    }
}
