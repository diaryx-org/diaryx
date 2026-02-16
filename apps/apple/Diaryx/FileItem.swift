import Foundation

struct FileItem: Identifiable, Hashable {
    let id: URL
    let url: URL

    var name: String {
        url.deletingPathExtension().lastPathComponent
    }

    var fileName: String {
        url.lastPathComponent
    }
}
