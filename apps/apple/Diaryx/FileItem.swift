import Foundation

struct FileItem: Identifiable, Hashable {
    let id: String
    let path: String
    let title: String?

    var name: String {
        if let title, !title.isEmpty {
            return title
        }
        return (path as NSString).deletingPathExtension
    }

    var fileName: String {
        (path as NSString).lastPathComponent
    }

    static func from(entry: WorkspaceEntrySummary) -> FileItem {
        FileItem(id: entry.id, path: entry.path, title: entry.title)
    }
}
