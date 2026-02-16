import SwiftUI

struct ContentView: View {
    private let backendFactory: any WorkspaceBackendFactory = AppBackends.makeDefaultFactory()

    @State private var backend: (any WorkspaceBackend)?
    @State private var fileTree: SidebarTreeNode?
    @State private var selectedPath: String?
    /// Content loaded from disk — only updated on file load. Drives EditorWebView's initialMarkdown.
    @State private var loadedMarkdown: String = ""
    /// Latest content received from the editor — used for saving.
    @State private var editorContent: String = ""
    @State private var workspaceURL: URL?
    @State private var isDirty: Bool = false
    @State private var lastError: String?
    @State private var currentMetadata: [MetadataFieldItem] = []
    @State private var showInspector: Bool = true
    @State private var showNewEntrySheet: Bool = false
    @State private var newEntryName: String = ""
    @State private var expandedFolders: Set<String> = []
    @State private var autoSaveTask: Task<Void, Never>?
    @State private var nodeToRename: SidebarTreeNode?
    @State private var renameText: String = ""
    @State private var nodeToDelete: SidebarTreeNode?
    @State private var nodeForNewChild: SidebarTreeNode?
    @State private var newChildTitle: String = ""

    var body: some View {
        NavigationSplitView {
            sidebar
        } detail: {
            detail
        }
        .inspector(isPresented: $showInspector) {
            MetadataSidebar(metadata: currentMetadata)
                .inspectorColumnWidth(min: 200, ideal: 260, max: 400)
        }
        .navigationTitle(selectedDisplayName)
        .focusedSceneValue(\.saveAction, SaveAction(save: {
            saveFile(path: selectedPath, content: editorContent)
        }))
        .toolbar {
            ToolbarItem(placement: .automatic) {
                Button {
                    showInspector.toggle()
                } label: {
                    Label("Inspector", systemImage: "sidebar.trailing")
                }
                .help("Toggle metadata inspector")
            }
        }
        .alert("Error", isPresented: Binding(
            get: { lastError != nil },
            set: { if !$0 { lastError = nil } }
        )) {
            Button("OK", role: .cancel) {
                lastError = nil
            }
        } message: {
            Text(lastError ?? "Unknown error")
        }
    }

    private var selectedDisplayName: String {
        guard let path = selectedPath else { return "Diaryx" }
        return (path as NSString).deletingPathExtension
    }

    // MARK: - Sidebar

    @ViewBuilder
    private var sidebar: some View {
        VStack(spacing: 0) {
            if fileTree == nil && workspaceURL == nil {
                ContentUnavailableView {
                    Label("No Folder Open", systemImage: "folder")
                } description: {
                    Text("Open a folder of Markdown files to get started.")
                } actions: {
                    Button("Open Folder...") { pickFolder() }
                }
            } else if let tree = fileTree, tree.children.isEmpty && tree.path.isEmpty {
                ContentUnavailableView {
                    Label("No Markdown Files", systemImage: "doc.text")
                } description: {
                    Text("This folder doesn't contain any .md files.")
                }
            } else if let tree = fileTree {
                List(selection: $selectedPath) {
                    if !tree.path.isEmpty {
                        // Root index: show it as a top-level folder
                        FileTreeRow(node: tree, expandedFolders: $expandedFolders, actions: treeActions)
                    } else {
                        // Filesystem tree: root is the workspace dir, show children directly
                        ForEach(tree.children) { child in
                            FileTreeRow(node: child, expandedFolders: $expandedFolders, actions: treeActions)
                        }
                    }
                }
                .listStyle(.sidebar)
            }
        }
        .toolbar {
            ToolbarItem {
                Button {
                    pickFolder()
                } label: {
                    Label("Open Folder", systemImage: "folder.badge.plus")
                }
            }
            if backend != nil {
                ToolbarItem {
                    Button {
                        newEntryName = ""
                        showNewEntrySheet = true
                    } label: {
                        Label("New Entry", systemImage: "doc.badge.plus")
                    }
                }
            }
        }
        .sheet(isPresented: $showNewEntrySheet) {
            NewEntrySheet(
                entryName: $newEntryName,
                onCreate: { createNewEntry(name: newEntryName) },
                onCancel: { showNewEntrySheet = false }
            )
        }
        .sheet(item: $nodeToRename) { node in
            RenameSheet(
                name: $renameText,
                onRename: { renameNode(node, newName: renameText) },
                onCancel: { nodeToRename = nil }
            )
        }
        .sheet(item: $nodeForNewChild) { node in
            AddChildSheet(
                title: $newChildTitle,
                parentName: node.displayName,
                onCreate: { addChildToNode(node) },
                onCancel: { nodeForNewChild = nil }
            )
        }
        .confirmationDialog(
            "Delete \"\(nodeToDelete?.displayName ?? "")\"?",
            isPresented: Binding(
                get: { nodeToDelete != nil },
                set: { if !$0 { nodeToDelete = nil } }
            ),
            titleVisibility: .visible
        ) {
            Button("Delete", role: .destructive) {
                if let node = nodeToDelete {
                    deleteNode(node)
                }
            }
            Button("Cancel", role: .cancel) {
                nodeToDelete = nil
            }
        } message: {
            if let node = nodeToDelete, node.isFolder {
                Text("This folder and all its contents will be permanently deleted.")
            } else {
                Text("This file will be permanently deleted.")
            }
        }
        .frame(minWidth: 200)
        .onChange(of: selectedPath) { oldPath, newPath in
            // Save the OLD file before loading the new one
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
    }

    // MARK: - Detail

    @ViewBuilder
    private var detail: some View {
        if selectedPath != nil {
            EditorWebView(
                initialMarkdown: loadedMarkdown,
                onContentChanged: { markdown in
                    editorContent = markdown
                    isDirty = true
                    scheduleSave()
                },
                onLinkClicked: { href in
                    handleLinkClick(href)
                }
            )
        } else {
            ContentUnavailableView {
                Label("No File Selected", systemImage: "doc.text")
            } description: {
                Text("Select a Markdown file from the sidebar to start editing.")
            }
        }
    }

    // MARK: - Tree Actions

    private var treeActions: TreeNodeActions {
        TreeNodeActions(
            addChild: { node in
                newChildTitle = ""
                nodeForNewChild = node
            },
            rename: { node in
                renameText = node.name
                nodeToRename = node
            },
            delete: { node in
                nodeToDelete = node
            },
            moveToParent: { draggedPath, target in
                moveNodeToParent(draggedPath: draggedPath, target: target)
            }
        )
    }

    private func addChildToNode(_ node: SidebarTreeNode) {
        nodeForNewChild = nil
        guard let backend else { return }
        let trimmed = newChildTitle.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }

        do {
            let result = try backend.createChildEntry(parentPath: node.path, title: trimmed)
            refreshTree()
            expandParentFolders(for: result.childPath)
            // Also expand the parent itself
            expandedFolders.insert(result.parentPath)
            selectedPath = result.childPath
        } catch {
            report(error)
        }
    }

