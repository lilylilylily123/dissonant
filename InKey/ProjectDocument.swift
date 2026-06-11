import SwiftUI
import UniformTypeIdentifiers
import InKeyCore

extension UTType {
    /// The "in key" project document type, declared in Info.plist (UTExportedTypeDeclarations).
    static let inKeyProject = UTType(exportedAs: "com.crackerjack.inkey.project")
}

/// Wraps `ProjectModel` (from InKeyCore) as a SwiftUI document. Persistence is plain JSON
/// via Codable — deliberately not SwiftData, whose document store has known corruption issues.
struct ProjectDocument: FileDocument {
    var model: ProjectModel

    init(model: ProjectModel = .empty) {
        self.model = model
    }

    static var readableContentTypes: [UTType] { [.inKeyProject] }

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
