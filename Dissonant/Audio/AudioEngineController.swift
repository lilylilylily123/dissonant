import Foundation
import AVFoundation
import AudioKit

/// Owns the AudioKit engine and a bank of voices, all wired into one mixer. The melody is
/// routed through whichever voice is selected; the chord bed has its own (pad) voice.
/// Sampled (SF2) voices join the bank in a later step.
@MainActor
final class AudioEngineController {
    let engine = AudioEngine()
    private let mixer = Mixer()

    /// Dedicated voice for the (optional) chord bed.
    let chordInstrument: Instrument

    init() {
        chordInstrument = WaveformSynthInstrument(table: VoiceKind.pad.table, preset: VoiceKind.pad.preset)
        mixer.addInput(chordInstrument.node)
        engine.output = mixer
    }

    /// Create a fresh synth voice instance and add it to the mixer. Each track needs its own
    /// instance so simultaneous notes on different tracks don't collide.
    func makeSynthVoice(_ kind: VoiceKind) -> Instrument {
        let inst = WaveformSynthInstrument(table: kind.table, preset: kind.preset)
        mixer.addInput(inst.node)
        return inst
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
