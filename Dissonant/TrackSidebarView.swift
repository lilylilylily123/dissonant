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

    @State private var name: String = ""
    @FocusState private var focused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 4) {
                TextField("name", text: $name)
                    .textFieldStyle(.plain)
                    .focused($focused)
                    .font(.custom(Theme.mono, size: 12))
                    .foregroundStyle(selected ? Theme.surface : Theme.ink)
                    .onSubmit { commit() }
                if canDelete {
                    Button("×") { onDelete() }
                        .buttonStyle(.plain)
                        .font(.custom(Theme.mono, size: 12))
                        .foregroundStyle(selected ? Theme.surface.opacity(0.8) : Theme.faded)
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
        .simultaneousGesture(TapGesture().onEnded { onSelect() })
        .onAppear { name = track.name }
        .onChange(of: focused) { _, isFocused in if !isFocused { commit() } }
        .onChange(of: track.name) { _, newValue in if !focused { name = newValue } }
    }

    private func commit() {
        if name != track.name { onRename(name) }
    }
}
