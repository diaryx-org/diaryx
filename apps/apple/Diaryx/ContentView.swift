import SwiftUI
import UniformTypeIdentifiers

struct ContentView: View {
    @State private var files: [FileItem] = []
    @State private var selectedFile: FileItem?
    @State private var editorContent: String = ""
    @State private var workspaceURL: URL?
    @State private var isDirty: Bool = false

    var body: some View {
        NavigationSplitView {
            sidebar
        } detail: {
            detail
        }
        .navigationTitle(selectedFile?.name ?? "Diaryx")
        .focusedSceneValue(\.saveAction, SaveAction(save: saveCurrentFile))
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
        scanFolder(url)
    }

    private func scanFolder(_ url: URL) {
        do {
            let contents = try FileManager.default.contentsOfDirectory(
                at: url,
                includingPropertiesForKeys: [.isRegularFileKey],
                options: [.skipsHiddenFiles]
            )
            files = contents
                .filter { $0.pathExtension.lowercased() == "md" }
                .sorted { $0.lastPathComponent.localizedCompare($1.lastPathComponent) == .orderedAscending }
                .map { FileItem(id: $0, url: $0) }
        } catch {
            print("Error scanning folder: \(error)")
            files = []
        }
    }

    private func loadFile(_ file: FileItem) {
        // Auto-save current file before switching
        if isDirty {
            saveCurrentFile()
        }

        do {
            editorContent = try String(contentsOf: file.url, encoding: .utf8)
            isDirty = false
        } catch {
            print("Error loading file: \(error)")
            editorContent = "Error loading file: \(error.localizedDescription)"
        }
    }

    private func saveCurrentFile() {
        guard let file = selectedFile, isDirty else { return }
        do {
            try editorContent.write(to: file.url, atomically: true, encoding: .utf8)
            isDirty = false
        } catch {
            print("Error saving file: \(error)")
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
        guard let workspace = workspaceURL else { return }
        let targetURL = workspace.appendingPathComponent(href)

        if let target = files.first(where: { $0.url == targetURL }) {
            selectedFile = target
        }
    }
}
