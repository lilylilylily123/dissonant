import SwiftUI
import DissonantCore

/// Song mode: arrange patterns into a full track. Click a pattern in the palette to append it
/// to the song; click a block in the song row to remove it. A playhead sweeps during playback.
struct SongView: View {
    let patterns: [SongPattern]
    @Binding var arrangement: [UUID]
    let playheadBeat: Double
    let patternLength: Double

    private let blockWidth: CGFloat = 96
    private let blockGap: CGFloat = 3

    private func name(_ id: UUID) -> String { patterns.first { $0.id == id }?.name ?? "?" }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            VStack(alignment: .leading, spacing: 7) {
                Text("patterns — click to add to the song")
                    .font(.custom(Theme.mono, size: 11)).foregroundStyle(Theme.faded)
                HStack(spacing: 8) {
                    ForEach(patterns) { pattern in
                        Button(pattern.name) { arrangement.append(pattern.id) }
                            .buttonStyle(.plain)
                            .font(.custom(Theme.mono, size: 13)).foregroundStyle(Theme.ink)
                            .padding(.horizontal, 12).padding(.vertical, 8)
                            .background(Theme.panel)
                            .clipShape(RoundedRectangle(cornerRadius: 5))
                    }
                }
            }

            VStack(alignment: .leading, spacing: 7) {
                Text("song — click a block to remove")
                    .font(.custom(Theme.mono, size: 11)).foregroundStyle(Theme.faded)
                ScrollView(.horizontal, showsIndicators: true) {
                    ZStack(alignment: .topLeading) {
                        HStack(spacing: blockGap) {
                            ForEach(Array(arrangement.enumerated()), id: \.offset) { index, pid in
                                Button { arrangement.remove(at: index) } label: {
                                    Text(name(pid))
                                        .font(.custom(Theme.mono, size: 12)).bold()
                                        .foregroundStyle(Theme.ink)
                                        .frame(width: blockWidth, height: 56)
                                        .background(Theme.panel)
                                        .overlay(RoundedRectangle(cornerRadius: 5).stroke(Theme.brand.opacity(0.4), lineWidth: 1))
                                        .clipShape(RoundedRectangle(cornerRadius: 5))
                                }
                                .buttonStyle(.plain)
                            }
                            if arrangement.isEmpty {
                                Text("add patterns above to build your song →")
                                    .font(.custom(Theme.mono, size: 11)).foregroundStyle(Theme.faded)
                                    .frame(height: 56)
                            }
                        }
                        Rectangle()
                            .fill(Theme.brand)
                            .frame(width: 2, height: 62)
                            .offset(x: CGFloat(playheadBeat / max(patternLength, 1)) * (blockWidth + blockGap))
                            .allowsHitTesting(false)
                    }
                    .padding(.bottom, 8)
                }
                .frame(height: 76)
            }
            Spacer()
        }
    }
}
