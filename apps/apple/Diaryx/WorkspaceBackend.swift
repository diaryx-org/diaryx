import Foundation

struct WorkspaceEntrySummary: Hashable {
    let id: String
    let path: String
    let title: String?
}

struct MetadataFieldItem: Identifiable {
    let id: String
    let key: String
    let value: String
    let values: [String]

    var isArray: Bool { !values.isEmpty }
    var displayValue: String { isArray ? values.joined(separator: ", ") : value }
}

struct WorkspaceEntryData {
    let id: String
    let path: String
    let markdown: String
    let body: String
    let metadata: [MetadataFieldItem]
}

enum WorkspaceBackendError: LocalizedError {
    case workspaceNotFound
    case workspaceNotDirectory
    case invalidEntryPath
    case rustBackendUnavailable
    case io(String)

    var errorDescription: String? {
        switch self {
        case .workspaceNotFound:
            return "Workspace does not exist."
        case .workspaceNotDirectory:
            return "Selected path is not a directory."
        case .invalidEntryPath:
            return "Invalid entry path."
        case .rustBackendUnavailable:
            return "Rust backend is not configured yet."
        case let .io(message):
            return message
        }
    }
}

/// A node in the workspace file tree.
final class SidebarTreeNode: Identifiable, ObservableObject {
    let id: String
    let name: String
    let path: String
    let isFolder: Bool
    let children: [SidebarTreeNode]

    init(name: String, path: String, isFolder: Bool, children: [SidebarTreeNode] = []) {
        self.id = path.isEmpty ? name : path
        self.name = name
        self.path = path
        self.isFolder = isFolder
        self.children = children
    }

    /// Display name: use title (name) but strip .md extension for leaf files.
    var displayName: String {
        if isFolder { return name }
        if name.hasSuffix(".md") {
            return String(name.dropLast(3))
        }
        return name
    }
}

protocol WorkspaceBackend {
    var workspaceRoot: URL { get }
    func listEntries() throws -> [WorkspaceEntrySummary]
    func getEntry(id: String) throws -> WorkspaceEntryData
    func saveEntry(id: String, markdown: String) throws
    func saveEntryBody(id: String, body: String) throws
    func createEntry(path: String, markdown: String) throws
    func createFolder(path: String) throws
    func buildFileTree() throws -> SidebarTreeNode

    // Hierarchy manipulation
    func createChildEntry(parentPath: String, title: String?) throws -> CreateChildResultData
    func moveEntry(fromPath: String, toPath: String) throws
    func attachAndMoveEntryToParent(entryPath: String, parentPath: String) throws -> String
    func convertToIndex(path: String) throws -> String
    func convertToLeaf(path: String) throws -> String
    func setFrontmatterProperty(path: String, key: String, value: FrontmatterValue) throws
    func removeFrontmatterProperty(path: String, key: String) throws
    func renameEntry(path: String, newFilename: String) throws -> String
    func deleteEntry(path: String) throws
}

protocol WorkspaceBackendFactory {
    func openWorkspace(at url: URL) throws -> any WorkspaceBackend
    func createWorkspace(at url: URL) throws -> any WorkspaceBackend
}

enum AppBackends {
    enum Mode: String {
        case local
        case rust
    }

    static func configuredMode() -> Mode {
        let raw = ProcessInfo.processInfo.environment["DIARYX_APPLE_BACKEND"]?.lowercased()
        return Mode(rawValue: raw ?? "rust") ?? .rust
    }

    static func makeDefaultFactory() -> any WorkspaceBackendFactory {
        switch configuredMode() {
        case .local:
            return LocalWorkspaceBackendFactory()
        case .rust:
            return RustWorkspaceBackendFactory()
        }
    }
}

struct LocalWorkspaceBackendFactory: WorkspaceBackendFactory {
    func openWorkspace(at url: URL) throws -> any WorkspaceBackend {
        try LocalWorkspaceBackend(workspaceRoot: url)
    }

    func createWorkspace(at url: URL) throws -> any WorkspaceBackend {
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return try LocalWorkspaceBackend(workspaceRoot: url)
    }
}

