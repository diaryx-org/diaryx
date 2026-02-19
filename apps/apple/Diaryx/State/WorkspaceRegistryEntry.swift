import Foundation

struct WorkspaceRegistryEntry: Codable, Identifiable, Hashable {
    let id: UUID
    var name: String
    var path: String
    var storageType: StorageType
    var lastOpenedAt: Date
    var createdAt: Date
    /// Security-scoped bookmark data for macOS sandbox access across launches.
    var bookmarkData: Data?

    enum StorageType: String, Codable {
        /// User-selected folder (macOS folder picker).
        case folder
        /// App-managed documents directory (iOS or default).
        case appDocuments
    }

    init(name: String, path: String, storageType: StorageType, bookmarkData: Data? = nil) {
        self.id = UUID()
        self.name = name
        self.path = path
        self.storageType = storageType
        self.lastOpenedAt = Date()
        self.createdAt = Date()
        self.bookmarkData = bookmarkData
    }
}
