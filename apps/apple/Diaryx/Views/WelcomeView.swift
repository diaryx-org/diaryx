import SwiftUI

struct WelcomeView: View {
    @Environment(AppState.self) private var appState

    @State private var showCreateSheet = false

    var body: some View {
        VStack(spacing: 24) {
            Spacer()

            Image(systemName: "book.closed.fill")
                .resizable()
                .scaledToFit()
                .frame(width: 64, height: 64)
                .foregroundStyle(.tint)

            Text("Diaryx")
                .font(.largeTitle.bold())

            Text("Your local-first journal and knowledge base")
                .font(.subheadline)
                .foregroundStyle(.secondary)

            if appState.workspaceRegistry.isEmpty {
                emptyState
            } else {
                recentWorkspaces
            }

            Spacer()

            actionButtons
                .padding(.bottom, 24)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .sheet(isPresented: $showCreateSheet) {
            CreateWorkspaceSheet()
        }
    }

    // MARK: - Sections

    @ViewBuilder
    private var emptyState: some View {
        VStack(spacing: 8) {
            Text("No workspaces yet")
                .font(.headline)
                .foregroundStyle(.secondary)
            Text("Create a new workspace to get started.")
                .font(.subheadline)
                .foregroundStyle(.tertiary)
        }
        .padding(.top, 16)
    }

    @ViewBuilder
    private var recentWorkspaces: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Recent Workspaces")
                .font(.headline)
                .padding(.horizontal, 4)

            List {
                ForEach(appState.workspaceRegistry) { entry in
                    WorkspaceRow(entry: entry)
                        .contentShape(Rectangle())
                        .onTapGesture {
                            appState.openWorkspace(entry: entry)
                        }
                        .contextMenu {
                            Button("Remove from List", role: .destructive) {
                                appState.removeWorkspace(id: entry.id)
                            }
                        }
                }
            }
            .listStyle(.inset)
            .frame(maxWidth: 400)
            .frame(maxHeight: 300)
            .clipShape(RoundedRectangle(cornerRadius: 8))
        }
    }

    @ViewBuilder
    private var actionButtons: some View {
        HStack(spacing: 12) {
            Button {
                showCreateSheet = true
            } label: {
                Label("New Workspace", systemImage: "plus")
            }
            .buttonStyle(.borderedProminent)

            #if os(macOS)
            Button {
                openExistingFolder()
            } label: {
                Label("Open Folder", systemImage: "folder")
            }
            .buttonStyle(.bordered)
            #endif
        }
    }

    // MARK: - Actions

    #if os(macOS)
    private func openExistingFolder() {
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
    }
    #endif
}

// MARK: - Workspace Row

private struct WorkspaceRow: View {
    let entry: WorkspaceRegistryEntry

    var body: some View {
        HStack {
            Image(systemName: entry.storageType == .folder ? "folder" : "doc.text")
                .foregroundStyle(.secondary)
                .frame(width: 24)

            VStack(alignment: .leading, spacing: 2) {
                Text(entry.name)
                    .font(.body)
                Text(entry.path)
                    .font(.caption)
                    .foregroundStyle(.tertiary)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }

            Spacer()

            Text(entry.lastOpenedAt, style: .relative)
                .font(.caption2)
                .foregroundStyle(.tertiary)
        }
        .padding(.vertical, 4)
    }
}
