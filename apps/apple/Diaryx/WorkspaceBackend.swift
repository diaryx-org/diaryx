import Foundation

struct WorkspaceEntrySummary: Hashable {
    let id: String
    let path: String
    let title: String?
}

struct WorkspaceEntryData {
    let id: String
    let path: String
    let markdown: String
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
    func openWorkspace(at _: URL) throws -> any WorkspaceBackend {
        throw WorkspaceBackendError.rustBackendUnavailable
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
            return WorkspaceEntryData(id: rel, path: rel, markdown: markdown)
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
