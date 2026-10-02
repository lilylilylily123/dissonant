import AudioKit

/// Anything the sequencer can play MIDI notes into — synth voices, samplers, or hosted
/// Audio Units. Decoupled from the audio graph so a hosted AU (not an AudioKit `Node`) can
/// still be a melody voice.
@MainActor
protocol MidiPlayable: AnyObject {
    func noteOn(_ pitch: UInt8, velocity: UInt8)
    func noteOff(_ pitch: UInt8)
}

/// A playable voice that lives in the AudioKit mixer graph (exposes a `Node`).
@MainActor
protocol Instrument: MidiPlayable {
    var node: Node { get }
}

/// SF2 sampler voice. `init?` fails when the named soundfont isn't bundled, letting the
/// caller fall back to the oscillator. Drop a `<name>.sf2` into the app's resources to enable.
@MainActor
final class SamplerInstrument: Instrument {
    private let sampler = AppleSampler()
    var node: Node { sampler }

    init?(soundFont name: String) {
        do {
            try sampler.loadSoundFont(name, preset: 0, bank: 0)
        } catch {
            return nil
        }
    }

    func noteOn(_ pitch: UInt8, velocity: UInt8) {
        sampler.play(noteNumber: MIDINoteNumber(pitch), velocity: MIDIVelocity(velocity), channel: 0)
    }

    func noteOff(_ pitch: UInt8) {
        sampler.stop(noteNumber: MIDINoteNumber(pitch), channel: 0)
    }
}