    private func renameNode(_ node: SidebarTreeNode, newName: String) {
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

    private func deleteNode(_ node: SidebarTreeNode) {
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

    private func moveNodeToParent(draggedPath: String, target: SidebarTreeNode) {
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

    // MARK: - Actions

    private func pickFolder() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.message = "Select a folder containing Markdown files"

        guard panel.runModal() == .OK, let url = panel.url else { return }

        // Start security-scoped access for the user-selected folder
        _ = url.startAccessingSecurityScopedResource()
        workspaceURL = url
        do {
            let openedBackend = try backendFactory.openWorkspace(at: url)
            backend = openedBackend
            refreshTree()
        } catch {
            fileTree = nil
            selectedPath = nil
            backend = nil
            currentMetadata = []
            report(error)
        }
    }

    private func refreshTree() {
        guard let backend else {
            fileTree = nil
            return
        }

        do {
            let tree = try backend.buildFileTree()
            fileTree = tree
            // Auto-expand the root node so contents are visible
            if !tree.path.isEmpty {
                expandedFolders.insert(tree.id)
            }
            // If selected file no longer exists in tree, deselect
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

    private func treeContainsPath(_ path: String) -> Bool {
        guard let tree = fileTree else { return false }
        return findNode(path: path, in: tree) != nil
    }

    private func findNode(path: String, in node: SidebarTreeNode) -> SidebarTreeNode? {
        if node.path == path { return node }
        for child in node.children {
            if let found = findNode(path: path, in: child) { return found }
        }
        return nil
    }

    private func loadEntry(id: String) {
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

    private func scheduleSave() {
        autoSaveTask?.cancel()
        let pathToSave = selectedPath
        let contentToSave = editorContent
        autoSaveTask = Task {
            try? await Task.sleep(for: .seconds(1))
            guard !Task.isCancelled else { return }
            saveFile(path: pathToSave, content: contentToSave)
        }
    }

    private func saveFile(path: String?, content: String) {
        guard let path, let backend, isDirty else { return }
        do {
            try backend.saveEntryBody(id: path, body: content)
            isDirty = false
        } catch {
            report(error)
        }
    }

    private func createNewEntry(name: String) {
        showNewEntrySheet = false
        guard let backend else { return }

        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }

        let path = trimmed.hasSuffix(".md") ? trimmed : "\(trimmed).md"
        do {
            try backend.createEntry(path: path, markdown: "")
            refreshTree()
            // Expand parent folders for the new entry
            expandParentFolders(for: path)
            selectedPath = path
        } catch {
            report(error)
        }
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

    private func handleLinkClick(_ href: String) {
        // External links: open in browser
        if href.hasPrefix("http://") || href.hasPrefix("https://") {
            if let url = URL(string: href) {
                NSWorkspace.shared.open(url)
            }
            return
        }

        // Relative markdown links: try to navigate to the file
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

    private func report(_ error: Error) {
        print("[ContentView] \(error)")
        lastError = error.localizedDescription
    }
}

private struct NewEntrySheet: View {
    @Binding var entryName: String
    let onCreate: () -> Void
    let onCancel: () -> Void

    var body: some View {
        VStack(spacing: 16) {
            Text("New Entry")
                .font(.headline)

            TextField("Filename (e.g. 2026/02/16.md)", text: $entryName)
                .textFieldStyle(.roundedBorder)
                .onSubmit {
                    guard !entryName.trimmingCharacters(in: .whitespaces).isEmpty else { return }
                    onCreate()
                }

            HStack {
                Button("Cancel", role: .cancel, action: onCancel)
                    .keyboardShortcut(.cancelAction)
                Button("Create", action: onCreate)
                    .keyboardShortcut(.defaultAction)
                    .disabled(entryName.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding()
        .frame(minWidth: 320)
    }
}

private struct RenameSheet: View {
    @Binding var name: String
    let onRename: () -> Void
    let onCancel: () -> Void

    var body: some View {
        VStack(spacing: 16) {
            Text("Rename")
                .font(.headline)

            TextField("New name", text: $name)
                .textFieldStyle(.roundedBorder)
                .onSubmit {
                    guard !name.trimmingCharacters(in: .whitespaces).isEmpty else { return }
                    onRename()
                }

            HStack {
                Button("Cancel", role: .cancel, action: onCancel)
                    .keyboardShortcut(.cancelAction)
                Button("Rename", action: onRename)
                    .keyboardShortcut(.defaultAction)
                    .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding()
        .frame(minWidth: 320)
    }
}

private struct AddChildSheet: View {
    @Binding var title: String
    let parentName: String
    let onCreate: () -> Void
    let onCancel: () -> Void

    var body: some View {
        VStack(spacing: 16) {
            Text("Add Child to \"\(parentName)\"")
                .font(.headline)

            TextField("Child title", text: $title)
                .textFieldStyle(.roundedBorder)
                .onSubmit {
                    guard !title.trimmingCharacters(in: .whitespaces).isEmpty else { return }
                    onCreate()
                }

            HStack {
                Button("Cancel", role: .cancel, action: onCancel)
                    .keyboardShortcut(.cancelAction)
                Button("Create", action: onCreate)
                    .keyboardShortcut(.defaultAction)
                    .disabled(title.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding()
        .frame(minWidth: 320)
    }
}

private struct TreeNodeActions {
    let addChild: (SidebarTreeNode) -> Void
    let rename: (SidebarTreeNode) -> Void
    let delete: (SidebarTreeNode) -> Void
    let moveToParent: (String, SidebarTreeNode) -> Void
}

private struct FileTreeRow: View {
    let node: SidebarTreeNode
    @Binding var expandedFolders: Set<String>
    let actions: TreeNodeActions

    var body: some View {
        if node.isFolder {
            DisclosureGroup(isExpanded: Binding(
                get: { expandedFolders.contains(node.id) },
                set: { isExpanded in
                    if isExpanded {
                        expandedFolders.insert(node.id)
                    } else {
                        expandedFolders.remove(node.id)
                    }
                }
            )) {
                ForEach(node.children) { child in
                    FileTreeRow(node: child, expandedFolders: $expandedFolders, actions: actions)
                }
            } label: {
                Label(node.displayName, systemImage: "folder")
                    .draggable(node.path)
                    .dropDestination(for: String.self) { paths, _ in
                        guard let draggedPath = paths.first, draggedPath != node.path else {
                            return false
                        }
                        actions.moveToParent(draggedPath, node)
                        return true
                    }
                    .contextMenu { contextMenuItems }
            }
            .tag(node.path)
        } else {
            Label(node.displayName, systemImage: "doc.text")
                .tag(node.path)
                .draggable(node.path)
                .dropDestination(for: String.self) { paths, _ in
                    guard let draggedPath = paths.first, draggedPath != node.path else {
                        return false
                    }
                    actions.moveToParent(draggedPath, node)
                    return true
                }
                .contextMenu { contextMenuItems }
        }
    }

    @ViewBuilder
    private var contextMenuItems: some View {
        Button {
            actions.addChild(node)
        } label: {
            Label("Add Child", systemImage: "doc.badge.plus")
        }

        Button {
            actions.rename(node)
        } label: {
            Label("Rename...", systemImage: "pencil")
        }

        Divider()

        Button(role: .destructive) {
            actions.delete(node)
        } label: {
            Label("Delete", systemImage: "trash")
        }
    }
}
