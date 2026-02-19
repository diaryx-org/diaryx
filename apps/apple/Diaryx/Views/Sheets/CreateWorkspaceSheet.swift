import SwiftUI

struct CreateWorkspaceSheet: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss

    @State private var name: String = ""
    @State private var selectedURL: URL?

    var body: some View {
        VStack(spacing: 16) {
            Text("New Workspace")
                .font(.headline)

            TextField("Workspace name", text: $name)
                .textFieldStyle(.roundedBorder)

            #if os(macOS)
            HStack {
                if let url = selectedURL {
                    Text(url.path)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                } else {
                    Text("Default location (Documents)")
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                }

                Spacer()

                Button("Choose Folder...") {
                    chooseFolder()
                }
                .buttonStyle(.bordered)
            }
            #else
            Text("Workspace will be created in the app's Documents folder.")
                .font(.caption)
                .foregroundStyle(.tertiary)
            #endif

            HStack {
                Button("Cancel", role: .cancel) {
                    dismiss()
                }
                .keyboardShortcut(.cancelAction)

                Button("Create") {
                    createWorkspace()
                }
                .keyboardShortcut(.defaultAction)
                .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding()
        .frame(minWidth: 380)
    }

    private func createWorkspace() {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }

        let url: URL
        let storageType: WorkspaceRegistryEntry.StorageType

        if let chosen = selectedURL {
            url = chosen
            storageType = .folder
        } else {
            url = Self.defaultDocumentsDir().appendingPathComponent(trimmed)
            storageType = .appDocuments
        }

        appState.createAndOpenWorkspace(name: trimmed, url: url, storageType: storageType)
        dismiss()
    }

    #if os(macOS)
    private func chooseFolder() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.canCreateDirectories = true
        panel.allowsMultipleSelection = false
        panel.message = "Choose a folder for the new workspace"

        guard panel.runModal() == .OK, let url = panel.url else { return }
        selectedURL = url
        if name.trimmingCharacters(in: .whitespaces).isEmpty {
            name = url.lastPathComponent
        }
    }
    #endif

    private static func defaultDocumentsDir() -> URL {
        #if os(iOS)
        return FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first!
        #else
        return FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first!
            .appendingPathComponent("Diaryx")
        #endif
    }
}
