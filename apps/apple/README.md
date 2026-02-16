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