struct RustWorkspaceBackendFactory: WorkspaceBackendFactory {
    private func wrapRustError(_ error: Error) -> WorkspaceBackendError {
        WorkspaceBackendError.io("Rust backend: \(String(describing: error))")
    }

    func openWorkspace(at url: URL) throws -> any WorkspaceBackend {
        do {
            let workspace = try DiaryxAppleWorkspace(workspacePath: url.path)
            return RustWorkspaceBackend(inner: workspace)
        } catch {
            throw wrapRustError(error)
        }
    }

    func createWorkspace(at url: URL) throws -> any WorkspaceBackend {
        do {
            let workspace = try Diaryx.createWorkspace(workspacePath: url.path)
            return RustWorkspaceBackend(inner: workspace)
        } catch {
            throw wrapRustError(error)
        }
    }
}

final class RustWorkspaceBackend: WorkspaceBackend {
    let workspaceRoot: URL
    private let inner: DiaryxAppleWorkspace

    init(inner: DiaryxAppleWorkspace) {
        self.inner = inner
        self.workspaceRoot = URL(fileURLWithPath: inner.workspaceRoot())
    }

    private func wrapRustError(_ error: Error) -> WorkspaceBackendError {
        WorkspaceBackendError.io("Rust backend: \(String(describing: error))")
    }

    func listEntries() throws -> [WorkspaceEntrySummary] {
        do {
            return try inner.listEntries().map {
                WorkspaceEntrySummary(id: $0.id, path: $0.path, title: $0.title)
            }
        } catch {
            throw wrapRustError(error)
        }
    }

    func getEntry(id: String) throws -> WorkspaceEntryData {
        do {
            let d = try inner.getEntry(id: id)
            let metadata = d.metadata.map {
                MetadataFieldItem(id: $0.key, key: $0.key, value: $0.value, values: $0.values)
            }
            return WorkspaceEntryData(
                id: d.id, path: d.path, markdown: d.markdown,
                body: d.body, metadata: metadata
            )
        } catch {
            throw wrapRustError(error)
        }
    }

    func saveEntry(id: String, markdown: String) throws {
        do {
            try inner.saveEntry(id: id, markdown: markdown)
        } catch {
            throw wrapRustError(error)
        }
    }

    func saveEntryBody(id: String, body: String) throws {
        do {
            try inner.saveEntryBody(id: id, body: body)
        } catch {
            throw wrapRustError(error)
        }
    }

    func createEntry(path: String, markdown: String) throws {
        do {
            try inner.createEntry(path: path, markdown: markdown)
        } catch {
            throw wrapRustError(error)
        }
    }

    func createFolder(path: String) throws {
        do {
            try inner.createFolder(path: path)
        } catch {
            throw wrapRustError(error)
        }
    }

    func buildFileTree() throws -> SidebarTreeNode {
        do {
            let tree = try inner.buildFileTree()
            return Self.convertTreeNode(tree)
        } catch {
            throw wrapRustError(error)
        }
    }

    func createChildEntry(parentPath: String, title: String?) throws -> CreateChildResultData {
        do {
            return try inner.createChildEntry(parentPath: parentPath, title: title)
        } catch {
            throw wrapRustError(error)
        }
    }

    func moveEntry(fromPath: String, toPath: String) throws {
        do {
            try inner.moveEntry(fromPath: fromPath, toPath: toPath)
        } catch {
            throw wrapRustError(error)
        }
    }

    func attachAndMoveEntryToParent(entryPath: String, parentPath: String) throws -> String {
        do {
            return try inner.attachAndMoveEntryToParent(entryPath: entryPath, parentPath: parentPath)
        } catch {
            throw wrapRustError(error)
        }
    }

    func convertToIndex(path: String) throws -> String {
        do {
            return try inner.convertToIndex(path: path)
        } catch {
            throw wrapRustError(error)
        }
    }

    func convertToLeaf(path: String) throws -> String {
        do {
            return try inner.convertToLeaf(path: path)
        } catch {
            throw wrapRustError(error)
        }
    }

