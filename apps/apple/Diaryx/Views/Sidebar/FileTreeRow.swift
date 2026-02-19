import SwiftUI

struct TreeNodeActions {
    let addChild: (SidebarTreeNode) -> Void
    let rename: (SidebarTreeNode) -> Void
    let delete: (SidebarTreeNode) -> Void
    let moveToParent: (String, SidebarTreeNode) -> Void
}

struct FileTreeRow: View {
    let node: SidebarTreeNode
    @Binding var expandedFolders: Set<String>
    let actions: TreeNodeActions

    var body: some View {
        if node.isFolder {
            DisclosureGroup(isExpanded: Binding(
                get: { expandedFolders.contains(node.id) },
                set: { isExpanded in
                    if isExpanded {
                        expandedFolders.insert(node.id)
                    } else {
                        expandedFolders.remove(node.id)
                    }
                }
            )) {
                ForEach(node.children) { child in
                    FileTreeRow(node: child, expandedFolders: $expandedFolders, actions: actions)
                }
            } label: {
                Label(node.displayName, systemImage: "folder")
                    .draggable(node.path)
                    .dropDestination(for: String.self) { paths, _ in
                        guard let draggedPath = paths.first, draggedPath != node.path else {
                            return false
                        }
                        actions.moveToParent(draggedPath, node)
                        return true
                    }
                    .contextMenu { contextMenuItems }
            }
            .tag(node.path)
        } else {
            Label(node.displayName, systemImage: "doc.text")
                .tag(node.path)
                .draggable(node.path)
                .dropDestination(for: String.self) { paths, _ in
                    guard let draggedPath = paths.first, draggedPath != node.path else {
                        return false
                    }
                    actions.moveToParent(draggedPath, node)
                    return true
                }
                .contextMenu { contextMenuItems }
        }
    }

    @ViewBuilder
    private var contextMenuItems: some View {
        Button {
            actions.addChild(node)
        } label: {
            Label("Add Child", systemImage: "doc.badge.plus")
        }

        Button {
            actions.rename(node)
        } label: {
            Label("Rename...", systemImage: "pencil")
        }

        Divider()

        Button(role: .destructive) {
            actions.delete(node)
        } label: {
            Label("Delete", systemImage: "trash")
        }
    }
}
