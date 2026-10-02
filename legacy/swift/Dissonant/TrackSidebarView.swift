import SwiftUI
import DissonantCore

/// Left rail listing the project's tracks: add, select (click), rename (✎ then Enter/✓),
/// delete (×), and mute/solo (M/S). The selected track is the one the piano roll edits.
struct TrackSidebarView: View {
    let tracks: [Track]
    let selectedTrackID: UUID?
    var onAdd: () -> Void
    var onAddDrum: () -> Void
    var onSelect: (UUID) -> Void
    var onRename: (UUID, String) -> Void
    var onDelete: (UUID) -> Void
    var onToggleMute: (UUID) -> Void
    var onToggleSolo: (UUID) -> Void
    var onMove: (UUID, Bool) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 7) {
            Text("tracks")
                .font(.custom(Theme.mono, size: 13)).bold().foregroundStyle(Theme.ink)
            HStack(spacing: 6) {
                Button("+ inst") { onAdd() }
                    .buttonStyle(.plain)
                    .font(.custom(Theme.mono, size: 12)).foregroundStyle(Theme.brand)
                Button("+ drum") { onAddDrum() }
                    .buttonStyle(.plain)
                    .font(.custom(Theme.mono, size: 12)).foregroundStyle(Theme.brand)
                Spacer()
            }
            .padding(.bottom, 2)

            ForEach(tracks) { track in
                TrackRow(
                    track: track,
                    selected: track.id == selectedTrackID,
                    canDelete: tracks.count > 1,
                    onSelect: { onSelect(track.id) },
                    onRename: { onRename(track.id, $0) },
                    onDelete: { onDelete(track.id) },
                    onToggleMute: { onToggleMute(track.id) },
                    onToggleSolo: { onToggleSolo(track.id) },
                    onMoveUp: { onMove(track.id, true) },
                    onMoveDown: { onMove(track.id, false) }
                )
            }
            Spacer()
        }
        .frame(width: 200)
        .padding(14)
        .background(Theme.panel)
    }
}

private struct TrackRow: View {
    let track: Track
    let selected: Bool
    let canDelete: Bool
    var onSelect: () -> Void
    var onRename: (String) -> Void
    var onDelete: () -> Void
    var onToggleMute: () -> Void
    var onToggleSolo: () -> Void
    var onMoveUp: () -> Void
    var onMoveDown: () -> Void

    @State private var editing = false
    @State private var draft = ""
    @FocusState private var focused: Bool

    private var ink: Color { selected ? Theme.surface : Theme.ink }
    private var faded: Color { selected ? Theme.surface.opacity(0.8) : Theme.faded }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            if editing {
                HStack(spacing: 5) {
                    TextField("name", text: $draft)
                        .textFieldStyle(.plain).focused($focused)
                        .font(.custom(Theme.mono, size: 13)).foregroundStyle(ink)
                        .onSubmit { finish() }
                        .onExitCommand { editing = false }
                        .onAppear { draft = track.name; focused = true }
                    Button("✓") { finish() }
                        .buttonStyle(.plain).font(.custom(Theme.mono, size: 13)).foregroundStyle(ink)
                }
            } else {
                HStack(spacing: 5) {
                    Text(track.name.isEmpty ? "untitled" : track.name)
                        .font(.custom(Theme.mono, size: 13)).foregroundStyle(ink)
                    Spacer(minLength: 4)
                    Button("▲") { onMoveUp() }
                        .buttonStyle(.plain).font(.custom(Theme.mono, size: 9)).foregroundStyle(faded)
                    Button("▼") { onMoveDown() }
                        .buttonStyle(.plain).font(.custom(Theme.mono, size: 9)).foregroundStyle(faded)
                    Button("✎") { startEditing() }
                        .buttonStyle(.plain).font(.custom(Theme.mono, size: 12)).foregroundStyle(faded)
                    if canDelete {
                        Button("×") { onDelete() }
                            .buttonStyle(.plain).font(.custom(Theme.mono, size: 14)).foregroundStyle(faded)
                    }
                }
            }

            HStack(spacing: 5) {
                Text(track.isDrum ? "drums" : track.voice)
                    .font(.custom(Theme.mono, size: 10))
                    .foregroundStyle(selected ? Theme.surface.opacity(0.7) : Theme.faded)
                Spacer()
                toggle("M", on: track.muted, color: Theme.dissonance, action: onToggleMute)
                toggle("S", on: track.soloed, color: Theme.brand, action: onToggleSolo)
            }
        }
        .padding(.horizontal, 9).padding(.vertical, 8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(selected ? Theme.brand : Theme.surface)
        .clipShape(RoundedRectangle(cornerRadius: 5))
        .contentShape(Rectangle())
        .onTapGesture { if !editing { onSelect() } }
    }

    private func toggle(_ label: String, on: Bool, color: Color, action: @escaping () -> Void) -> some View {
        Button(label, action: action)
            .buttonStyle(.plain)
            .font(.custom(Theme.mono, size: 10)).bold()
            .foregroundStyle(on ? Theme.surface : faded)
            .frame(width: 18, height: 16)
            .background(on ? color : Color.clear)
            .overlay(RoundedRectangle(cornerRadius: 3).stroke(faded.opacity(0.5), lineWidth: on ? 0 : 0.8))
            .clipShape(RoundedRectangle(cornerRadius: 3))
    }

    private func startEditing() {
        onSelect(); draft = track.name; editing = true
    }

    private func finish() {
        if draft != track.name { onRename(draft) }
        editing = false
    }
}
