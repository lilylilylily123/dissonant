import SwiftUI
import DissonantCore

/// Left rail listing the project's tracks. Add, select (click), rename (type), delete.
/// Renaming uses a local buffer committed on Enter/blur so typing doesn't churn the document
/// and steal focus; a `simultaneousGesture` lets the row select without blocking the field.
struct TrackSidebarView: View {
    let tracks: [Track]
    let selectedTrackID: UUID?
    var onAdd: () -> Void
    var onSelect: (UUID) -> Void
    var onRename: (UUID, String) -> Void
    var onDelete: (UUID) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text("tracks")
                    .font(.custom(Theme.mono, size: 11)).bold().foregroundStyle(Theme.ink)
                Spacer()
                Button("+ add") { onAdd() }
                    .buttonStyle(.plain)
                    .font(.custom(Theme.mono, size: 11)).foregroundStyle(Theme.brand)
            }
            .padding(.bottom, 2)

            ForEach(tracks) { track in
                TrackRow(
                    track: track,
                    selected: track.id == selectedTrackID,
                    canDelete: tracks.count > 1,
                    onSelect: { onSelect(track.id) },
                    onRename: { onRename(track.id, $0) },
                    onDelete: { onDelete(track.id) }
                )
            }
            Spacer()
        }
        .frame(width: 176)
        .padding(12)
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

    @State private var editing = false
    @State private var draft = ""
    @FocusState private var focused: Bool

    private var ink: Color { selected ? Theme.surface : Theme.ink }
    private var faded: Color { selected ? Theme.surface.opacity(0.8) : Theme.faded }

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            if editing {
                HStack(spacing: 4) {
                    TextField("name", text: $draft)
                        .textFieldStyle(.plain)
                        .focused($focused)
                        .font(.custom(Theme.mono, size: 12)).foregroundStyle(ink)
                        .onSubmit { finish() }
                        .onExitCommand { editing = false }   // Esc cancels
                        .onAppear { draft = track.name; focused = true }
                    Button("✓") { finish() }
                        .buttonStyle(.plain).font(.custom(Theme.mono, size: 12)).foregroundStyle(ink)
                }
            } else {
                HStack(spacing: 4) {
                    Text(track.name.isEmpty ? "untitled" : track.name)
                        .font(.custom(Theme.mono, size: 12)).foregroundStyle(ink)
                    Spacer(minLength: 4)
                    Button("✎") { startEditing() }
                        .buttonStyle(.plain).font(.custom(Theme.mono, size: 11)).foregroundStyle(faded)
                    if canDelete {
                        Button("×") { onDelete() }
                            .buttonStyle(.plain).font(.custom(Theme.mono, size: 12)).foregroundStyle(faded)
                    }
                }
            }
            Text(track.voice)
                .font(.custom(Theme.mono, size: 9))
                .foregroundStyle(selected ? Theme.surface.opacity(0.7) : Theme.faded)
        }
        .padding(.horizontal, 8).padding(.vertical, 7)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(selected ? Theme.brand : Theme.surface)
        .clipShape(RoundedRectangle(cornerRadius: 5))
        .contentShape(Rectangle())
        .onTapGesture { if !editing { onSelect() } }
    }

    private func startEditing() {
        onSelect()
        draft = track.name
        editing = true
    }

    private func finish() {
        if draft != track.name { onRename(draft) }
        editing = false
    }
}
