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

protocol WorkspaceBackend {
    var workspaceRoot: URL { get }
    func listEntries() throws -> [WorkspaceEntrySummary]
    func getEntry(id: String) throws -> WorkspaceEntryData
    func saveEntry(id: String, markdown: String) throws
    func saveEntryBody(id: String, body: String) throws
}

protocol WorkspaceBackendFactory {
    func openWorkspace(at url: URL) throws -> any WorkspaceBackend
}

enum AppBackends {
    enum Mode: String {
        case local
        case rust
    }

    static func configuredMode() -> Mode {
        let raw = ProcessInfo.processInfo.environment["DIARYX_APPLE_BACKEND"]?.lowercased()
        return Mode(rawValue: raw ?? "local") ?? .local
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
}

struct RustWorkspaceBackendFactory: WorkspaceBackendFactory {
    func openWorkspace(at url: URL) throws -> any WorkspaceBackend {
        do {
            let workspace = try DiaryxAppleWorkspace(workspacePath: url.path)
            return RustWorkspaceBackend(inner: workspace)
        } catch {
            throw WorkspaceBackendError.io("Rust backend: \(error.localizedDescription)")
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

    func listEntries() throws -> [WorkspaceEntrySummary] {
        do {
            return try inner.listEntries().map {
                WorkspaceEntrySummary(id: $0.id, path: $0.path, title: $0.title)
            }
        } catch {
            throw WorkspaceBackendError.io("Rust backend: \(error.localizedDescription)")
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
            throw WorkspaceBackendError.io("Rust backend: \(error.localizedDescription)")
        }
    }

    func saveEntry(id: String, markdown: String) throws {
        do {
            try inner.saveEntry(id: id, markdown: markdown)
        } catch {
            throw WorkspaceBackendError.io("Rust backend: \(error.localizedDescription)")
        }
    }

    func saveEntryBody(id: String, body: String) throws {
        do {
            try inner.saveEntryBody(id: id, body: body)
        } catch {
            throw WorkspaceBackendError.io("Rust backend: \(error.localizedDescription)")
        }
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
