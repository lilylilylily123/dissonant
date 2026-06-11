import Foundation
import AVFoundation
import AudioKit
import AudioKitEX

/// Owns the AudioKit engine and a bank of voices, all wired into one mixer. The melody is
/// routed through whichever voice is selected; the chord bed has its own (pad) voice.
/// Sampled (SF2) voices join the bank in a later step.
@MainActor
final class AudioEngineController {
    let engine = AudioEngine()
    private let mixer = Mixer()

    // Master FX chain: mixer → low-cut → high-cut (tone) → reverb → gain → output.
    private let lowCut: HighPassFilter
    private let highCut: LowPassFilter
    private let reverb: Reverb
    private let masterFader: Fader

    /// Dedicated voice for the (optional) chord bed.
    let chordInstrument: Instrument

    init() {
        chordInstrument = WaveformSynthInstrument(table: VoiceKind.pad.table, preset: VoiceKind.pad.preset)
        mixer.addInput(chordInstrument.node)

        lowCut = HighPassFilter(mixer, cutoffFrequency: 20)
        highCut = LowPassFilter(lowCut, cutoffFrequency: 18_000)
        reverb = Reverb(highCut)
        reverb.dryWetMix = 0          // fully dry by default
        masterFader = Fader(reverb, gain: 1)
        engine.output = masterFader
    }

    /// Master output gain (0…~1.5).
    func setGain(_ gain: Float) { masterFader.gain = AUValue(gain) }
    /// Reverb wet amount (0 = dry … 1 = wet).
    func setReverb(_ wet: Float) { reverb.dryWetMix = AUValue(min(max(wet, 0), 1)) }
    /// Low-cut (high-pass) cutoff in Hz — rolls off rumble.
    func setLowCut(_ hz: Float) { lowCut.cutoffFrequency = AUValue(hz) }
    /// High-cut (low-pass) cutoff in Hz — tone/brightness.
    func setHighCut(_ hz: Float) { highCut.cutoffFrequency = AUValue(hz) }

    /// Create a fresh synth voice instance and add it to the mixer. Each track needs its own
    /// instance so simultaneous notes on different tracks don't collide.
    func makeSynthVoice(_ kind: VoiceKind) -> Instrument {
        let inst = WaveformSynthInstrument(table: kind.table, preset: kind.preset)
        mixer.addInput(inst.node)
        return inst
    }

    /// Create a procedural drum kit voice and wire its player nodes into the mixer.
    func makeDrumVoice() -> DrumInstrument {
        let drum = DrumInstrument()
        drum.attach(to: engine.avEngine, mixer: mixer.avAudioNode)
        drum.start()
        return drum
    }

    func start() {
        do {
            try engine.start()
        } catch {
            print("dissonant: audio engine failed to start — \(error)")
        }
    }

    func stop() {
        engine.stop()
    }

    /// Play a note briefly through any voice — used for placement audition and the test tone.
    func audition(_ pitch: UInt8, on playable: MidiPlayable) {
        playable.noteOn(pitch, velocity: 100)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) {
            playable.noteOff(pitch)
        }
    }

    /// Instantiate an installed AU instrument and wire it into the mixer. The completion
    /// returns a playable handle (or nil on failure) on the main thread.
    func loadAudioUnit(_ info: AUInstrumentInfo, completion: @escaping (AUHostInstrument?) -> Void) {
        AVAudioUnit.instantiate(with: info.componentDescription, options: []) { [weak self] unit, _ in
            DispatchQueue.main.async {
                guard let self, let unit else { completion(nil); return }
                let av = self.engine.avEngine
                av.attach(unit)
                av.connect(unit, to: self.mixer.avAudioNode, format: nil)
                completion(AUHostInstrument(avAudioUnit: unit))
            }
        }
    }
}
