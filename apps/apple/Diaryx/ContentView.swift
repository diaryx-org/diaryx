import SwiftUI

struct ContentView: View {
    private let backendFactory: any WorkspaceBackendFactory = AppBackends.makeDefaultFactory()

    @State private var backend: (any WorkspaceBackend)?
    @State private var files: [FileItem] = []
    @State private var selectedFile: FileItem?
    @State private var editorContent: String = ""
    @State private var workspaceURL: URL?
    @State private var isDirty: Bool = false
    @State private var lastError: String?

    var body: some View {
        NavigationSplitView {
            sidebar
        } detail: {
            detail
        }
        .navigationTitle(selectedFile?.name ?? "Diaryx")
        .focusedSceneValue(\.saveAction, SaveAction(save: saveCurrentFile))
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

    // MARK: - Sidebar

    @ViewBuilder
    private var sidebar: some View {
        VStack {
            if files.isEmpty && workspaceURL == nil {
                ContentUnavailableView {
                    Label("No Folder Open", systemImage: "folder")
                } description: {
                    Text("Open a folder of Markdown files to get started.")
                } actions: {
                    Button("Open Folder...") { pickFolder() }
                }
            } else if files.isEmpty {
                ContentUnavailableView {
                    Label("No Markdown Files", systemImage: "doc.text")
                } description: {
                    Text("This folder doesn't contain any .md files.")
                }
            } else {
                List(files, selection: $selectedFile) { file in
                    Label(file.name, systemImage: "doc.text")
                        .tag(file)
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
        }
        .frame(minWidth: 200)
        .onChange(of: selectedFile) { _, newFile in
            if let file = newFile {
                loadFile(file)
            }
        }
    }

    // MARK: - Detail

    @ViewBuilder
    private var detail: some View {
        if selectedFile != nil {
            EditorWebView(
                initialMarkdown: editorContent,
                onContentChanged: { markdown in
                    editorContent = markdown
                    isDirty = true
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
            refreshEntries()
        } catch {
            files = []
            selectedFile = nil
            backend = nil
            report(error)
        }
    }

    private func refreshEntries() {
        guard let backend else {
            files = []
            return
        }

        do {
            files = try backend.listEntries().map(FileItem.from(entry:))
            if let selectedFile,
               !files.contains(where: { $0.id == selectedFile.id }) {
                self.selectedFile = nil
                editorContent = ""
                isDirty = false
            }
        } catch {
            files = []
            report(error)
        }
    }

    private func loadFile(_ file: FileItem) {
        // Auto-save current file before switching
        if isDirty {
            saveCurrentFile()
        }

        guard let backend else { return }

        do {
            let entry = try backend.getEntry(id: file.id)
            editorContent = entry.markdown
            isDirty = false
        } catch {
            report(error)
            editorContent = "Error loading file: \(error.localizedDescription)"
        }
    }

    private func saveCurrentFile() {
        guard let file = selectedFile, let backend, isDirty else { return }
        do {
            try backend.saveEntry(id: file.id, markdown: editorContent)
            isDirty = false
        } catch {
            report(error)
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

        if let target = files.first(where: { $0.id == targetPath }) {
            selectedFile = target
            return
        }

        if !targetPath.hasSuffix(".md"),
           let target = files.first(where: { $0.id == "\(targetPath).md" }) {
            selectedFile = target
        }
    }

    private func report(_ error: Error) {
        print("[ContentView] \(error)")
        lastError = error.localizedDescription
    }
}
