import SwiftUI

/// Lists the Audio Unit instruments installed on the system. Pick one to load it as the
/// melody voice. Empty? Install some — most free synths/samplers ship an AU component.
struct AUBrowserView: View {
    let onSelect: (AUInstrumentInfo) -> Void
    let onClose: () -> Void

    @State private var instruments: [AUInstrumentInfo] = []
    @State private var filter: String = ""

    private var shown: [AUInstrumentInfo] {
        filter.isEmpty ? instruments
            : instruments.filter { $0.name.localizedCaseInsensitiveContains(filter) || $0.manufacturer.localizedCaseInsensitiveContains(filter) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("audio unit instruments")
                    .font(.custom(Theme.mono, size: 14)).bold().foregroundStyle(Theme.ink)
                Spacer()
                Button("close") { onClose() }
                    .buttonStyle(.plain)
                    .font(.custom(Theme.mono, size: 12)).foregroundStyle(Theme.faded)
            }

            TextField("filter", text: $filter)
                .textFieldStyle(.plain)
                .font(.custom(Theme.mono, size: 12))
                .foregroundStyle(Theme.ink)
                .padding(8)
                .background(Theme.panel)
                .clipShape(RoundedRectangle(cornerRadius: 4))

            if instruments.isEmpty {
                Text("No AU instruments found.\nInstall some (most free plugins ship an Audio Unit), then reopen.")
                    .font(.custom(Theme.mono, size: 11)).foregroundStyle(Theme.faded)
                    .padding(.vertical, 20)
            } else {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 2) {
                        ForEach(shown) { inst in
                            Button { onSelect(inst) } label: {
                                VStack(alignment: .leading, spacing: 1) {
                                    Text(inst.name)
                                        .font(.custom(Theme.mono, size: 12)).foregroundStyle(Theme.ink)
                                    Text(inst.manufacturer)
                                        .font(.custom(Theme.mono, size: 9)).foregroundStyle(Theme.faded)
                                }
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .padding(.horizontal, 8).padding(.vertical, 6)
                                .background(Theme.panel.opacity(0.5))
                                .clipShape(RoundedRectangle(cornerRadius: 4))
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }
            }
        }
        .padding(16)
        .frame(width: 440, height: 480)
        .background(Theme.surface)
        .onAppear { instruments = AudioUnitBrowser.installedInstruments() }
    }
}
