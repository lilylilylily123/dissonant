import Foundation
import AudioKit
import Combine
import DissonantCore

/// Drives playback and exposes a main-thread-safe playhead position for the UI.
///
/// The pure `TransportState` (DissonantCore) holds play/position/loop logic. For now the
/// playhead is advanced from a wall-clock tick scaled by tempo — guaranteed to move even
/// before any audio content exists. Once U5 schedules real notes into `AppleSequencer`,
/// the clock source switches to `sequencer.currentPosition` for sample-accurate sync.
/// The position is only ever updated on the main thread, never from the audio render thread.
@MainActor
final class Transport: ObservableObject {
    @Published private(set) var state = TransportState(loop: LoopRegion(startBeat: 0, endBeat: 16))
    @Published var tempo = Tempo(bpm: 120) {
        didSet { sequencer.setTempo(tempo.bpm) }
    }

    private let sequencer = AppleSequencer()
    private var pollTimer: Timer?
    private var lastTick: Date?

    init() {
        sequencer.setTempo(tempo.bpm)
        sequencer.setLength(Duration(beats: 16))
        sequencer.enableLooping()
    }

    func play() {
        state.play()
        lastTick = Date()
        sequencer.play()
        startPolling()
    }

    func stop() {
        sequencer.stop()
        state.stop()
        stopPolling()
        lastTick = nil
    }

    /// Return the playhead to the start. Keeps playing if it was playing.
    func rewind() {
        sequencer.rewind()
        state.seek(toBeat: 0)
        if state.isPlaying { lastTick = Date() }
    }

    /// Set the loop length in beats (pattern length in pattern mode, song length in song mode).
    func setLength(_ beats: Double) {
        let length = max(1, beats)
        state.loop = LoopRegion(startBeat: 0, endBeat: length)
        sequencer.setLength(Duration(beats: length))
        if state.positionBeats >= length { state.seek(toBeat: 0) }
    }

    private func startPolling() {
        pollTimer?.invalidate()
        pollTimer = Timer.scheduledTimer(withTimeInterval: 1.0 / 60.0, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in
                guard let self, let last = self.lastTick else { return }
                let now = Date()
                self.state.advance(byBeats: self.tempo.beats(forSeconds: now.timeIntervalSince(last)))
                self.lastTick = now
            }
        }
    }

    private func stopPolling() {
        pollTimer?.invalidate()
        pollTimer = nil
    }
}
