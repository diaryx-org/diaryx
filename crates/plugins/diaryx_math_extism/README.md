---
title: "Math"
description: "LaTeX math rendering with inline ($...$) and block ($$...$$) support"
id: "diaryx.math"
version: "0.1.3"
author: "Diaryx Team"
license: "PolyForm Shield 1.0.0"
repository: "https://github.com/diaryx-org/diaryx"
categories: ["editor", "formatting"]
tags: ["math", "latex", "editor"]
capabilities: ["editor_extension"]
artifact:
  url: ""
  sha256: ""
  size: 0
  published_at: ""
ui:
  - slot: EditorExtension
    id: mathInline
    label: "Math"
  - slot: EditorExtension
    id: mathBlock
    label: "Math Block"
---

# diaryx_math_extism

Extism WASM guest plugin that renders LaTeX math expressions to MathML.

## Overview

This plugin contributes two `EditorExtension` slots via the plugin manifest:

- **Inline math** (`$...$`) — `InlineAtom` node with popover editing
- **Block math** (`$$...$$`) — `BlockAtom` node with source toggle editing

The host generates TipTap `Node` extensions from the manifest and calls the plugin's `render` export to convert LaTeX to MathML via `pulldown-latex`.

## Plugin ID

`diaryx.math`

## Exports

| Export | Description |
|--------|-------------|
| `manifest()` | Plugin metadata + editor extension declarations |
| `render(params)` | Render LaTeX string to MathML |

## Build

```bash
cargo build -p diaryx_math_extism --target wasm32-wasip1 --release
```
