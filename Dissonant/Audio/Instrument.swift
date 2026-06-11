import AudioKit

/// A playable instrument voice — an AudioKit `Node` plus note on/off. Concrete voices:
/// `SynthInstrument` (preset synths), `SamplerInstrument` (SF2), and AU-hosted instruments.
@MainActor
protocol Instrument: AnyObject {
    var node: Node { get }
    func noteOn(_ pitch: UInt8, velocity: UInt8)
    func noteOff(_ pitch: UInt8)
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
