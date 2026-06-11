import Foundation

/// Converts between musical beats and wall-clock seconds at a fixed tempo.
/// Pure value type so the conversion is unit-testable without any audio engine.
public struct Tempo: Equatable, Sendable {
    public var bpm: Double

    public init(bpm: Double = 120) {
        self.bpm = bpm
    }

    /// Seconds occupied by `beats` beats at this tempo.
    public func seconds(forBeats beats: Double) -> Double {
        beats * 60.0 / bpm
    }

    /// Beats elapsed over `seconds` seconds at this tempo.
    public func beats(forSeconds seconds: Double) -> Double {
        seconds * bpm / 60.0
    }
}
