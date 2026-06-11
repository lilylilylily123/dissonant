import AudioKit
import DunneAudioKit

/// A playable instrument voice. Two backends: a pure oscillator synth (no asset)
/// and an SF2 sampler (when a soundfont is bundled). Both are AudioKit `Node`s so
/// they drop into the engine's mixer interchangeably.
@MainActor
protocol Instrument: AnyObject {
    var node: Node { get }
    func noteOn(_ pitch: UInt8, velocity: UInt8)
    func noteOff(_ pitch: UInt8)
}

/// Pure-synthesis voice — the default. No bundled asset, suits the lo-fi/DIY brand.
@MainActor
final class OscillatorInstrument: Instrument {
    private let synth = Synth()
    var node: Node { synth }

    func noteOn(_ pitch: UInt8, velocity: UInt8) {
        synth.play(noteNumber: MIDINoteNumber(pitch), velocity: MIDIVelocity(velocity))
    }

    func noteOff(_ pitch: UInt8) {
        synth.stop(noteNumber: MIDINoteNumber(pitch))
    }
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
