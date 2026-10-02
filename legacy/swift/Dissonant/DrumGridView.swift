import SwiftUI
import DissonantCore

/// Step sequencer for a drum track. Rows are drums, columns are 8th-note steps. Same FL-style
/// mouse as the piano roll: left-click/drag adds hits, right-click/drag deletes. Hits are
/// `NoteEvent`s (pitch = the drum's kit note) in the pattern model, so they play and arrange
/// through the same pipeline as everything else.
struct DrumGridView: View {
    @Binding var notes: [NoteEvent]
    let patternLength: Double
    let playheadBeat: Double
    var onHit: (Int) -> Void

    private let stepBeats = 0.5
    private let cell: CGFloat = 26
    private let gap: CGFloat = 3
    private let labelW: CGFloat = 56

    private var steps: Int { max(1, Int((patternLength / stepBeats).rounded())) }
    private var pitchStep: CGFloat { cell + gap }
    private var gridW: CGFloat { labelW + CGFloat(steps) * pitchStep }
    private var gridH: CGFloat { CGFloat(DrumKit.voices.count) * pitchStep }
    private var currentStep: Int { Int((playheadBeat / stepBeats).rounded(.down)) }

    private func beat(_ step: Int) -> Double { Double(step) * stepBeats }
    private func isOn(_ pitch: Int, _ step: Int) -> Bool {
        notes.contains { $0.pitch == pitch && abs($0.startBeat - beat(step)) < 0.001 }
    }

    private func cellAt(_ point: CGPoint) -> (pitch: Int, step: Int)? {
        let col = Int((point.x - labelW) / pitchStep)
        let row = Int(point.y / pitchStep)
        guard col >= 0, col < steps, row >= 0, row < DrumKit.voices.count else { return nil }
        return (DrumKit.voices[row].pitch, col)
    }
    private func add(_ point: CGPoint) {
        guard let (pitch, step) = cellAt(point), !isOn(pitch, step) else { return }
        notes.append(NoteEvent(startBeat: beat(step), lengthBeats: stepBeats, pitch: pitch))
        onHit(pitch)
    }
    private func remove(_ point: CGPoint) {
        guard let (pitch, step) = cellAt(point) else { return }
        notes.removeAll { $0.pitch == pitch && abs($0.startBeat - beat(step)) < 0.001 }
    }

    var body: some View {
        ScrollView(.horizontal, showsIndicators: true) {
            ZStack(alignment: .topLeading) {
                ForEach(Array(DrumKit.voices.enumerated()), id: \.offset) { row, drum in
                    Text(drum.name)
                        .font(.custom(Theme.mono, size: 12)).foregroundStyle(Theme.ink)
                        .frame(width: labelW, height: cell, alignment: .leading)
                        .offset(x: 0, y: CGFloat(row) * pitchStep)

                    ForEach(0..<steps, id: \.self) { step in
                        cellView(drum.pitch, step)
                            .offset(x: labelW + CGFloat(step) * pitchStep, y: CGFloat(row) * pitchStep)
                    }
                }

                RollMouseHandler(
                    onLeftDown: { add($0) },
                    onLeftDrag: { add($0) },
                    onRightDown: { remove($0) },
                    onRightDrag: { remove($0) }
                )
                .frame(width: gridW, height: gridH)
            }
            .frame(width: gridW, height: gridH)
            .padding(10)
        }
        .background(Theme.surface)
        .clipShape(RoundedRectangle(cornerRadius: 6))
        .frame(maxHeight: 200)
    }

    private func cellView(_ pitch: Int, _ step: Int) -> some View {
        let on = isOn(pitch, step)
        let onBeat = (Double(step) * stepBeats).truncatingRemainder(dividingBy: 1) == 0
        return RoundedRectangle(cornerRadius: 3)
            .fill(on ? Theme.brand : (onBeat ? Theme.panel : Theme.panel.opacity(0.45)))
            .frame(width: cell, height: cell)
            .overlay(
                RoundedRectangle(cornerRadius: 3)
                    .stroke(Theme.ink.opacity(step == currentStep ? 0.7 : 0), lineWidth: 1.5)
            )
    }
}