    func setFrontmatterProperty(path: String, key: String, value: FrontmatterValue) throws {
        do {
            try inner.setFrontmatterProperty(path: path, key: key, value: value)
        } catch {
            throw wrapRustError(error)
        }
    }

    func removeFrontmatterProperty(path: String, key: String) throws {
        do {
            try inner.removeFrontmatterProperty(path: path, key: key)
        } catch {
            throw wrapRustError(error)
        }
    }

    func renameEntry(path: String, newFilename: String) throws -> String {
        do {
            return try inner.renameEntry(path: path, newFilename: newFilename)
        } catch {
            throw wrapRustError(error)
        }
    }

    func deleteEntry(path: String) throws {
        do {
            try inner.deleteEntry(path: path)
        } catch {
            throw wrapRustError(error)
        }
    }

    // MARK: - Extended APIs (Phase 4)

    func searchWorkspace(query: String) throws -> SearchResultsData {
        do {
            return try inner.searchWorkspace(query: query)
        } catch {
            throw wrapRustError(error)
        }
    }

    func getWorkspaceConfig() throws -> WorkspaceConfigData {
        do {
            return try inner.getWorkspaceConfig()
        } catch {
            throw wrapRustError(error)
        }
    }

    func setWorkspaceConfigField(field: String, value: String) throws {
        do {
            try inner.setWorkspaceConfigField(field: field, value: value)
        } catch {
            throw wrapRustError(error)
        }
    }

    func getOrCreateDailyEntry(dateString: String?) throws -> String {
        do {
            return try inner.getOrCreateDailyEntry(dateString: dateString)
        } catch {
            throw wrapRustError(error)
        }
    }

    func duplicateEntry(path: String) throws -> String {
        do {
            return try inner.duplicateEntry(path: path)
        } catch {
            throw wrapRustError(error)
        }
    }

    private static func convertTreeNode(_ node: TreeNodeData) -> SidebarTreeNode {
        SidebarTreeNode(
            name: node.name,
            path: node.path,
            isFolder: node.isFolder,
            children: node.children.map { convertTreeNode($0) }
        )
    }
}

final class LocalWorkspaceBackend: WorkspaceBackend {
    let workspaceRoot: URL
    private let fileManager: FileManager

    init(workspaceRoot: URL, fileManager: FileManager = .default) throws {
        self.workspaceRoot = workspaceRoot
        self.fileManager = fileManager

        var isDir: ObjCBool = false
        let exists = fileManager.fileExists(atPath: workspaceRoot.path, isDirectory: &isDir)
        guard exists else { throw WorkspaceBackendError.workspaceNotFound }
        guard isDir.boolValue else { throw WorkspaceBackendError.workspaceNotDirectory }
    }

    func listEntries() throws -> [WorkspaceEntrySummary] {
        let keys: [URLResourceKey] = [.isRegularFileKey, .nameKey]
        let enumerator = fileManager.enumerator(
            at: workspaceRoot,
            includingPropertiesForKeys: keys,
            options: [.skipsHiddenFiles]
        )

        var entries: [WorkspaceEntrySummary] = []
        while let url = enumerator?.nextObject() as? URL {
            let values = try url.resourceValues(forKeys: Set(keys))
            guard values.isRegularFile == true else { continue }
            guard url.pathExtension.lowercased() == "md" else { continue }

            let rel = relativePath(for: url)
            let title = try? titleFromFrontmatter(at: url)
            entries.append(
                WorkspaceEntrySummary(
                    id: rel,
                    path: rel,
                    title: title
                )
            )
        }

        entries.sort { $0.path.localizedStandardCompare($1.path) == .orderedAscending }
        return entries
    }

    func getEntry(id: String) throws -> WorkspaceEntryData {
        let rel = try normalizedRelativePath(id)
        let fileURL = workspaceRoot.appendingPathComponent(rel)
        do {
            let markdown = try String(contentsOf: fileURL, encoding: .utf8)
            let (body, metadata) = parseFrontmatter(from: markdown)
            return WorkspaceEntryData(
                id: rel, path: rel, markdown: markdown,
                body: body, metadata: metadata
            )
        } catch {
            throw WorkspaceBackendError.io("Failed to read entry: \(error.localizedDescription)")
        }
    }

