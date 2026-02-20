import SwiftUI

struct WorkspaceView: View {
    @Bindable var workspace: WorkspaceState
    @Environment(AppState.self) private var appState

    @State private var showCommandPalette = false

    var body: some View {
        NavigationSplitView {
            sidebar
        } detail: {
            detail
        }
        .inspector(isPresented: $workspace.showInspector) {
            MetadataSidebar(metadata: workspace.currentMetadata)
                .inspectorColumnWidth(min: 200, ideal: 260, max: 400)
        }
        .focusedSceneValue(\.saveAction, SaveAction(save: {
            workspace.saveFile(path: workspace.selectedPath, content: workspace.editorContent)
        }))
        #if os(macOS)
        .overlay {
            if showCommandPalette {
                ZStack {
                    Color.black.opacity(0.001)
                        .ignoresSafeArea()
                        .onTapGesture { showCommandPalette = false }

                    VStack {
                        CommandPaletteView(
                            workspace: workspace,
                            onDismiss: { showCommandPalette = false }
                        )
                        .padding(.top, 60)
                        Spacer()
                    }
                }
                .transition(.opacity)
            }
        }
        .animation(.easeOut(duration: 0.15), value: showCommandPalette)
        #endif
        .focusedSceneValue(\.toggleCommandPalette, ToggleCommandPaletteAction {
            showCommandPalette.toggle()
        })
        .onKeyPress(.escape) {
            if showCommandPalette {
                showCommandPalette = false
                return .handled
            }
            return .ignored
        }
        .alert("Error", isPresented: Binding(
            get: { workspace.lastError != nil },
            set: { if !$0 { workspace.lastError = nil } }
        )) {
            Button("OK", role: .cancel) {
                workspace.lastError = nil
            }
        } message: {
            Text(workspace.lastError ?? "Unknown error")
        }
        #if os(iOS)
        .sheet(isPresented: $showCommandPalette) {
            NavigationStack {
                CommandPaletteView(
                    workspace: workspace,
                    onDismiss: { showCommandPalette = false }
                )
                .navigationTitle("Command Palette")
                .navigationBarTitleDisplayMode(.inline)
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button("Done") { showCommandPalette = false }
                    }
                }
            }
        }
        #endif
    }

    // MARK: - Detail

    @ViewBuilder
    private var detail: some View {
        EditorDetailView(
            selectedPath: workspace.selectedPath,
            loadedMarkdown: workspace.loadedMarkdown,
            onContentChanged: { markdown in
                workspace.editorContent = markdown
                workspace.isDirty = true
                workspace.scheduleSave()
            },
            onLinkClicked: { workspace.handleLinkClick($0) }
        )
        .navigationTitle(workspace.selectedDisplayName)
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItemGroup(placement: .topBarTrailing) {
                Button {
                    showCommandPalette.toggle()
                } label: {
                    Label("Command Palette", systemImage: "magnifyingglass")
                }
                Button {
                    workspace.showInspector.toggle()
                } label: {
                    Label("Inspector", systemImage: "sidebar.trailing")
                }
                Button {
                    appState.switchToWelcome()
                } label: {
                    Label("Workspaces", systemImage: "square.grid.2x2")
                }
            }
        }
        #else
        .toolbar {
            ToolbarItem(placement: .automatic) {
                Button {
                    showCommandPalette.toggle()
                } label: {
                    Label("Command Palette", systemImage: "magnifyingglass")
                }
                .help("Command Palette (Cmd+K)")
            }
            ToolbarItem(placement: .automatic) {
                Button {
                    workspace.showInspector.toggle()
                } label: {
                    Label("Inspector", systemImage: "sidebar.trailing")
                }
                .help("Toggle metadata inspector")
            }
            ToolbarItem(placement: .automatic) {
                Button {
                    appState.switchToWelcome()
                } label: {
                    Label("Workspaces", systemImage: "square.grid.2x2")
                }
                .help("Switch workspace")
            }
        }
        #endif
    }

    // MARK: - Sidebar

    @ViewBuilder
    private var sidebar: some View {
        SidebarView(
            fileTree: workspace.fileTree,
            workspaceURL: URL(fileURLWithPath: workspace.registryEntry.path),
            hasBackend: workspace.backend != nil,
            selectedPath: $workspace.selectedPath,
            expandedFolders: $workspace.expandedFolders,
            showNewEntrySheet: $workspace.showNewEntrySheet,
            newEntryName: $workspace.newEntryName,
            nodeToRename: $workspace.nodeToRename,
            renameText: $workspace.renameText,
            nodeForNewChild: $workspace.nodeForNewChild,
            newChildTitle: $workspace.newChildTitle,
            nodeToDelete: $workspace.nodeToDelete,
            treeActions: workspace.treeActions,
            onPickFolder: { pickFolder() },
            onCreateNewEntry: { workspace.createNewEntry(name: $0) },
            onRenameNode: { workspace.renameNode($0, newName: $1) },
            onAddChild: { workspace.addChildToNode($0) },
            onDeleteNode: { workspace.deleteNode($0) }
        )
        .onChange(of: workspace.selectedPath) { oldPath, newPath in
            workspace.onSelectedPathChanged(oldPath: oldPath, newPath: newPath)
        }
    }

    // MARK: - Folder Picker

    private func pickFolder() {
        #if os(macOS)
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.message = "Select a folder containing Markdown files"

        guard panel.runModal() == .OK, let url = panel.url else { return }
        appState.registerAndOpenWorkspace(
            name: url.lastPathComponent,
            url: url,
            storageType: .folder
        )
        #endif
    }
}
