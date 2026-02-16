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

- `WorkspaceBackendFactory.openWorkspace(at:)`
- `WorkspaceBackend.listEntries()`
- `WorkspaceBackend.getEntry(id:)` — returns `WorkspaceEntryData` with `body`, `metadata`, and raw `markdown`
- `WorkspaceBackend.saveEntry(id:markdown:)` — save raw markdown
- `WorkspaceBackend.saveEntryBody(id:body:)` — save body only, preserving frontmatter

Two implementations are provided:

- `LocalWorkspaceBackend` — pure-Swift `FileManager` I/O (no Rust dependency)
- `RustWorkspaceBackend` — wraps the `diaryx_apple` UniFFI bindings, delegating to `diaryx_core`

Backend selection is controlled by `DIARYX_APPLE_BACKEND`:

- `local` (default) — uses `LocalWorkspaceBackend`
- `rust` — uses `RustWorkspaceBackend` via UniFFI

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
