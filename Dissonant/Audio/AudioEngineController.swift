import Foundation
import AudioKit

/// Owns the AudioKit engine and a bank of voices, all wired into one mixer. The melody is
/// routed through whichever voice is selected; the chord bed has its own (pad) voice.
/// Sampled (SF2) voices join the bank in a later step.
@MainActor
final class AudioEngineController {
    let engine = AudioEngine()
    private let mixer = Mixer()

    private var voices: [VoiceKind: Instrument] = [:]
    /// Dedicated voice for the (optional) chord bed.
    let chordInstrument: Instrument

    init() {
        for kind in VoiceKind.allCases {
            let inst = SynthInstrument(preset: kind.preset)
            voices[kind] = inst
            mixer.addInput(inst.node)
        }
        let chords = SynthInstrument(preset: .pad)
        chordInstrument = chords
        mixer.addInput(chords.node)
        engine.output = mixer
    }

    func instrument(for kind: VoiceKind) -> Instrument {
        voices[kind] ?? chordInstrument
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

    /// Play a note briefly through a voice — used for placement audition and the test tone.
    func audition(_ pitch: UInt8, voice: VoiceKind) {
        let inst = instrument(for: voice)
        inst.noteOn(pitch, velocity: 100)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) {
            inst.noteOff(pitch)
        }
    }
}

private extension SynthPreset {
    static let pad = VoiceKind.pad.preset
}
