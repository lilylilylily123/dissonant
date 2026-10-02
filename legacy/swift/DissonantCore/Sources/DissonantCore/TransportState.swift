import Foundation

/// A half-open loop region `[startBeat, endBeat)` in beats.
public struct LoopRegion: Equatable, Sendable {
    public var startBeat: Double
    public var endBeat: Double

    public init(startBeat: Double, endBeat: Double) {
        self.startBeat = startBeat
        self.endBeat = endBeat
    }

    public var lengthBeats: Double { max(0, endBeat - startBeat) }
}

/// Pure model of the transport: whether it's playing, where the playhead is (in beats),
/// and an optional loop region. The audio layer (AppleSequencer) is the real clock;
/// this type holds the logic the audio layer and UI agree on, so it can be tested
/// without sound. The visual playhead reads `positionBeats`.
public struct TransportState: Equatable, Sendable {
    public private(set) var isPlaying: Bool
    public private(set) var positionBeats: Double
    public var loop: LoopRegion?

    public init(isPlaying: Bool = false, positionBeats: Double = 0, loop: LoopRegion? = nil) {
        self.isPlaying = isPlaying
        self.positionBeats = max(0, positionBeats)
        self.loop = loop
    }

    public mutating func play() {
        isPlaying = true
    }

    public mutating func stop() {
        isPlaying = false
    }

    /// Jump the playhead to a beat (never negative).
    public mutating func seek(toBeat beat: Double) {
        positionBeats = max(0, beat)
    }

    /// Advance the playhead by `delta` beats, wrapping within the loop region if one is set.
    /// No-op when stopped. Handles multiple wraps if `delta` exceeds the loop length.
    public mutating func advance(byBeats delta: Double) {
        guard isPlaying, delta > 0 else { return }
        var p = positionBeats + delta
        if let loop, loop.lengthBeats > 0, p >= loop.endBeat {
            let over = (p - loop.startBeat).truncatingRemainder(dividingBy: loop.lengthBeats)
            p = loop.startBeat + over
        }
        positionBeats = p
    }
}
