import SwiftUI

enum CommandCategory: String, CaseIterable {
    case general = "General"
    case currentEntry = "Current Entry"
    case workspace = "Workspace"
}

struct PaletteCommand: Identifiable {
    let id: String
    let title: String
    let icon: String
    let shortcut: String?
    let category: CommandCategory
    /// Whether this command requires an active entry selection.
    let requiresEntry: Bool
    let action: (WorkspaceState) -> Void

    init(
        _ id: String,
        title: String,
        icon: String,
        shortcut: String? = nil,
        category: CommandCategory,
        requiresEntry: Bool = false,
        action: @escaping (WorkspaceState) -> Void
    ) {
        self.id = id
        self.title = title
        self.icon = icon
        self.shortcut = shortcut
        self.category = category
        self.requiresEntry = requiresEntry
        self.action = action
    }
}

@MainActor
struct CommandRegistry {
    static let commands: [PaletteCommand] = [
        // General
        PaletteCommand(
            "daily_entry",
            title: "Open Daily Entry",
            icon: "calendar",
            shortcut: nil,
            category: .general,
            action: { ws in
                guard let backend = ws.backend as? RustWorkspaceBackend else { return }
                do {
                    let path = try backend.getOrCreateDailyEntry(dateString: nil)
                    ws.refreshTree()
                    ws.selectedPath = path
                } catch {
                    ws.lastError = error.localizedDescription
                }
            }
        ),
        PaletteCommand(
            "new_entry",
            title: "New Entry",
            icon: "doc.badge.plus",
            shortcut: nil,
            category: .general,
            action: { ws in
                ws.newEntryName = ""
                ws.showNewEntrySheet = true
            }
        ),
        PaletteCommand(
            "refresh_tree",
            title: "Refresh File Tree",
            icon: "arrow.clockwise",
            shortcut: nil,
            category: .workspace,
            action: { ws in ws.refreshTree() }
        ),

        // Current Entry
        PaletteCommand(
            "duplicate",
            title: "Duplicate Entry",
            icon: "doc.on.doc",
            shortcut: nil,
            category: .currentEntry,
            requiresEntry: true,
            action: { ws in
                guard let path = ws.selectedPath,
                      let backend = ws.backend as? RustWorkspaceBackend else { return }
                do {
                    let newPath = try backend.duplicateEntry(path: path)
                    ws.refreshTree()
                    ws.selectedPath = newPath
                } catch {
                    ws.lastError = error.localizedDescription
                }
            }
        ),
        PaletteCommand(
            "rename",
            title: "Rename Entry",
            icon: "pencil",
            shortcut: nil,
            category: .currentEntry,
            requiresEntry: true,
            action: { ws in
                guard let path = ws.selectedPath else { return }
                if let node = ws.findNodeByPath(path) {
                    ws.renameText = node.name
                    ws.nodeToRename = node
                }
            }
        ),
        PaletteCommand(
            "delete",
            title: "Delete Entry",
            icon: "trash",
            shortcut: nil,
            category: .currentEntry,
            requiresEntry: true,
            action: { ws in
                guard let path = ws.selectedPath else { return }
                if let node = ws.findNodeByPath(path) {
                    ws.nodeToDelete = node
                }
            }
        ),
        PaletteCommand(
            "add_child",
            title: "Add Child Entry",
            icon: "doc.badge.plus",
            shortcut: nil,
            category: .currentEntry,
            requiresEntry: true,
            action: { ws in
                guard let path = ws.selectedPath else { return }
                if let node = ws.findNodeByPath(path) {
                    ws.newChildTitle = ""
                    ws.nodeForNewChild = node
                }
            }
        ),
    ]
}
