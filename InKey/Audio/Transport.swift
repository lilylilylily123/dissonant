import Foundation
import AudioKit
import Combine
import InKeyCore

/// Drives playback and exposes a main-thread-safe playhead position for the UI.
/// The pure `TransportState` (InKeyCore) holds play/position/loop logic; `AppleSequencer`
/// is the audio clock. The visual playhead reads `state.positionBeats`, polled ~60fps on
/// the main thread — never updated from the audio render thread.
@MainActor
final class Transport: ObservableObject {
    @Published private(set) var state = TransportState()
    @Published var tempo = Tempo(bpm: 120) {
        didSet { sequencer.setTempo(tempo.bpm) }
    }

    private let sequencer = AppleSequencer()
    private var pollTimer: Timer?

    init() {
        sequencer.setTempo(tempo.bpm)
    }

    func play() {
        state.play()
        sequencer.play()
        startPolling()
    }

    func stop() {
        sequencer.stop()
        state.stop()
        stopPolling()
    }

    private func startPolling() {
        pollTimer?.invalidate()
        pollTimer = Timer.scheduledTimer(withTimeInterval: 1.0 / 60.0, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in
                guard let self else { return }
                self.state.seek(toBeat: self.sequencer.currentPosition.beats)
            }
        }
    }

    private func stopPolling() {
        pollTimer?.invalidate()
        pollTimer = nil
    }
}
