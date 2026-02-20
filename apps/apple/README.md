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

`apps/apple` contains the native SwiftUI app for macOS and iOS. It embeds a TipTap editor inside `WKWebView` and supports multi-workspace, settings, and a command palette.

## Architecture

### App Structure

```
DiaryxApp.swift           — App entry point, injects AppState + AppSettings
Views/RootView.swift      — Routes between WelcomeView and WorkspaceView
ContentView.swift         — WorkspaceView: sidebar + editor + inspector
State/AppState.swift      — @Observable: workspace registry, active view
State/WorkspaceState.swift — @Observable: per-workspace state + actions
State/AppSettings.swift   — @Observable: theme preference
State/WorkspaceRegistryEntry.swift — Codable workspace metadata
```

### View Hierarchy

```
RootView
├── WelcomeView           — Hero + recent workspaces + quick create/open actions
└── WorkspaceView
    ├── SidebarView       — File tree with drag-and-drop
    │   ├── FileTreeRow   — Recursive tree node with context menu
    │   ├── NewEntrySheet
    │   ├── RenameSheet
    │   └── AddChildSheet
    ├── EditorDetailView  — TipTap WKWebView wrapper
    ├── MetadataSidebar   — Read-only frontmatter inspector
    └── CommandPaletteView — Cmd+K command/file/content search
```

### Platform Abstraction

`EditorWebView.swift` uses `#if os(iOS)` / `#else` to provide:
- iOS: `UIViewRepresentable` wrapping `WKWebView`
- macOS: `NSViewRepresentable` wrapping `WKWebView`

The shared `Coordinator` handles `WKScriptMessageHandler` and `WKNavigationDelegate` across both platforms. External URL opening uses platform-specific APIs (`UIApplication.shared.open` vs `NSWorkspace.shared.open`).

## Multi-Workspace

`AppState` manages a registry of workspaces persisted to `UserDefaults` as JSON.

- **macOS**: Workspaces can be opened from any folder via `NSOpenPanel`. Security-scoped bookmarks maintain sandbox access across launches.
- **iOS**: Workspaces are created in the app's Documents directory.
- **Default workspace**: Welcome includes a one-tap "Create Default Workspace" action that creates/opens an app-managed `Documents/Diaryx` workspace.

The `WorkspacePicker` dropdown in the toolbar allows quick switching between registered workspaces.

## Default Workspace Bootstrap (Rust Backend)

When using `RustWorkspaceBackend`, creating a workspace now bootstraps a root index file if none exists in the target directory:

- Calls `diaryx_apple::create_workspace(path)`
- Ensures the directory exists
- Initializes a Diaryx root (`README.md` with `contents: []`) via `diaryx_core::workspace::init_workspace`
- Leaves existing root-index-based workspaces unchanged

This means a newly created workspace opens with a valid Diaryx structure immediately, instead of an empty folder.

## Command Palette

Activated via Cmd+K (macOS) or the magnifying glass toolbar button (iOS). Features:

- **Commands**: Daily Entry, New Entry, Duplicate, Rename, Delete, Add Child, Refresh Tree
- **Files**: Client-side filtering of the file tree by name/path
- **Content**: Full-text search via the Rust search API (debounced 200ms)

## Settings

- **macOS**: Native Settings scene (Cmd+,)
- **iOS**: Pushed from gear icon
- **App settings**: Theme (system/light/dark)
- **Workspace settings**: Filename style, link format, daily entry folder, auto-rename, auto-timestamp, sync title to heading

## Editor Bridge

The JavaScript bridge (`apps/apple/editor-bundle/src/main.ts`) exposes:

- `editorBridge.setMarkdown(markdown: string)`
- `editorBridge.getMarkdown(): string`
- `editorBridge.setJSON(json: string)`
- `editorBridge.getJSON(): string`
- `editorBridge.setEditable(editable: boolean)`

## Workspace Backend Abstraction

`WorkspaceView` consumes a `WorkspaceBackend` protocol:

- `listEntries()`, `getEntry(id:)`, `saveEntry(id:markdown:)`, `saveEntryBody(id:body:)`
- `createEntry(path:markdown:)`, `createFolder(path:)`, `buildFileTree()`
- Hierarchy: `createChildEntry`, `moveEntry`, `attachAndMoveEntryToParent`, `convertToIndex/Leaf`, `renameEntry`, `deleteEntry`
- Extended (Rust only): `searchWorkspace`, `getWorkspaceConfig`, `setWorkspaceConfigField`, `getOrCreateDailyEntry`, `duplicateEntry`

Two implementations:
- `RustWorkspaceBackend` — wraps `diaryx_apple` UniFFI bindings
- `LocalWorkspaceBackend` — pure-Swift `FileManager` fallback

## Building

```bash
./setup.sh          # builds editor bundle, Rust library + UniFFI bindings
open Diaryx.xcodeproj
```

To rebuild just the Rust library:

```bash
./build-rust.sh                # release, all platforms (macOS + iOS + iOS Sim)
./build-rust.sh debug          # debug, all platforms
./build-rust.sh release mac    # release, macOS only (faster)
```

`build-rust.sh` produces:
- `diaryx_apple.xcframework/` — static XCFramework (1 or 3 platform slices)
- `Diaryx/Generated/diaryx_apple.swift` — generated Swift bindings
