import SwiftUI
import DissonantCore

/// Free-build / alter any chord (U7). Tap pitch classes to add or remove them; the color of
/// each candidate shows how it fits the notes already stacked — solid = in the chord, amber =
/// spicy-but-good tension, red = harsh half-step clash. Non-diatonic chords are fully allowed
/// (go weird on purpose); the result drives the roll's highlighting like any other chord.
struct ChordEditorView: View {
    @Binding var chordTrack: ChordTrackModel
    let chordID: UUID

    private let classifier = TierClassifier()

    private var chord: ChordEvent? { chordTrack.chords.first { $0.id == chordID } }
    private var pcs: [Int] { chord?.pitchClasses ?? [] }

    private let columns = Array(repeating: GridItem(.fixed(38), spacing: 5), count: 6)

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(Harmony.chordName(forPitchClasses: pcs))
                .font(.custom(Theme.mono, size: 18)).bold()
                .foregroundStyle(Theme.brand)
            Text("tap notes to build · color = how it fits")
                .font(.custom(Theme.mono, size: 9)).foregroundStyle(Theme.faded)

            LazyVGrid(columns: columns, spacing: 5) {
                ForEach(0..<12, id: \.self) { pc in pcButton(pc) }
            }

            HStack(spacing: 8) {
                editButton("next ▸ diatonic") { swapToNextDiatonic() }
                editButton("clear") { setPitchClasses([]) }
            }
        }
        .padding(14)
        .frame(width: 288)
        .background(Theme.panel)
    }

    private func pcButton(_ pc: Int) -> some View {
        let inChord = pcs.contains(pc)
        let fit: Tier? = inChord ? .chordTone : (pcs.isEmpty ? nil : classifier.tier(pitchClass: pc, chordPitchClasses: pcs))
        let bg: Color = inChord ? Theme.solid : (fit == nil ? Theme.surface : Theme.color(for: fit).opacity(0.38))
        return Button { toggle(pc) } label: {
            Text(Harmony.noteName(pc))
                .font(.custom(Theme.mono, size: 12)).bold()
                .foregroundStyle(inChord ? Theme.surface : Theme.ink)
                .frame(width: 38, height: 30)
                .background(bg)
                .overlay(alignment: .topTrailing) {
                    if !inChord, TierCue.cue(for: fit) == .hatch {
                        Text("!").font(.custom(Theme.mono, size: 8)).bold().foregroundStyle(Theme.dissonance).padding(1)
                    }
                }
                .clipShape(RoundedRectangle(cornerRadius: 4))
        }
        .buttonStyle(.plain)
    }

    private func editButton(_ label: String, _ action: @escaping () -> Void) -> some View {
        Button(label, action: action)
            .buttonStyle(.plain)
            .font(.custom(Theme.mono, size: 10))
            .foregroundStyle(Theme.faded)
            .padding(.horizontal, 8).padding(.vertical, 5)
            .background(Theme.surface)
            .clipShape(RoundedRectangle(cornerRadius: 4))
    }

    // MARK: - Edits

    private func toggle(_ pc: Int) {
        var set = pcs
        if let i = set.firstIndex(of: pc) { set.remove(at: i) } else { set.append(pc) }
        setPitchClasses(set)
    }

    private func setPitchClasses(_ set: [Int]) {
        guard var c = chord else { return }
        c.pitchClasses = set.sorted()
        c.name = Harmony.chordName(forPitchClasses: set)
        chordTrack.update(c)
    }

    private func swapToNextDiatonic() {
        let diatonic = Harmony.diatonicChords(root: 0, scale: .major)
        guard let c = chord else { return }
        let idx = diatonic.firstIndex { $0.name == c.name } ?? -1
        let next = diatonic[(idx + 1 + diatonic.count) % diatonic.count]
        setPitchClasses(next.pitchClasses)
    }
}
