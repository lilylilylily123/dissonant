import Foundation
import AudioKit
import SoundpipeAudioKit

/// A selectable voice — now defined by its *waveform* (the thing that actually changes the
/// character) plus an envelope. Saw rips, square is hollow, triangle/sine are soft.
enum VoiceKind: String, CaseIterable, Identifiable, Sendable {
    case saw, square, triangle, sine, pad, pluck

    var id: String { rawValue }
    var label: String { rawValue }

    var table: Table {
        switch self {
        case .saw, .pad:        return Table(.sawtooth)
        case .square:           return Table(.square)
        case .triangle, .pluck: return Table(.triangle)
        case .sine:             return Table(.sine)
        }
    }

    var preset: SynthPreset {
        switch self {
        case .saw:      return SynthPreset(attack: 0.01,  decay: 0.10, sustain: 0.70, release: 0.30)
        case .square:   return SynthPreset(attack: 0.005, decay: 0.15, sustain: 0.50, release: 0.20)
        case .triangle: return SynthPreset(attack: 0.01,  decay: 0.20, sustain: 0.60, release: 0.40)
        case .sine:     return SynthPreset(attack: 0.02,  decay: 0.20, sustain: 0.70, release: 0.50)
        case .pad:      return SynthPreset(attack: 0.50,  decay: 0.40, sustain: 0.85, release: 1.40)
        case .pluck:    return SynthPreset(attack: 0.001, decay: 0.18, sustain: 0.00, release: 0.22)
        }
    }
}

/// ADSR envelope shape.
struct SynthPreset {
    var attack: Double
    var decay: Double
    var sustain: Double
    var release: Double
}

/// Polyphonic synth: a pool of oscillators, each gated by its own amplitude envelope. The
/// waveform table sets the timbre; voices are allocated round-robin with oldest-note stealing.
@MainActor
final class WaveformSynthInstrument: Instrument {
    private let mixer = Mixer()
    var node: Node { mixer }
    private var voices: [Voice] = []
    private var counter = 0

    @MainActor
    private final class Voice {
        let osc: Oscillator
        let env: AmplitudeEnvelope
        var pitch: UInt8?
        var startedAt = 0

        init(table: Table, preset: SynthPreset) {
            osc = Oscillator(waveform: table)
            env = AmplitudeEnvelope(
                osc,
                attackDuration: AUValue(preset.attack),
                decayDuration: AUValue(preset.decay),
                sustainLevel: AUValue(preset.sustain),
                releaseDuration: AUValue(preset.release)
            )
            osc.amplitude = 0
            osc.start()
        }

        func on(pitch: UInt8, frequency: AUValue, amplitude: AUValue) {
            osc.frequency = frequency
            osc.amplitude = amplitude
            env.openGate()
            self.pitch = pitch
        }

        func off() {
            env.closeGate()
            pitch = nil
        }
    }

    init(table: Table, preset: SynthPreset, polyphony: Int = 8) {
        voices = (0..<polyphony).map { _ in Voice(table: table, preset: preset) }
        for voice in voices { mixer.addInput(voice.env) }
    }

    func noteOn(_ pitch: UInt8, velocity: UInt8) {
        let frequency = AUValue(440.0 * pow(2.0, (Double(pitch) - 69.0) / 12.0))
        let amplitude = AUValue(Double(velocity) / 127.0 * 0.3)
        let voice = voices.first { $0.pitch == nil } ?? voices.min { $0.startedAt < $1.startedAt }!
        counter += 1
        voice.startedAt = counter
        voice.on(pitch: pitch, frequency: frequency, amplitude: amplitude)
    }

    func noteOff(_ pitch: UInt8) {
        voices.first { $0.pitch == pitch }?.off()
    }
}
