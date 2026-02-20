import SwiftUI

enum ActiveView {
    case welcome
    case workspace(WorkspaceState)
}

@Observable @MainActor
final class AppState {
    private let backendFactory: any WorkspaceBackendFactory = AppBackends.makeDefaultFactory()

    var activeView: ActiveView = .welcome
    var workspaceRegistry: [WorkspaceRegistryEntry] = []

    private let registryKey = "diaryx_workspace_registry"

    init() {
        loadRegistry()
    }

    var currentWorkspace: WorkspaceState? {
        if case .workspace(let ws) = activeView { return ws }
        return nil
    }

    // MARK: - Registry Persistence

    private func loadRegistry() {
        guard let data = UserDefaults.standard.data(forKey: registryKey) else { return }
        do {
            workspaceRegistry = try JSONDecoder().decode([WorkspaceRegistryEntry].self, from: data)
            workspaceRegistry.sort { $0.lastOpenedAt > $1.lastOpenedAt }
        } catch {
            print("[AppState] Failed to load workspace registry: \(error)")
        }
    }

    private func saveRegistry() {
        do {
            let data = try JSONEncoder().encode(workspaceRegistry)
            UserDefaults.standard.set(data, forKey: registryKey)
        } catch {
            print("[AppState] Failed to save workspace registry: \(error)")
        }
    }

    // MARK: - Workspace Management

    func openWorkspace(entry: WorkspaceRegistryEntry) {
        let url = resolveURL(for: entry)
        guard let url else {
            print("[AppState] Could not resolve URL for workspace: \(entry.name)")
            return
        }

        do {
            let backend = try backendFactory.openWorkspace(at: url)
            let ws = WorkspaceState(registryEntry: entry, backend: backend)
            ws.refreshTree()

            // Update lastOpenedAt
            if let idx = workspaceRegistry.firstIndex(where: { $0.id == entry.id }) {
                workspaceRegistry[idx].lastOpenedAt = Date()
                saveRegistry()
            }

            activeView = .workspace(ws)
        } catch {
            print("[AppState] Failed to open workspace: \(error)")
        }
    }

    func registerAndOpenWorkspace(name: String, url: URL, storageType: WorkspaceRegistryEntry.StorageType) {
        var bookmarkData: Data?
        #if os(macOS)
        if storageType == .folder {
            _ = url.startAccessingSecurityScopedResource()
            bookmarkData = try? url.bookmarkData(
                options: .withSecurityScope,
                includingResourceValuesForKeys: nil,
                relativeTo: nil
            )
        }
        #endif

        let entry = WorkspaceRegistryEntry(
            name: name,
            path: url.path,
            storageType: storageType,
            bookmarkData: bookmarkData
        )

        workspaceRegistry.insert(entry, at: 0)
        saveRegistry()

        do {
            let backend = try backendFactory.openWorkspace(at: url)
            let ws = WorkspaceState(registryEntry: entry, backend: backend)
            ws.refreshTree()
            activeView = .workspace(ws)
        } catch {
            print("[AppState] Failed to open newly registered workspace: \(error)")
        }
    }

    func createAndOpenWorkspace(name: String, url: URL, storageType: WorkspaceRegistryEntry.StorageType) {
        do {
            let backend = try backendFactory.createWorkspace(at: url)

            var bookmarkData: Data?
            #if os(macOS)
            if storageType == .folder {
                _ = url.startAccessingSecurityScopedResource()
                bookmarkData = try? url.bookmarkData(
                    options: .withSecurityScope,
                    includingResourceValuesForKeys: nil,
                    relativeTo: nil
                )
            }
            #endif

            let entry = WorkspaceRegistryEntry(
                name: name,
                path: url.path,
                storageType: storageType,
                bookmarkData: bookmarkData
            )
            workspaceRegistry.insert(entry, at: 0)
            saveRegistry()

            let ws = WorkspaceState(registryEntry: entry, backend: backend)
            ws.refreshTree()
            activeView = .workspace(ws)
        } catch {
            print("[AppState] Failed to create workspace: \(error)")
        }
    }

    func removeWorkspace(id: UUID) {
        workspaceRegistry.removeAll { $0.id == id }
        saveRegistry()

        // If we just removed the active workspace, go to welcome
        if case .workspace(let ws) = activeView, ws.registryEntry.id == id {
            activeView = .welcome
        }
    }

    func renameWorkspace(id: UUID, newName: String) {
        if let idx = workspaceRegistry.firstIndex(where: { $0.id == id }) {
            workspaceRegistry[idx].name = newName
            saveRegistry()
        }
    }

    func switchToWelcome() {
        if let ws = currentWorkspace {
            ws.saveCurrentIfDirty()
        }
        activeView = .welcome
    }

    // MARK: - URL Resolution

    private func resolveURL(for entry: WorkspaceRegistryEntry) -> URL? {
        #if os(macOS)
        if let bookmarkData = entry.bookmarkData {
            var isStale = false
            if let url = try? URL(
                resolvingBookmarkData: bookmarkData,
                options: .withSecurityScope,
                relativeTo: nil,
                bookmarkDataIsStale: &isStale
            ) {
                _ = url.startAccessingSecurityScopedResource()
                if isStale {
                    // Re-create bookmark
                    if let idx = workspaceRegistry.firstIndex(where: { $0.id == entry.id }) {
                        workspaceRegistry[idx].bookmarkData = try? url.bookmarkData(
                            options: .withSecurityScope,
                            includingResourceValuesForKeys: nil,
                            relativeTo: nil
                        )
                        saveRegistry()
                    }
                }
                return url
            }
        }
        #endif

        return URL(fileURLWithPath: entry.path)
    }
}