    func saveEntry(id: String, markdown: String) throws {
        let rel = try normalizedRelativePath(id)
        let fileURL = workspaceRoot.appendingPathComponent(rel)
        do {
            try markdown.write(to: fileURL, atomically: true, encoding: .utf8)
        } catch {
            throw WorkspaceBackendError.io("Failed to save entry: \(error.localizedDescription)")
        }
    }

    func saveEntryBody(id: String, body: String) throws {
        let rel = try normalizedRelativePath(id)
        let fileURL = workspaceRoot.appendingPathComponent(rel)
        do {
            let existing = try String(contentsOf: fileURL, encoding: .utf8)
            let content: String
            if let block = extractFrontmatterBlock(from: existing) {
                content = block + body
            } else {
                content = body
            }
            try content.write(to: fileURL, atomically: true, encoding: .utf8)
        } catch {
            throw WorkspaceBackendError.io("Failed to save entry: \(error.localizedDescription)")
        }
    }

    func createEntry(path: String, markdown: String) throws {
        let rel = try normalizedRelativePath(path)
        let fileURL = workspaceRoot.appendingPathComponent(rel)

        guard !fileManager.fileExists(atPath: fileURL.path) else {
            throw WorkspaceBackendError.io("Entry already exists: \(path)")
        }

        // Create parent directories if needed
        let parentDir = fileURL.deletingLastPathComponent()
        if !fileManager.fileExists(atPath: parentDir.path) {
            try fileManager.createDirectory(at: parentDir, withIntermediateDirectories: true)
        }

        do {
            try markdown.write(to: fileURL, atomically: true, encoding: .utf8)
        } catch {
            throw WorkspaceBackendError.io("Failed to create entry: \(error.localizedDescription)")
        }
    }

    func createFolder(path: String) throws {
        let rel = try normalizedRelativePath(path)
        let folderURL = workspaceRoot.appendingPathComponent(rel)
        do {
            try fileManager.createDirectory(at: folderURL, withIntermediateDirectories: true)
        } catch {
            throw WorkspaceBackendError.io("Failed to create folder: \(error.localizedDescription)")
        }
    }

    func buildFileTree() throws -> SidebarTreeNode {
        try buildTreeNode(at: workspaceRoot, relativeTo: workspaceRoot)
    }

    // MARK: - Hierarchy Manipulation (unsupported in local backend)

    func createChildEntry(parentPath: String, title: String?) throws -> CreateChildResultData {
        throw WorkspaceBackendError.rustBackendUnavailable
    }

    func moveEntry(fromPath: String, toPath: String) throws {
        throw WorkspaceBackendError.rustBackendUnavailable
    }

    func attachAndMoveEntryToParent(entryPath: String, parentPath: String) throws -> String {
        throw WorkspaceBackendError.rustBackendUnavailable
    }

    func convertToIndex(path: String) throws -> String {
        throw WorkspaceBackendError.rustBackendUnavailable
    }

    func convertToLeaf(path: String) throws -> String {
        throw WorkspaceBackendError.rustBackendUnavailable
    }

    func setFrontmatterProperty(path: String, key: String, value: FrontmatterValue) throws {
        throw WorkspaceBackendError.rustBackendUnavailable
    }

    func removeFrontmatterProperty(path: String, key: String) throws {
        throw WorkspaceBackendError.rustBackendUnavailable
    }

    func renameEntry(path: String, newFilename: String) throws -> String {
        throw WorkspaceBackendError.rustBackendUnavailable
    }

    func deleteEntry(path: String) throws {
        throw WorkspaceBackendError.rustBackendUnavailable
    }

