import SwiftUI

struct SidebarView: View {
    let fileTree: SidebarTreeNode?
    let workspaceURL: URL?
    let hasBackend: Bool
    @Binding var selectedPath: String?
    @Binding var expandedFolders: Set<String>
    @Binding var showNewEntrySheet: Bool
    @Binding var newEntryName: String
    @Binding var nodeToRename: SidebarTreeNode?
    @Binding var renameText: String
    @Binding var nodeForNewChild: SidebarTreeNode?
    @Binding var newChildTitle: String
    @Binding var nodeToDelete: SidebarTreeNode?
    let treeActions: TreeNodeActions
    let onPickFolder: () -> Void
    let onCreateNewEntry: (String) -> Void
    let onRenameNode: (SidebarTreeNode, String) -> Void
    let onAddChild: (SidebarTreeNode) -> Void
    let onDeleteNode: (SidebarTreeNode) -> Void

    var body: some View {
        VStack(spacing: 0) {
            if fileTree == nil && workspaceURL == nil {
                ContentUnavailableView {
                    Label("No Folder Open", systemImage: "folder")
                } description: {
                    Text("Open a folder of Markdown files to get started.")
                } actions: {
                    Button("Open Folder...") { onPickFolder() }
                }
            } else if let tree = fileTree, tree.children.isEmpty && tree.path.isEmpty {
                ContentUnavailableView {
                    Label("No Markdown Files", systemImage: "doc.text")
                } description: {
                    Text("This folder doesn't contain any .md files.")
                }
            } else if let tree = fileTree {
                List(selection: $selectedPath) {
                    if !tree.path.isEmpty {
                        FileTreeRow(node: tree, expandedFolders: $expandedFolders, actions: treeActions)
                    } else {
                        ForEach(tree.children) { child in
                            FileTreeRow(node: child, expandedFolders: $expandedFolders, actions: treeActions)
                        }
                    }
                }
                .listStyle(.sidebar)
            }
        }
        .toolbar {
            ToolbarItem {
                Button {
                    onPickFolder()
                } label: {
                    Label("Open Folder", systemImage: "folder.badge.plus")
                }
            }
            if hasBackend {
                ToolbarItem {
                    Button {
                        newEntryName = ""
                        showNewEntrySheet = true
                    } label: {
                        Label("New Entry", systemImage: "doc.badge.plus")
                    }
                }
            }
        }
        .sheet(isPresented: $showNewEntrySheet) {
            NewEntrySheet(
                entryName: $newEntryName,
                onCreate: { onCreateNewEntry(newEntryName) },
                onCancel: { showNewEntrySheet = false }
            )
        }
        .sheet(item: $nodeToRename) { node in
            RenameSheet(
                name: $renameText,
                onRename: { onRenameNode(node, renameText) },
                onCancel: { nodeToRename = nil }
            )
        }
        .sheet(item: $nodeForNewChild) { node in
            AddChildSheet(
                title: $newChildTitle,
                parentName: node.displayName,
                onCreate: { onAddChild(node) },
                onCancel: { nodeForNewChild = nil }
            )
        }
        .confirmationDialog(
            "Delete \"\(nodeToDelete?.displayName ?? "")\"?",
            isPresented: Binding(
                get: { nodeToDelete != nil },
                set: { if !$0 { nodeToDelete = nil } }
            ),
            titleVisibility: .visible
        ) {
            Button("Delete", role: .destructive) {
                if let node = nodeToDelete {
                    onDeleteNode(node)
                }
            }
            Button("Cancel", role: .cancel) {
                nodeToDelete = nil
            }
        } message: {
            if let node = nodeToDelete, node.isFolder {
                Text("This folder and all its contents will be permanently deleted.")
            } else {
                Text("This file will be permanently deleted.")
            }
        }
        .frame(minWidth: 200)
    }
}
