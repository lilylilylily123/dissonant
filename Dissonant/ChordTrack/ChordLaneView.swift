import SwiftUI
import DissonantCore

/// The guided chord lane (basic U6): pick a known-good progression starter, then tap any
/// chord to swap it for the next in-key chord and hear the difference. The chord under the
/// playhead is highlighted. Theory names are shown but never required to use it (R9).
struct ChordLaneView: View {
    @Binding var chordTrack: ChordTrackModel
    let playheadBeat: Double

    // Suggestions are drawn from the project key (default C major for v1).
    private let keyRoot = 0
    private let keyScale: ScaleType = .major

    private let beats = 16
    private let beatWidth: CGFloat = 44
    private let gutter: CGFloat = 56

    private var diatonic: [ChordSuggestion] { Harmony.diatonicChords(root: keyRoot, scale: keyScale) }

    private var currentChordID: UUID? { chordTrack.chord(atBeat: playheadBeat)?.id }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 0) {
                Color.clear.frame(width: gutter)
                ZStack(alignment: .topLeading) {
                    ForEach(chordTrack.chords) { chord in
                        chordBlock(chord)
                    }
                }
                .frame(width: CGFloat(beats) * beatWidth, height: 34, alignment: .topLeading)
            }

            HStack(spacing: 8) {
                Text("starters")
                    .font(.custom(Theme.mono, size: 10))
                    .foregroundStyle(Theme.faded)
                ForEach(starters, id: \.name) { starter in
                    Button(starter.name) { chordTrack = ChordTrackModel(chords: starter.build()) }
                        .buttonStyle(.plain)
                        .font(.custom(Theme.mono, size: 11))
                        .padding(.horizontal, 8).padding(.vertical, 4)
                        .background(Theme.panel)
                        .foregroundStyle(Theme.ink)
                        .clipShape(RoundedRectangle(cornerRadius: 4))
                }
            }
        }
    }

    private func chordBlock(_ chord: ChordEvent) -> some View {
        let isCurrent = chord.id == currentChordID
        return Button {
            swap(chord)
        } label: {
            Text(chord.name ?? "?")
                .font(.custom(Theme.mono, size: 13)).bold()
                .foregroundStyle(isCurrent ? Theme.surface : Theme.ink)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(isCurrent ? Theme.brand : Theme.panel)
                .clipShape(RoundedRectangle(cornerRadius: 4))
        }
        .buttonStyle(.plain)
        .frame(width: CGFloat(chord.lengthBeats) * beatWidth - 3, height: 34)
        .offset(x: CGFloat(chord.startBeat) * beatWidth, y: 0)
    }

    /// Swap a chord for the next diatonic chord, keeping its position — a taste of guided swap.
    private func swap(_ chord: ChordEvent) {
        let chords = diatonic
        let idx = chords.firstIndex { $0.name == chord.name } ?? -1
        let next = chords[(idx + 1 + chords.count) % chords.count]
        var updated = chord
        updated.pitchClasses = next.pitchClasses
        updated.name = next.name
        chordTrack.update(updated)
    }

    // MARK: - Starters

    private struct Starter {
        let name: String
        let degrees: [Int]
        let diatonic: [ChordSuggestion]
        func build() -> [ChordEvent] {
            degrees.enumerated().map { i, degree in
                let c = diatonic[degree % diatonic.count]
                return ChordEvent(startBeat: Double(i * 4), lengthBeats: 4, pitchClasses: c.pitchClasses, name: c.name)
            }
        }
    }

    private var starters: [Starter] {
        [
            Starter(name: "I–IV–V–vi", degrees: [0, 3, 4, 5], diatonic: diatonic),
            Starter(name: "I–V–vi–IV", degrees: [0, 4, 5, 3], diatonic: diatonic),
            Starter(name: "vi–IV–I–V", degrees: [5, 3, 0, 4], diatonic: diatonic)
        ]
    }
}
