import SwiftUI

struct WorkspacePicker: View {
    @Environment(AppState.self) private var appState
    let currentWorkspaceId: UUID

    var body: some View {
        Menu {
            ForEach(appState.workspaceRegistry) { entry in
                Button {
                    if entry.id != currentWorkspaceId {
                        appState.openWorkspace(entry: entry)
                    }
                } label: {
                    HStack {
                        Text(entry.name)
                        if entry.id == currentWorkspaceId {
                            Image(systemName: "checkmark")
                        }
                    }
                }
            }

            Divider()

            Button {
                appState.switchToWelcome()
            } label: {
                Label("All Workspaces...", systemImage: "square.grid.2x2")
            }
        } label: {
            HStack(spacing: 6) {
                Image(systemName: "book.closed")
                    .font(.caption)
                Text(currentWorkspaceName)
                    .font(.caption)
                    .fontWeight(.medium)
                    .lineLimit(1)
                Image(systemName: "chevron.up.chevron.down")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            .background(.quaternary, in: RoundedRectangle(cornerRadius: 6))
        }
        .buttonStyle(.plain)
    }

    private var currentWorkspaceName: String {
        appState.workspaceRegistry.first { $0.id == currentWorkspaceId }?.name ?? "Workspace"
    }
}
