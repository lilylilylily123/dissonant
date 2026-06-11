import Foundation
import AVFoundation
import DissonantCore

/// A drum kit synthesized into one-shot buffers at runtime (no bundled samples). Each drum is
/// a `MidiPlayable` voice triggered by its kit pitch; hits are scheduled on a per-drum player
/// node. Kick = pitched sine drop, snare = tone+noise, hat/clap = shaped noise.
@MainActor
final class DrumInstrument: MidiPlayable {
    private let format: AVAudioFormat
    private var players: [Int: AVAudioPlayerNode] = [:]
    private var buffers: [Int: AVAudioPCMBuffer] = [:]

    init() {
        let sampleRate = 44_100.0
        format = AVAudioFormat(standardFormatWithSampleRate: sampleRate, channels: 1)!
        for drum in DrumKit.voices {
            buffers[drum.pitch] = Self.makeBuffer(name: drum.name, format: format, sampleRate: sampleRate)
            players[drum.pitch] = AVAudioPlayerNode()
        }
    }

    /// Attach every drum player into the engine graph and start them (idle until scheduled).
    func attach(to engine: AVAudioEngine, mixer: AVAudioNode) {
        for player in players.values {
            engine.attach(player)
            engine.connect(player, to: mixer, format: format)
        }
    }

    func start() {
        for player in players.values { player.play() }
    }

    func noteOn(_ pitch: UInt8, velocity: UInt8) {
        let key = Int(pitch)
        guard let player = players[key], let buffer = buffers[key] else { return }
        if !player.isPlaying { player.play() }
        player.scheduleBuffer(buffer, at: nil, options: .interrupts, completionHandler: nil)
    }

    func noteOff(_ pitch: UInt8) {}  // one-shots ring out on their own

    // MARK: - Procedural one-shots

    private static func makeBuffer(name: String, format: AVAudioFormat, sampleRate: Double) -> AVAudioPCMBuffer {
        let duration: Double
        switch name {
        case "kick": duration = 0.34
        case "snare", "clap": duration = 0.20
        default: duration = 0.06   // hat
        }
        let frames = AVAudioFrameCount(duration * sampleRate)
        let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frames)!
        buffer.frameLength = frames
        let out = buffer.floatChannelData![0]

        for i in 0..<Int(frames) {
            let t = Double(i) / sampleRate
            var s = 0.0
            switch name {
            case "kick":
                let freq = 110.0 * exp(-t * 28) + 45      // pitch drop into a thud
                s = sin(2 * .pi * freq * t) * exp(-t * 8)
            case "snare":
                let tone = sin(2 * .pi * 185 * t) * 0.4
                let noise = Double.random(in: -1...1)
                s = (tone + noise * 0.9) * exp(-t * 20)
            case "clap":
                // a couple of fast noise bursts then a tail
                let burst = (sin(2 * .pi * 50 * t) > 0 || t > 0.03) ? 1.0 : 0.4
                s = Double.random(in: -1...1) * burst * exp(-t * 22)
            default: // hat
                s = Double.random(in: -1...1) * exp(-t * 65)
            }
            out[i] = Float(s * 0.7)
        }
        return buffer
    }
}
