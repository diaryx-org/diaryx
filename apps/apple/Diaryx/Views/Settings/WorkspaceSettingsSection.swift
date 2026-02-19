import SwiftUI

struct WorkspaceSettingsSection: View {
    let workspace: WorkspaceState

    @State private var config: WorkspaceConfigData?
    @State private var loadError: String?

    var body: some View {
        Section("Workspace") {
            if let config {
                Picker("Filename Style", selection: filenameStyleBinding(config)) {
                    Text("Preserve").tag("Preserve")
                    Text("kebab-case").tag("KebabCase")
                    Text("snake_case").tag("SnakeCase")
                    Text("SCREAMING_SNAKE").tag("ScreamingSnakeCase")
                }

                Picker("Link Format", selection: linkFormatBinding(config)) {
                    Text("Root-relative").tag("MarkdownRoot")
                    Text("Relative").tag("MarkdownRelative")
                    Text("Wikilink").tag("Wikilink")
                }

                TextField("Daily Entry Folder", text: dailyFolderBinding(config))
                    .textFieldStyle(.roundedBorder)

                Toggle("Sync title to heading", isOn: toggleBinding(config, field: "sync_title_to_heading", current: config.syncTitleToHeading))

                Toggle("Auto-update timestamp", isOn: toggleBinding(config, field: "auto_update_timestamp", current: config.autoUpdateTimestamp))

                Toggle("Auto-rename to title", isOn: toggleBinding(config, field: "auto_rename_to_title", current: config.autoRenameToTitle))
            } else if let loadError {
                Text(loadError)
                    .foregroundStyle(.secondary)
                    .font(.caption)
            } else {
                Text("No workspace configuration available.")
                    .foregroundStyle(.secondary)
                    .font(.caption)
            }
        }
        .onAppear { loadConfig() }
    }

    private func loadConfig() {
        guard let backend = workspace.backend as? RustWorkspaceBackend else {
            loadError = "Workspace config requires the Rust backend."
            return
        }
        do {
            config = try backend.getWorkspaceConfig()
        } catch {
            loadError = "Could not load config: \(error.localizedDescription)"
        }
    }

    private func setField(_ field: String, _ value: String) {
        guard let backend = workspace.backend as? RustWorkspaceBackend else { return }
        do {
            try backend.setWorkspaceConfigField(field: field, value: value)
            // Reload after change
            config = try backend.getWorkspaceConfig()
        } catch {
            print("[WorkspaceSettings] Failed to set \(field): \(error)")
        }
    }

    // MARK: - Bindings

    private func filenameStyleBinding(_ config: WorkspaceConfigData) -> Binding<String> {
        Binding(
            get: { config.filenameStyle },
            set: { setField("filename_style", $0) }
        )
    }

    private func linkFormatBinding(_ config: WorkspaceConfigData) -> Binding<String> {
        Binding(
            get: { config.linkFormat },
            set: { setField("link_format", $0) }
        )
    }

    private func dailyFolderBinding(_ config: WorkspaceConfigData) -> Binding<String> {
        Binding(
            get: { config.dailyEntryFolder ?? "" },
            set: { setField("daily_entry_folder", $0) }
        )
    }

    private func toggleBinding(_ config: WorkspaceConfigData, field: String, current: Bool) -> Binding<Bool> {
        Binding(
            get: { current },
            set: { setField(field, $0 ? "true" : "false") }
        )
    }
}
