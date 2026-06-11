import SwiftUI
import UniformTypeIdentifiers
import DissonantCore

extension UTType {
    /// The "dissonant" project document type, declared in Info.plist (UTExportedTypeDeclarations).
    static let dissonantProject = UTType(exportedAs: "com.crackerjack.dissonant.project")
}

/// Wraps `ProjectModel` (from DissonantCore) as a SwiftUI document. Persistence is plain JSON
/// via Codable — deliberately not SwiftData, whose document store has known corruption issues.
struct ProjectDocument: FileDocument {
    var model: ProjectModel

    init(model: ProjectModel = .empty) {
        self.model = model
    }

    static var readableContentTypes: [UTType] { [.dissonantProject] }

    init(configuration: ReadConfiguration) throws {
        guard let data = configuration.file.regularFileContents else {
            throw CocoaError(.fileReadCorruptFile)
        }
        self.model = try JSONDecoder().decode(ProjectModel.self, from: data)
    }

    func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper {
        let data = try JSONEncoder().encode(model)
        return FileWrapper(regularFileWithContents: data)
    }
}
