import AudioKit
import DunneAudioKit

/// A selectable melody/bass timbre. v1 voices are pure-synth presets (no asset); sampled
/// (SF2) voices join this list in a later step.
enum VoiceKind: String, CaseIterable, Identifiable, Sendable {
    case pluck, keys, pad, bass, lead

    var id: String { rawValue }
    var label: String { rawValue }

    var preset: SynthPreset {
        switch self {
        case .pluck: return SynthPreset(attack: 0.001, decay: 0.18, sustain: 0.0,  release: 0.22)
        case .keys:  return SynthPreset(attack: 0.002, decay: 0.30, sustain: 0.45, release: 0.45)
        case .pad:   return SynthPreset(attack: 0.5,   decay: 0.40, sustain: 0.85, release: 1.40)
        case .bass:  return SynthPreset(attack: 0.004, decay: 0.20, sustain: 0.70, release: 0.18)
        case .lead:  return SynthPreset(attack: 0.01,  decay: 0.10, sustain: 0.75, release: 0.35)
        }
    }
}

/// ADSR envelope shape — the cheap, reliable axis that makes voices feel distinct (a pluck
/// stabs and dies, a pad swells and sustains) without risking a mis-set filter that silences
/// the synth.
struct SynthPreset {
    var attack: Double
    var decay: Double
    var sustain: Double
    var release: Double
}

/// A polyphonic synth voice configured from a preset.
@MainActor
final class SynthInstrument: Instrument {
    private let synth = Synth()
    var node: Node { synth }

    init(preset: SynthPreset) {
        synth.attackDuration = AUValue(preset.attack)
        synth.decayDuration = AUValue(preset.decay)
        synth.sustainLevel = AUValue(preset.sustain)
        synth.releaseDuration = AUValue(preset.release)
    }

    func noteOn(_ pitch: UInt8, velocity: UInt8) {
        synth.play(noteNumber: MIDINoteNumber(pitch), velocity: MIDIVelocity(velocity))
    }

    func noteOff(_ pitch: UInt8) {
        synth.stop(noteNumber: MIDINoteNumber(pitch))
    }
}
