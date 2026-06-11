import SwiftUI
import DissonantCore

/// Step sequencer for a drum track. Rows are drums, columns are 8th-note steps across the
/// pattern. Toggling a cell adds/removes a `NoteEvent` at that step (pitch = the drum's kit
/// note), so drum hits live in the same pattern model and play through the same pipeline.
struct DrumGridView: View {
    @Binding var notes: [NoteEvent]
    let patternLength: Double
    let playheadBeat: Double
    var onHit: (Int) -> Void

    private let stepBeats = 0.5            // 8th notes
    private let cell: CGFloat = 26
    private let labelWidth: CGFloat = 56

    private var steps: Int { max(1, Int((patternLength / stepBeats).rounded())) }
    private var currentStep: Int { Int((playheadBeat / stepBeats).rounded(.down)) }

    private func beat(_ step: Int) -> Double { Double(step) * stepBeats }
    private func isOn(_ pitch: Int, _ step: Int) -> Bool {
        notes.contains { $0.pitch == pitch && abs($0.startBeat - beat(step)) < 0.001 }
    }
    private func toggle(_ pitch: Int, _ step: Int) {
        let b = beat(step)
        if let idx = notes.firstIndex(where: { $0.pitch == pitch && abs($0.startBeat - b) < 0.001 }) {
            notes.remove(at: idx)
        } else {
            notes.append(NoteEvent(startBeat: b, lengthBeats: stepBeats, pitch: pitch))
            onHit(pitch)
        }
    }

    var body: some View {
        ScrollView(.horizontal, showsIndicators: true) {
            VStack(alignment: .leading, spacing: 4) {
                ForEach(DrumKit.voices) { drum in
                    HStack(spacing: 3) {
                        Text(drum.name)
                            .font(.custom(Theme.mono, size: 12))
                            .foregroundStyle(Theme.ink)
                            .frame(width: labelWidth, alignment: .leading)
                        ForEach(0..<steps, id: \.self) { step in
                            cellView(drum.pitch, step)
                        }
                    }
                }
            }
            .padding(10)
        }
        .background(Theme.surface)
        .clipShape(RoundedRectangle(cornerRadius: 6))
        .frame(maxHeight: 200)
    }

    private func cellView(_ pitch: Int, _ step: Int) -> some View {
        let on = isOn(pitch, step)
        let onBeat = (Double(step) * stepBeats).truncatingRemainder(dividingBy: 1) == 0
        let isCurrent = step == currentStep
        return RoundedRectangle(cornerRadius: 3)
            .fill(on ? Theme.brand : (onBeat ? Theme.panel : Theme.panel.opacity(0.45)))
            .frame(width: cell, height: cell)
            .overlay(
                RoundedRectangle(cornerRadius: 3)
                    .stroke(isCurrent ? Theme.ink.opacity(0.7) : Color.clear, lineWidth: 1.5)
            )
            .contentShape(Rectangle())
            .onTapGesture { toggle(pitch, step) }
    }
}
