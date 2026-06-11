import Foundation
import AudioKit

/// Owns the AudioKit engine and the active instrument. Prefers the SF2 sampler when a
/// soundfont is bundled; otherwise falls back to the oscillator synth (no asset needed).
@MainActor
final class AudioEngineController {
    let engine = AudioEngine()
    private let mixer = Mixer()
    let instrument: Instrument

    /// Pass `nil` to force the oscillator. Default looks for a bundled soundfont and
    /// uses it if present.
    init(soundFontResource: String? = "GeneralUserGS") {
        if let name = soundFontResource, let sampler = SamplerInstrument(soundFont: name) {
            instrument = sampler
        } else {
            instrument = OscillatorInstrument()
        }
        mixer.addInput(instrument.node)
        engine.output = mixer
    }

    func start() {
        do {
            try engine.start()
        } catch {
            print("in key: audio engine failed to start — \(error)")
        }
    }

    func stop() {
        engine.stop()
    }

    /// Convenience for verifying the audio path end-to-end (U2 "it makes sound" check).
    func playTestNote(_ pitch: UInt8 = 60) {
        instrument.noteOn(pitch, velocity: 100)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) { [weak self] in
            self?.instrument.noteOff(pitch)
        }
    }
}
