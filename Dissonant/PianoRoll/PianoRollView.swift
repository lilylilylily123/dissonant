import SwiftUI
import DissonantCore

/// Hand-rolled piano roll: a dark beat×pitch grid where notes are tier-colored, the keyboard
/// gutter and grid recolor live to the chord under the playhead, and tapping places or removes
/// notes. Tiering is visual only — any note can be placed in any tier (R11).
struct PianoRollView: View {
    @Binding var notes: [NoteEvent]
    let chordTrack: ChordTrackModel
    let key: KeyState
    let playheadBeat: Double

    var onAudition: ((Int) -> Void)? = nil

    // Geometry
    private let lowMIDI = 48          // C3
    private let highMIDI = 72         // C5
    private let beats = 16
    private let rowHeight: CGFloat = 16
    private let beatWidth: CGFloat = 44
    private let gutter: CGFloat = 56

    private let engine = HighlightEngine()

    private var rowCount: Int { highMIDI - lowMIDI + 1 }
    private var gridWidth: CGFloat { CGFloat(beats) * beatWidth }
    private var gridHeight: CGFloat { CGFloat(rowCount) * rowHeight }

    private func y(forPitch pitch: Int) -> CGFloat { CGFloat(highMIDI - pitch) * rowHeight }
    private func pitch(forY y: CGFloat) -> Int { highMIDI - Int(y / rowHeight) }
    private func isBlackKey(_ pitch: Int) -> Bool { [1, 3, 6, 8, 10].contains(((pitch % 12) + 12) % 12) }

    /// Live tier for a pitch row at the playhead — drives the keyboard + grid sweep.
    private func liveTier(_ pitch: Int) -> Tier? {
        engine.tier(forPitch: pitch, atBeat: playheadBeat, chordTrack: chordTrack, key: key)
    }

    var body: some View {
        HStack(spacing: 0) {
            keyboardGutter
            grid
        }
        .background(Theme.surface)
        .clipShape(RoundedRectangle(cornerRadius: 6))
    }

    // MARK: - Keyboard gutter (note names, tinted live by tier)

    private var keyboardGutter: some View {
        VStack(spacing: 0) {
            ForEach((lowMIDI...highMIDI).reversed(), id: \.self) { pitch in
                let tier = liveTier(pitch)
                HStack(spacing: 4) {
                    Text(noteLabel(pitch))
                        .font(.custom(Theme.mono, size: 9))
                        .foregroundStyle(Theme.ink)
                    Spacer(minLength: 0)
                    Circle()
                        .fill(Theme.color(for: tier))
                        .frame(width: 5, height: 5)
                        .opacity(tier == nil ? 0.15 : 1)
                }
                .padding(.horizontal, 5)
                .frame(width: gutter, height: rowHeight)
                .background(isBlackKey(pitch) ? Theme.surface : Theme.panel)
            }
        }
    }

    private func noteLabel(_ pitch: Int) -> String {
        "\(Harmony.noteName(pitch))\(pitch / 12 - 1)"
    }

    // MARK: - Grid

    private var grid: some View {
        ZStack(alignment: .topLeading) {
            Canvas { ctx, size in
                // row tints (live tier at playhead) + black-key shading
                for pitch in lowMIDI...highMIDI {
                    let rowRect = CGRect(x: 0, y: y(forPitch: pitch), width: size.width, height: rowHeight)
                    if isBlackKey(pitch) {
                        ctx.fill(Path(rowRect), with: .color(Theme.surface))
                    } else {
                        ctx.fill(Path(rowRect), with: .color(Theme.panel.opacity(0.35)))
                    }
                    if let tier = liveTier(pitch) {
                        ctx.fill(Path(rowRect), with: .color(Theme.color(for: tier).opacity(0.16)))
                    }
                }
                // beat gridlines (heavier every 4)
                for b in 0...beats {
                    let x = CGFloat(b) * beatWidth
                    var path = Path()
                    path.move(to: CGPoint(x: x, y: 0))
                    path.addLine(to: CGPoint(x: x, y: size.height))
                    ctx.stroke(path, with: .color(Theme.gridLine.opacity(b % 4 == 0 ? 1 : 0.4)),
                               lineWidth: b % 4 == 0 ? 1.2 : 0.6)
                }
            }
            .frame(width: gridWidth, height: gridHeight)

            notesLayer
            playhead
        }
        .frame(width: gridWidth, height: gridHeight)
        .contentShape(Rectangle())
        .gesture(
            SpatialTapGesture().onEnded { value in
                toggleNote(at: value.location)
            }
        )
    }

    private var notesLayer: some View {
        ForEach(notes) { note in
            let tier = engine.tier(forPitch: note.pitch, atBeat: note.startBeat, chordTrack: chordTrack, key: key)
            noteBlock(tier: tier)
                .frame(width: CGFloat(note.lengthBeats) * beatWidth - 2, height: rowHeight - 2)
                .position(
                    x: CGFloat(note.startBeat) * beatWidth + CGFloat(note.lengthBeats) * beatWidth / 2,
                    y: y(forPitch: note.pitch) + rowHeight / 2
                )
        }
    }

    @ViewBuilder
    private func noteBlock(tier: Tier?) -> some View {
        ZStack {
            RoundedRectangle(cornerRadius: 2).fill(Theme.color(for: tier))
            switch TierCue.cue(for: tier) {
            case .none:
                EmptyView()
            case .dot:
                Circle().fill(Theme.surface.opacity(0.85)).frame(width: 4, height: 4)
            case .hatch:
                HatchOverlay()
                Text("!").font(.custom(Theme.mono, size: 9)).bold().foregroundStyle(Theme.surface)
            }
        }
        .overlay(RoundedRectangle(cornerRadius: 2).stroke(Theme.surface.opacity(0.6), lineWidth: 0.5))
    }

    private var playhead: some View {
        Rectangle()
            .fill(Theme.brand)
            .frame(width: 1.5, height: gridHeight)
            .position(x: CGFloat(playheadBeat) * beatWidth, y: gridHeight / 2)
            .allowsHitTesting(false)
    }

    // MARK: - Interaction

    private func toggleNote(at point: CGPoint) {
        let beat = Int(point.x / beatWidth)
        let p = pitch(forY: point.y)
        guard beat >= 0, beat < beats, p >= lowMIDI, p <= highMIDI else { return }

        if let idx = notes.firstIndex(where: { $0.pitch == p && Int($0.startBeat) == beat }) {
            notes.remove(at: idx)
        } else {
            notes.append(NoteEvent(startBeat: Double(beat), lengthBeats: 1, pitch: p))
            onAudition?(p)
        }
    }
}
