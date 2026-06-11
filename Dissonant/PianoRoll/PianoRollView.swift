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
    /// When true, paint every cell by its fit against the chord at *that* beat — the whole
    /// progression's harmonic map at once (red = dissonant), not just the playhead column.
    var showLandscape: Bool = false

    // Geometry
    private let lowMIDI = 24          // C1 (bass)
    private let highMIDI = 84         // C6
    private let visibleHeight: CGFloat = 380
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
        ScrollViewReader { proxy in
            ScrollView(.vertical, showsIndicators: true) {
                HStack(spacing: 0) {
                    keyboardGutter
                    grid
                }
            }
            .onAppear { proxy.scrollTo(60, anchor: .center) }
        }
        .frame(height: visibleHeight)
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
                .id(pitch)
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
                // base black-key shading
                for pitch in lowMIDI...highMIDI {
                    let rowRect = CGRect(x: 0, y: y(forPitch: pitch), width: size.width, height: rowHeight)
                    ctx.fill(Path(rowRect), with: .color(isBlackKey(pitch) ? Theme.surface : Theme.panel.opacity(0.35)))
                }

                if showLandscape {
                    // whole-progression map: every cell tinted by its fit against the chord at that beat
                    for b in 0..<beats {
                        let map = engine.tierMap(atBeat: Double(b) + 0.5, chordTrack: chordTrack, key: key)
                        for pitch in lowMIDI...highMIDI {
                            guard let tier = map[((pitch % 12) + 12) % 12] else { continue }
                            let cell = CGRect(x: CGFloat(b) * beatWidth, y: y(forPitch: pitch), width: beatWidth, height: rowHeight)
                            let opacity = tier == .dissonance ? 0.40 : (tier == .tension ? 0.26 : 0.16)
                            ctx.fill(Path(cell), with: .color(Theme.color(for: tier).opacity(opacity)))
                            if tier == .dissonance {
                                // non-color cue: a diagonal stripe marks dissonant cells
                                var stripe = Path()
                                stripe.move(to: CGPoint(x: cell.minX, y: cell.maxY))
                                stripe.addLine(to: CGPoint(x: cell.maxX, y: cell.minY))
                                ctx.stroke(stripe, with: .color(Theme.dissonance.opacity(0.6)), lineWidth: 0.9)
                            }
                        }
                    }
                } else {
                    // live: tint each row by the chord under the playhead
                    for pitch in lowMIDI...highMIDI {
                        guard let tier = liveTier(pitch) else { continue }
                        let rowRect = CGRect(x: 0, y: y(forPitch: pitch), width: size.width, height: rowHeight)
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
            .allowsHitTesting(false)

            notesLayer
                .allowsHitTesting(false)
            playhead
        }
        .frame(width: gridWidth, height: gridHeight)
        .overlay(
            RollMouseHandler(
                onLeftDown: { place(at: $0) },
                onLeftDrag: { place(at: $0) },     // left-drag paints
                onRightDown: { delete(at: $0) },
                onRightDrag: { delete(at: $0) }    // right-drag erases
            )
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

    private func cell(at point: CGPoint) -> (beat: Int, pitch: Int)? {
        let beat = Int(point.x / beatWidth)
        let p = pitch(forY: point.y)
        guard beat >= 0, beat < beats, p >= lowMIDI, p <= highMIDI else { return nil }
        return (beat, p)
    }

    /// Place a note at the cell (left-click / paint). No-op if one already exists there, so
    /// dragging across a cell doesn't stack duplicates or re-audition.
    private func place(at point: CGPoint) {
        guard let (beat, p) = cell(at: point) else { return }
        guard !notes.contains(where: { $0.pitch == p && Int($0.startBeat) == beat }) else { return }
        notes.append(NoteEvent(startBeat: Double(beat), lengthBeats: 1, pitch: p))
        onAudition?(p)
    }

    /// Remove the note at the cell (right-click / erase).
    private func delete(at point: CGPoint) {
        guard let (beat, p) = cell(at: point) else { return }
        notes.removeAll { $0.pitch == p && Int($0.startBeat) == beat }
    }
}

/// Thin AppKit bridge so the roll gets FL-style mouse behaviour SwiftUI can't express:
/// place on mouse-DOWN (snappy, not on release), left-drag to paint, right-click/drag to
/// erase — all with the cursor location SwiftUI's tap gestures don't hand back.
struct RollMouseHandler: NSViewRepresentable {
    var onLeftDown: (CGPoint) -> Void
    var onLeftDrag: (CGPoint) -> Void
    var onRightDown: (CGPoint) -> Void
    var onRightDrag: (CGPoint) -> Void

    func makeNSView(context: Context) -> MouseView {
        let v = MouseView()
        v.onLeftDown = onLeftDown
        v.onLeftDrag = onLeftDrag
        v.onRightDown = onRightDown
        v.onRightDrag = onRightDrag
        return v
    }

    func updateNSView(_ nsView: MouseView, context: Context) {
        nsView.onLeftDown = onLeftDown
        nsView.onLeftDrag = onLeftDrag
        nsView.onRightDown = onRightDown
        nsView.onRightDrag = onRightDrag
    }

    final class MouseView: NSView {
        var onLeftDown: ((CGPoint) -> Void)?
        var onLeftDrag: ((CGPoint) -> Void)?
        var onRightDown: ((CGPoint) -> Void)?
        var onRightDrag: ((CGPoint) -> Void)?

        // Match SwiftUI's top-left origin.
        override var isFlipped: Bool { true }

        private func loc(_ event: NSEvent) -> CGPoint {
            convert(event.locationInWindow, from: nil)
        }

        override func mouseDown(with event: NSEvent) { onLeftDown?(loc(event)) }
        override func mouseDragged(with event: NSEvent) { onLeftDrag?(loc(event)) }
        override func rightMouseDown(with event: NSEvent) { onRightDown?(loc(event)) }
        override func rightMouseDragged(with event: NSEvent) { onRightDrag?(loc(event)) }
    }
}
