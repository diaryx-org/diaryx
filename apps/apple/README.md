---
title: apple
description: Native SwiftUI app for Diaryx using WKWebView + TipTap
author: adammharris
audience:
- developers
part_of: '[README](/apps/README.md)'
attachments:
- '[project.yml](/apps/apple/project.yml)'
- '[DiaryxApp](/apps/apple/Diaryx/DiaryxApp.swift)'
- '[ContentView](/apps/apple/Diaryx/ContentView.swift)'
- '[EditorWebView](/apps/apple/Diaryx/EditorWebView.swift)'
- '[WorkspaceBackend](/apps/apple/Diaryx/WorkspaceBackend.swift)'
- '[MetadataSidebar](/apps/apple/Diaryx/MetadataSidebar.swift)'
---

# Diaryx Apple App

`apps/apple` contains the native SwiftUI app that embeds a TipTap editor inside `WKWebView`.

## Editor Bridge

The JavaScript bridge (`apps/apple/editor-bundle/src/main.ts`) exposes:

- `editorBridge.setMarkdown(markdown: string)`
- `editorBridge.getMarkdown(): string`
- `editorBridge.setJSON(json: string)`
- `editorBridge.getJSON(): string`
- `editorBridge.setEditable(editable: boolean)`

Compatibility aliases are also kept for existing Swift call sites:

- `editorBridge.setContent(markdown: string)` -> `setMarkdown`
- `editorBridge.getContent()` -> `getMarkdown`

## Markdown Handling

Markdown is parsed/serialized through TipTap's `@tiptap/markdown` extension.
The Apple editor bundle does not use `marked` for markdown-to-HTML conversion.

## Workspace Backend Abstraction

`ContentView` now consumes a `WorkspaceBackend` protocol instead of directly reading/writing files.

- `WorkspaceBackendFactory.openWorkspace(at:)` — open an existing workspace
- `WorkspaceBackendFactory.createWorkspace(at:)` — create a new workspace directory
- `WorkspaceBackend.listEntries()` — flat list of all entries
- `WorkspaceBackend.buildFileTree()` → `SidebarTreeNode` — recursive directory tree (used by sidebar)
- `WorkspaceBackend.getEntry(id:)` — returns `WorkspaceEntryData` with `body`, `metadata`, and raw `markdown`
- `WorkspaceBackend.saveEntry(id:markdown:)` — save raw markdown
- `WorkspaceBackend.saveEntryBody(id:body:)` — save body only, preserving frontmatter
- `WorkspaceBackend.createEntry(path:markdown:)` — create a new markdown file (with parent dirs)
- `WorkspaceBackend.createFolder(path:)` — create a subfolder

### Hierarchy Manipulation

The backend also supports workspace hierarchy operations (Rust backend only):

- `createChildEntry(parentPath:title:)` → `CreateChildResultData` — add a child (auto-converts leaf parent to index)
- `moveEntry(fromPath:toPath:)` — move an entry
- `attachAndMoveEntryToParent(entryPath:parentPath:)` → `String` — reparent with frontmatter link updates
- `convertToIndex(path:)` / `convertToLeaf(path:)` → `String` — toggle leaf/index
- `setFrontmatterProperty(path:key:value:)` / `removeFrontmatterProperty(path:key:)` — edit frontmatter
- `renameEntry(path:newFilename:)` → `String` — rename a file
- `deleteEntry(path:)` — delete an entry

Two implementations are provided:

- `LocalWorkspaceBackend` — pure-Swift `FileManager` I/O (no Rust dependency). Hierarchy operations throw `.rustBackendUnavailable`.
- `RustWorkspaceBackend` — wraps the `diaryx_apple` UniFFI bindings, delegating to `diaryx_core`

Backend selection is controlled by `DIARYX_APPLE_BACKEND`:

- `rust` (default) — uses `RustWorkspaceBackend` via UniFFI
- `local` — uses `LocalWorkspaceBackend`

## File Tree Sidebar

The left sidebar displays a collapsible tree built from the workspace's `contents`/`part_of` hierarchy — the same tree-building logic used by the web app's LeftSidebar (`diaryx_core::Workspace::build_tree()`). If the workspace has a root index file (a `.md` with `contents` but no `part_of`), the tree follows frontmatter references. Otherwise it falls back to `build_filesystem_tree()` for plain directory structure. The `RustWorkspaceBackend` delegates to the core Rust tree builder, while `LocalWorkspaceBackend` implements a filesystem-based fallback in pure Swift.

## Context Menu

Right-clicking any node in the file tree sidebar shows a context menu with hierarchy operations:

- **Add Child** — creates a child entry under the node (auto-converts leaf to index folder)
- **Rename...** — opens a sheet to rename the file
- **Delete** — deletes the entry with a confirmation dialog (warns about folder contents)

These operations require the Rust backend (`RustWorkspaceBackend`). The local-only backend does not support hierarchy manipulation.

## Drag and Drop

Files and folders in the sidebar support drag-and-drop for reparenting:

- **Drag a file onto a folder** — moves the file into the folder, updating `contents`/`part_of` frontmatter links
- **Drag a file onto another file** — the target is auto-converted to a folder (index), and the dragged file becomes a child

Both operations use `attachAndMoveEntryToParent` from `diaryx_core`, which handles leaf-to-index conversion, bidirectional frontmatter link updates, and file relocation.

## Metadata Inspector

The right sidebar shows a read-only view of YAML frontmatter fields parsed from the current file. Toggle it with the toolbar button (sidebar.trailing icon) or hide it completely when not needed.

- Scalar values (title, date, draft) display as plain text
- Array values (tags, audience) display as bulleted lists
- Files without frontmatter show an empty state
- Editing in the TipTap editor only saves the body; frontmatter is preserved automatically via `saveEntryBody()`

## Building

```bash
./setup.sh          # builds editor bundle, Rust library + UniFFI bindings, then generates Xcode project
open Diaryx.xcodeproj
```

To rebuild just the Rust library and bindings:

```bash
./build-rust.sh             # release build (default)
./build-rust.sh debug       # debug build
```

`build-rust.sh` produces:
- `diaryx_apple.xcframework/` — static XCFramework linked by Xcode
- `Diaryx/Generated/diaryx_apple.swift` — generated Swift bindings (compiled as source)
