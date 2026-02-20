import SwiftUI

struct CommandPaletteView: View {
    let workspace: WorkspaceState
    let onDismiss: () -> Void

    @State private var query: String = ""
    @State private var searchResults: [FileSearchResultData] = []
    @State private var searchTask: Task<Void, Never>?

    var body: some View {
        VStack(spacing: 0) {
            // Search field
            HStack(spacing: 8) {
                Image(systemName: "magnifyingglass")
                    .foregroundStyle(.secondary)
                TextField("Type a command, filename, or search term...", text: $query)
                    .textFieldStyle(.plain)
                    .font(.body)
                    .onSubmit { executeFirstResult() }

                if !query.isEmpty {
                    Button {
                        query = ""
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundStyle(.tertiary)
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 10)

            Divider()

            // Results
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    if !filteredCommands.isEmpty {
                        sectionHeader("Commands")
                        ForEach(filteredCommands) { cmd in
                            CommandRow(command: cmd) {
                                cmd.action(workspace)
                                onDismiss()
                            }
                        }
                    }

                    if !filteredFiles.isEmpty {
                        sectionHeader("Files")
                        ForEach(filteredFiles.prefix(10), id: \.path) { node in
                            FileRow(node: node) {
                                workspace.selectedPath = node.path
                                onDismiss()
                            }
                        }
                    }

                    if !searchResults.isEmpty {
                        sectionHeader("Content Matches")
                        ForEach(searchResults.prefix(5), id: \.path) { result in
                            ContentMatchRow(result: result) {
                                workspace.selectedPath = result.path
                                onDismiss()
                            }
                        }
                    }

                    if query.isEmpty && filteredCommands.isEmpty {
                        // Show all commands when empty
                        sectionHeader("Commands")
                        ForEach(allAvailableCommands) { cmd in
                            CommandRow(command: cmd) {
                                cmd.action(workspace)
                                onDismiss()
                            }
                        }
                    }
                }
                .padding(.vertical, 4)
            }
            #if os(macOS)
            .frame(maxHeight: 400)
            #endif
        }
        #if os(macOS)
        .frame(width: 500)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12))
        .shadow(color: .black.opacity(0.2), radius: 20, y: 10)
        #endif
        .onChange(of: query) { _, newQuery in
            debounceSearch(newQuery)
        }
    }

    // MARK: - Filtered Results

    private var allAvailableCommands: [PaletteCommand] {
        CommandRegistry.commands.filter { cmd in
            !cmd.requiresEntry || workspace.selectedPath != nil
        }
    }

    private var filteredCommands: [PaletteCommand] {
        guard !query.isEmpty else { return [] }
        let lowered = query.lowercased()
        return allAvailableCommands.filter { cmd in
            cmd.title.lowercased().contains(lowered)
        }
    }

    private var filteredFiles: [SidebarTreeNode] {
        guard !query.isEmpty else { return [] }
        let lowered = query.lowercased()
        return collectAllFiles().filter { node in
            node.name.lowercased().contains(lowered) ||
            node.path.lowercased().contains(lowered)
        }
    }

    private func collectAllFiles() -> [SidebarTreeNode] {
        guard let tree = workspace.fileTree else { return [] }
        var files: [SidebarTreeNode] = []
        collectFiles(from: tree, into: &files)
        return files
    }

    private func collectFiles(from node: SidebarTreeNode, into files: inout [SidebarTreeNode]) {
        if !node.isFolder && !node.path.isEmpty {
            files.append(node)
        }
        for child in node.children {
            collectFiles(from: child, into: &files)
        }
    }

    // MARK: - Search

    private func debounceSearch(_ query: String) {
        searchTask?.cancel()
        guard !query.isEmpty, query.count >= 2 else {
            searchResults = []
            return
        }
        searchTask = Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(200))
            guard !Task.isCancelled else { return }
            performSearch(query)
        }
    }

    private func performSearch(_ query: String) {
        guard let backend = workspace.backend as? RustWorkspaceBackend else { return }
        do {
            let results = try backend.searchWorkspace(query: query)
            searchResults = results.files
        } catch {
            print("[CommandPalette] Search failed: \(error)")
        }
    }

    private func executeFirstResult() {
        if let first = filteredCommands.first {
            first.action(workspace)
            onDismiss()
        } else if let first = filteredFiles.first {
            workspace.selectedPath = first.path
            onDismiss()
        } else if let first = searchResults.first {
            workspace.selectedPath = first.path
            onDismiss()
        }
    }

    // MARK: - Section Header

    @ViewBuilder
    private func sectionHeader(_ title: String) -> some View {
        Text(title.uppercased())
            .font(.caption2)
            .fontWeight(.semibold)
            .foregroundStyle(.secondary)
            .padding(.horizontal, 16)
            .padding(.top, 8)
            .padding(.bottom, 4)
    }
}

// MARK: - Row Views

private struct CommandRow: View {
    let command: PaletteCommand
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 10) {
                Image(systemName: command.icon)
                    .frame(width: 20)
                    .foregroundStyle(.secondary)
                Text(command.title)
                Spacer()
                if let shortcut = command.shortcut {
                    Text(shortcut)
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                }
            }
            .contentShape(Rectangle())
            .padding(.horizontal, 16)
            .padding(.vertical, 6)
        }
        .buttonStyle(.plain)
    }
}

private struct FileRow: View {
    let node: SidebarTreeNode
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 10) {
                Image(systemName: "doc.text")
                    .frame(width: 20)
                    .foregroundStyle(.secondary)
                VStack(alignment: .leading, spacing: 1) {
                    Text(node.displayName)
                    Text(node.path)
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                }
                Spacer()
            }
            .contentShape(Rectangle())
            .padding(.horizontal, 16)
            .padding(.vertical, 4)
        }
        .buttonStyle(.plain)
    }
}

private struct ContentMatchRow: View {
    let result: FileSearchResultData
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 10) {
                Image(systemName: "text.magnifyingglass")
                    .frame(width: 20)
                    .foregroundStyle(.secondary)
                VStack(alignment: .leading, spacing: 1) {
                    Text(result.title ?? (result.path as NSString).lastPathComponent)
                    if let firstMatch = result.matches.first {
                        Text(firstMatch.lineContent.trimmingCharacters(in: .whitespaces))
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                    Text(result.path)
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                }
                Spacer()
                Text("\(result.matches.count)")
                    .font(.caption2)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(.quaternary, in: Capsule())
            }
            .contentShape(Rectangle())
            .padding(.horizontal, 16)
            .padding(.vertical, 4)
        }
        .buttonStyle(.plain)
    }
}
