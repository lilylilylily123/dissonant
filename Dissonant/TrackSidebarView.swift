import SwiftUI
import DissonantCore

/// Left rail listing the project's tracks. Add, select (click), rename (type), and delete.
/// The selected track is the one the piano roll edits.
struct TrackSidebarView: View {
    @Binding var tracks: [Track]
    var selectedTrackID: UUID?
    var onAdd: () -> Void
    var onSelect: (UUID) -> Void
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

            ForEach($tracks) { $track in
                row($track)
            }
            Spacer()
        }
        .frame(width: 176)
        .padding(12)
        .background(Theme.panel)
    }

    private func row(_ track: Binding<Track>) -> some View {
        let id = track.wrappedValue.id
        let selected = id == selectedTrackID
        return VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 4) {
                TextField("name", text: track.name)
                    .textFieldStyle(.plain)
                    .font(.custom(Theme.mono, size: 12))
                    .foregroundStyle(selected ? Theme.surface : Theme.ink)
                if tracks.count > 1 {
                    Button("×") { onDelete(id) }
                        .buttonStyle(.plain)
                        .font(.custom(Theme.mono, size: 12))
                        .foregroundStyle(selected ? Theme.surface.opacity(0.8) : Theme.faded)
                }
            }
            Text(track.wrappedValue.voice)
                .font(.custom(Theme.mono, size: 9))
                .foregroundStyle(selected ? Theme.surface.opacity(0.7) : Theme.faded)
        }
        .padding(.horizontal, 8).padding(.vertical, 7)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(selected ? Theme.brand : Theme.surface)
        .clipShape(RoundedRectangle(cornerRadius: 5))
        .contentShape(Rectangle())
        .onTapGesture { onSelect(id) }
    }
}
