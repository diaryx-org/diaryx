import SwiftUI

@Observable
final class WorkspaceState {
    let registryEntry: WorkspaceRegistryEntry
    private(set) var backend: (any WorkspaceBackend)?

    var fileTree: SidebarTreeNode?
    var selectedPath: String?
    var loadedMarkdown: String = ""
    var editorContent: String = ""
    var isDirty: Bool = false
    var lastError: String?
    var currentMetadata: [MetadataFieldItem] = []
    var showInspector: Bool = true
    var expandedFolders: Set<String> = []

    // Sheet state
    var showNewEntrySheet: Bool = false
    var newEntryName: String = ""
    var nodeToRename: SidebarTreeNode?
    var renameText: String = ""
    var nodeToDelete: SidebarTreeNode?
    var nodeForNewChild: SidebarTreeNode?
    var newChildTitle: String = ""

    private var autoSaveTask: Task<Void, Never>?

    init(registryEntry: WorkspaceRegistryEntry, backend: (any WorkspaceBackend)?) {
        self.registryEntry = registryEntry
        self.backend = backend
    }

    // MARK: - Tree Actions

    var treeActions: TreeNodeActions {
        TreeNodeActions(
            addChild: { [weak self] node in
                self?.newChildTitle = ""
                self?.nodeForNewChild = node
            },
            rename: { [weak self] node in
                self?.renameText = node.name
                self?.nodeToRename = node
            },
            delete: { [weak self] node in
                self?.nodeToDelete = node
            },
            moveToParent: { [weak self] draggedPath, target in
                self?.moveNodeToParent(draggedPath: draggedPath, target: target)
            }
        )
    }

    func addChildToNode(_ node: SidebarTreeNode) {
        nodeForNewChild = nil
        guard let backend else { return }
        let trimmed = newChildTitle.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }

        do {
            let result = try backend.createChildEntry(parentPath: node.path, title: trimmed)
            refreshTree()
            expandParentFolders(for: result.childPath)
            expandedFolders.insert(result.parentPath)
            selectedPath = result.childPath
        } catch {
            report(error)
        }
    }

    func renameNode(_ node: SidebarTreeNode, newName: String) {
        nodeToRename = nil
        guard let backend else { return }
        let trimmed = newName.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }

        do {
            let newPath = try backend.renameEntry(path: node.path, newFilename: trimmed)
            let wasSelected = selectedPath == node.path
            refreshTree()
            if wasSelected {
                selectedPath = newPath
            }
        } catch {
            report(error)
        }
    }

    func deleteNode(_ node: SidebarTreeNode) {
        nodeToDelete = nil
        guard let backend else { return }

        do {
            let wasSelected = selectedPath == node.path
            try backend.deleteEntry(path: node.path)
            refreshTree()
            if wasSelected {
                selectedPath = nil
            }
        } catch {
            report(error)
        }
    }

    func moveNodeToParent(draggedPath: String, target: SidebarTreeNode) {
        guard let backend else { return }
        do {
            let newPath = try backend.attachAndMoveEntryToParent(
                entryPath: draggedPath, parentPath: target.path
            )
            refreshTree()
            expandedFolders.insert(target.path)
            if selectedPath == draggedPath {
                selectedPath = newPath
            }
        } catch {
            report(error)
        }
    }

    // MARK: - File Operations

    func refreshTree() {
        guard let backend else {
            fileTree = nil
            return
        }

        do {
            let tree = try backend.buildFileTree()
            fileTree = tree
            if !tree.path.isEmpty {
                expandedFolders.insert(tree.id)
            }
            if let path = selectedPath, !treeContainsPath(path) {
                selectedPath = nil
                loadedMarkdown = ""
                editorContent = ""
                isDirty = false
                currentMetadata = []
            }
        } catch {
            fileTree = nil
            report(error)
        }
    }

    func loadEntry(id: String) {
        guard let backend else { return }

        do {
            let entry = try backend.getEntry(id: id)
            loadedMarkdown = entry.body
            editorContent = entry.body
            currentMetadata = entry.metadata
            isDirty = false
        } catch {
            report(error)
            loadedMarkdown = "Error loading file: \(error.localizedDescription)"
            editorContent = ""
            currentMetadata = []
        }
    }

    func scheduleSave() {
        autoSaveTask?.cancel()
        let pathToSave = selectedPath
        let contentToSave = editorContent
        autoSaveTask = Task { @MainActor [weak self] in
            try? await Task.sleep(for: .seconds(1))
            guard !Task.isCancelled else { return }
            self?.saveFile(path: pathToSave, content: contentToSave)
        }
    }

    func saveFile(path: String?, content: String) {
        guard let path, let backend, isDirty else { return }
        do {
            try backend.saveEntryBody(id: path, body: content)
            isDirty = false
        } catch {
            report(error)
        }
    }

    func saveCurrentIfDirty() {
        if isDirty, let path = selectedPath {
            autoSaveTask?.cancel()
            saveFile(path: path, content: editorContent)
        }
    }

    func createNewEntry(name: String) {
        showNewEntrySheet = false
        guard let backend else { return }

        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }

        let path = trimmed.hasSuffix(".md") ? trimmed : "\(trimmed).md"
        do {
            try backend.createEntry(path: path, markdown: "")
            refreshTree()
            expandParentFolders(for: path)
            selectedPath = path
        } catch {
            report(error)
        }
    }

    func handleLinkClick(_ href: String) {
        if href.hasPrefix("http://") || href.hasPrefix("https://") {
            if let url = URL(string: href) {
                #if os(iOS)
                UIApplication.shared.open(url)
                #else
                NSWorkspace.shared.open(url)
                #endif
            }
            return
        }

        var targetPath = href
        if let withoutFragment = targetPath.split(separator: "#", maxSplits: 1).first {
            targetPath = String(withoutFragment)
        }
        if let withoutQuery = targetPath.split(separator: "?", maxSplits: 1).first {
            targetPath = String(withoutQuery)
        }

        if treeContainsPath(targetPath) {
            selectedPath = targetPath
            return
        }

        if !targetPath.hasSuffix(".md"), treeContainsPath("\(targetPath).md") {
            selectedPath = "\(targetPath).md"
        }
    }

    func onSelectedPathChanged(oldPath: String?, newPath: String?) {
        if isDirty, let old = oldPath {
            autoSaveTask?.cancel()
            saveFile(path: old, content: editorContent)
        }
        if let path = newPath {
            loadEntry(id: path)
        } else {
            currentMetadata = []
        }
    }

    // MARK: - Helpers

    var selectedDisplayName: String {
        guard let path = selectedPath else { return registryEntry.name }
        return (path as NSString).deletingPathExtension
    }

    func treeContainsPath(_ path: String) -> Bool {
        guard let tree = fileTree else { return false }
        return findNode(path: path, in: tree) != nil
    }

    func findNodeByPath(_ path: String) -> SidebarTreeNode? {
        guard let tree = fileTree else { return nil }
        return findNode(path: path, in: tree)
    }

    private func findNode(path: String, in node: SidebarTreeNode) -> SidebarTreeNode? {
        if node.path == path { return node }
        for child in node.children {
            if let found = findNode(path: path, in: child) { return found }
        }
        return nil
    }

    private func expandParentFolders(for path: String) {
        let components = path.split(separator: "/").dropLast()
        var current = ""
        for component in components {
            if current.isEmpty {
                current = String(component)
            } else {
                current += "/\(component)"
            }
            expandedFolders.insert(current)
        }
    }

    private func report(_ error: Error) {
        print("[WorkspaceState] \(error)")
        lastError = error.localizedDescription
    }
}