    private func buildTreeNode(at url: URL, relativeTo root: URL) -> SidebarTreeNode {
        let name: String
        let relPath: String
        if url == root {
            name = url.lastPathComponent
            relPath = ""
        } else {
            name = titleFromFile(at: url) ?? url.lastPathComponent
            let rootPath = root.standardizedFileURL.path
            let fullPath = url.standardizedFileURL.path
            if fullPath.hasPrefix(rootPath) {
                let start = fullPath.index(fullPath.startIndex, offsetBy: rootPath.count)
                relPath = String(fullPath[start...])
                    .trimmingCharacters(in: CharacterSet(charactersIn: "/"))
                    .replacingOccurrences(of: "\\", with: "/")
            } else {
                relPath = url.lastPathComponent
            }
        }

        var isDir: ObjCBool = false
        guard fileManager.fileExists(atPath: url.path, isDirectory: &isDir) else {
            return SidebarTreeNode(name: name, path: relPath, isFolder: false)
        }

        guard isDir.boolValue else {
            return SidebarTreeNode(name: name, path: relPath, isFolder: false)
        }

        // Directory: enumerate children
        let keys: [URLResourceKey] = [.isRegularFileKey, .isDirectoryKey, .nameKey]
        let contents = (try? fileManager.contentsOfDirectory(
            at: url, includingPropertiesForKeys: keys,
            options: [.skipsHiddenFiles]
        )) ?? []

        var folders: [SidebarTreeNode] = []
        var files: [SidebarTreeNode] = []

        for child in contents {
            var childIsDir: ObjCBool = false
            fileManager.fileExists(atPath: child.path, isDirectory: &childIsDir)

            if childIsDir.boolValue {
                let childNode = buildTreeNode(at: child, relativeTo: root)
                // Only include folders that contain .md files (directly or nested)
                if childNode.children.contains(where: { !$0.isFolder }) || childNode.children.contains(where: { $0.isFolder }) {
                    folders.append(childNode)
                }
            } else if child.pathExtension.lowercased() == "md" {
                let childName = titleFromFile(at: child) ?? child.lastPathComponent
                let childRelPath = relativePath(for: child)
                files.append(SidebarTreeNode(name: childName, path: childRelPath, isFolder: false))
            }
        }

        // Sort: folders first (alphabetical), then files (alphabetical)
        folders.sort { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
        files.sort { $0.name.localizedStandardCompare($1.name) == .orderedAscending }

        return SidebarTreeNode(
            name: name, path: relPath, isFolder: true,
            children: folders + files
        )
    }

    /// Try to get a title from a file's frontmatter, or nil.
    private func titleFromFile(at url: URL) -> String? {
        guard url.pathExtension.lowercased() == "md" else { return nil }
        return try? titleFromFrontmatter(at: url)
    }

    // MARK: - Frontmatter Parsing

    /// Parse frontmatter from markdown content, returning the body and metadata fields.
    private func parseFrontmatter(from content: String) -> (String, [MetadataFieldItem]) {
        guard content.hasPrefix("---\n") || content.hasPrefix("---\r\n") else {
            return (content, [])
        }

        let rest = String(content.dropFirst(4))
        guard let endRange = rest.range(of: "\n---\n") ?? rest.range(of: "\n---\r\n") else {
            return (content, [])
        }

        let yamlBlock = String(rest[..<endRange.lowerBound])
        let body = String(rest[endRange.upperBound...])
        var metadata: [MetadataFieldItem] = []

        for rawLine in yamlBlock.split(whereSeparator: \.isNewline) {
            let line = rawLine.trimmingCharacters(in: .whitespaces)
            guard let colonIndex = line.firstIndex(of: ":") else { continue }

            // Skip lines that start with "- " (array items handled by parent key)
            guard !line.hasPrefix("- ") else { continue }

            let key = String(line[..<colonIndex]).trimmingCharacters(in: .whitespaces)
            let rawValue = String(line[line.index(after: colonIndex)...])
                .trimmingCharacters(in: .whitespaces)

            // Check if this key has array values (following lines starting with "- ")
            let arrayValues = extractArrayValues(for: key, in: yamlBlock)
            if !arrayValues.isEmpty {
                metadata.append(MetadataFieldItem(
                    id: key, key: key, value: "", values: arrayValues
                ))
            } else {
                let value = rawValue.trimmingCharacters(in: CharacterSet(charactersIn: "\"'"))
                metadata.append(MetadataFieldItem(
                    id: key, key: key, value: value, values: []
                ))
            }
        }

        return (body, metadata)
    }

    /// Extract array values for a key from YAML frontmatter.
    private func extractArrayValues(for key: String, in yaml: String) -> [String] {
        let lines = yaml.split(whereSeparator: \.isNewline).map(String.init)
        guard let keyIndex = lines.firstIndex(where: {
            $0.trimmingCharacters(in: .whitespaces).hasPrefix("\(key):")
        }) else { return [] }

        // Check if the value after the colon is empty (indicating an array follows)
        let keyLine = lines[keyIndex].trimmingCharacters(in: .whitespaces)
        let afterColon = String(keyLine.dropFirst(key.count + 1)).trimmingCharacters(in: .whitespaces)
        guard afterColon.isEmpty else { return [] }

        var values: [String] = []
        for i in (keyIndex + 1)..<lines.count {
            let line = lines[i].trimmingCharacters(in: .whitespaces)
            guard line.hasPrefix("- ") else { break }
            let item = String(line.dropFirst(2)).trimmingCharacters(in: .whitespaces)
            values.append(item.trimmingCharacters(in: CharacterSet(charactersIn: "\"'")))
        }
        return values
    }

    /// Extract the raw frontmatter block (including delimiters) from content.
    /// Returns the block with trailing newline, ready to prepend to body.
    private func extractFrontmatterBlock(from content: String) -> String? {
        guard content.hasPrefix("---\n") || content.hasPrefix("---\r\n") else {
            return nil
        }

        let rest = String(content.dropFirst(4))
        guard let endRange = rest.range(of: "\n---\n") ?? rest.range(of: "\n---\r\n") else {
            return nil
        }

        let endOffset = content.distance(
            from: content.startIndex,
            to: content.index(content.startIndex, offsetBy: 4 + rest.distance(from: rest.startIndex, to: endRange.upperBound))
        )
        return String(content.prefix(endOffset))
    }

    // MARK: - Path Helpers

    private func normalizedRelativePath(_ path: String) throws -> String {
        let trimmed = path.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { throw WorkspaceBackendError.invalidEntryPath }
        guard !trimmed.hasPrefix("/") else { throw WorkspaceBackendError.invalidEntryPath }

        let nsPath = trimmed as NSString
        let standardized = nsPath.standardizingPath.replacingOccurrences(of: "\\", with: "/")

        let components = standardized.split(separator: "/")
        if components.contains(where: { $0 == ".." || $0 == "." || $0.isEmpty }) {
            throw WorkspaceBackendError.invalidEntryPath
        }

        return components.joined(separator: "/")
    }

    private func relativePath(for url: URL) -> String {
        let root = workspaceRoot.standardizedFileURL.path
        let full = url.standardizedFileURL.path
        if full.hasPrefix(root) {
            let start = full.index(full.startIndex, offsetBy: root.count)
            let suffix = String(full[start...]).trimmingCharacters(in: CharacterSet(charactersIn: "/"))
            return suffix.replacingOccurrences(of: "\\", with: "/")
        }
        return url.lastPathComponent
    }

    private func titleFromFrontmatter(at fileURL: URL) throws -> String? {
        let content = try String(contentsOf: fileURL, encoding: .utf8)
        guard content.hasPrefix("---\n") || content.hasPrefix("---\r\n") else {
            return nil
        }

        let rest = String(content.dropFirst(4))
        guard let endRange = rest.range(of: "\n---\n") ?? rest.range(of: "\n---\r\n") else {
            return nil
        }

        let frontmatter = String(rest[..<endRange.lowerBound])
        for rawLine in frontmatter.split(whereSeparator: \.isNewline) {
            let line = rawLine.trimmingCharacters(in: .whitespaces)
            guard line.hasPrefix("title:") else { continue }
            let value = line.dropFirst("title:".count).trimmingCharacters(in: .whitespaces)
            return value.trimmingCharacters(in: CharacterSet(charactersIn: "\"'"))
        }

        return nil
    }
}
