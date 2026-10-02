import SwiftUI
import DissonantCore

/// The live "notes you could play right now" readout (R13): every pitch class as a chip,
/// tier-colored to the chord under the playhead. Reads from the same HighlightEngine as the
/// roll, so the two surfaces always agree.
struct PlayableNowView: View {
    let chordTrack: ChordTrackModel
    let key: KeyState
    let playheadBeat: Double

    private let engine = HighlightEngine()

    private var tierMap: [Tier?] {
        engine.tierMap(atBeat: playheadBeat, chordTrack: chordTrack, key: key)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("playable now")
                .font(.custom(Theme.mono, size: 10))
                .foregroundStyle(Theme.faded)
            HStack(spacing: 4) {
                ForEach(0..<12, id: \.self) { pc in
                    chip(pc, tier: tierMap[pc])
                }
            }
        }
    }

    private func chip(_ pc: Int, tier: Tier?) -> some View {
        let color = Theme.color(for: tier)
        return Text(Harmony.noteName(pc))
            .font(.custom(Theme.mono, size: 11))
            .foregroundStyle(tier == nil ? Theme.faded : Theme.surface)
            .frame(width: 30, height: 24)
            .background(tier == nil ? Theme.panel : color)
            .overlay(alignment: .topTrailing) {
                // non-color cue: a corner mark for tension, a stripe for dissonance
                switch TierCue.cue(for: tier) {
                case .none: EmptyView()
                case .dot: Circle().fill(Theme.surface).frame(width: 4, height: 4).padding(2)
                case .hatch: Text("!").font(.custom(Theme.mono, size: 8)).bold().foregroundStyle(Theme.surface).padding(1)
                }
            }
            .clipShape(RoundedRectangle(cornerRadius: 3))
    }
}
