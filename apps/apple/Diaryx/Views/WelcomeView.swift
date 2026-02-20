import SwiftUI

struct WelcomeView: View {
    @Environment(AppState.self) private var appState

    @State private var showCreateSheet = false

    var body: some View {
        ZStack {
            background

            ScrollView {
                VStack(spacing: 24) {
                    heroCard

                    if appState.workspaceRegistry.isEmpty {
                        emptyStateCard
                    } else {
                        recentWorkspacesCard
                    }

                    actionCard
                }
                .frame(maxWidth: 760)
                .padding(.horizontal, 20)
                .padding(.vertical, 36)
            }
            .scrollIndicators(.hidden)
        }
        .sheet(isPresented: $showCreateSheet) {
            CreateWorkspaceSheet()
        }
    }

    // MARK: - Layout Sections

    @ViewBuilder
    private var background: some View {
        LinearGradient(
            colors: [
                Color(red: 0.93, green: 0.97, blue: 0.98),
                Color(red: 0.99, green: 0.94, blue: 0.88),
            ],
            startPoint: .topLeading,
            endPoint: .bottomTrailing
        )
        .ignoresSafeArea()

        Circle()
            .fill(Color.accentColor.opacity(0.16))
            .frame(width: 420, height: 420)
            .blur(radius: 50)
            .offset(x: -220, y: -300)
            .allowsHitTesting(false)

        Circle()
            .fill(Color.cyan.opacity(0.16))
            .frame(width: 360, height: 360)
            .blur(radius: 55)
            .offset(x: 250, y: 280)
            .allowsHitTesting(false)
    }

    @ViewBuilder
    private var heroCard: some View {
        HStack(alignment: .top, spacing: 16) {
            Image(systemName: "book.pages.fill")
                .font(.system(size: 28, weight: .semibold))
                .foregroundStyle(Color.white)
                .frame(width: 56, height: 56)
                .background(Color.accentColor, in: RoundedRectangle(cornerRadius: 16, style: .continuous))

            VStack(alignment: .leading, spacing: 6) {
                Text("Diaryx")
                    .font(.largeTitle.bold())
                Text("Local-first writing with structured Markdown metadata.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }

            Spacer(minLength: 0)
        }
        .padding(24)
        .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 24, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: 24, style: .continuous)
                .stroke(.white.opacity(0.45), lineWidth: 1)
        }
    }

    @ViewBuilder
    private var emptyStateCard: some View {
        VStack(alignment: .leading, spacing: 10) {
            Label("No workspaces yet", systemImage: "folder.badge.questionmark")
                .font(.headline)
            Text("Create a default workspace instantly, or set up a custom one.")
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(20)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
    }

    @ViewBuilder
    private var recentWorkspacesCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Recent Workspaces")
                    .font(.headline)
                Spacer()
                Text("\(recentEntries.count)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 3)
                    .background(.white.opacity(0.4), in: Capsule())
            }

            ForEach(Array(recentEntries.enumerated()), id: \.element.id) { index, entry in
                Button {
                    appState.openWorkspace(entry: entry)
                } label: {
                    WorkspaceRow(entry: entry)
                }
                .buttonStyle(.plain)
                .contextMenu {
                    Button("Remove from List", role: .destructive) {
                        appState.removeWorkspace(id: entry.id)
                    }
                }

                if index < recentEntries.count - 1 {
                    Divider()
                }
            }
        }
        .padding(20)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: 20, style: .continuous)
                .stroke(.white.opacity(0.35), lineWidth: 1)
        }
    }

    @ViewBuilder
    private var actionCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Get Started")
                .font(.headline)

            Text("Use the default workspace path, or customize location and name.")
                .font(.subheadline)
                .foregroundStyle(.secondary)

            ViewThatFits {
                HStack(spacing: 10) {
                    newWorkspaceButton
                    #if os(macOS)
                    openFolderButton
                    #endif
                }

                VStack(alignment: .leading, spacing: 8) {
                    newWorkspaceButton
                    #if os(macOS)
                    openFolderButton
                    #endif
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(20)
        .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
    }

    private var newWorkspaceButton: some View {
        Button {
            showCreateSheet = true
        } label: {
            Label("New Workspace", systemImage: "plus")
        }
        .buttonStyle(.borderedProminent)
    }

    #if os(macOS)
    private var openFolderButton: some View {
        Button {
            openExistingFolder()
        } label: {
            Label("Open Folder", systemImage: "folder")
        }
        .buttonStyle(.bordered)
    }
    #endif

    private var recentEntries: [WorkspaceRegistryEntry] {
        Array(appState.workspaceRegistry.prefix(8))
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
        HStack(spacing: 12) {
            Image(systemName: entry.storageType == .folder ? "folder.fill" : "internaldrive.fill")
                .font(.headline)
                .foregroundStyle(.secondary)
                .frame(width: 26)

            VStack(alignment: .leading, spacing: 2) {
                Text(entry.name)
                    .font(.body.weight(.medium))
                    .lineLimit(1)

                Text(entry.path)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }

            Spacer(minLength: 12)

            Text(entry.lastOpenedAt, style: .relative)
                .font(.caption2)
                .foregroundStyle(.tertiary)
        }
        .contentShape(Rectangle())
        .padding(.vertical, 4)
    }
}
