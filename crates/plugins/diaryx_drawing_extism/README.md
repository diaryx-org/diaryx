---
title: Drawing Plugin
description: Freehand drawing blocks for Diaryx, shipped as an Extism WASM plugin with an iframe-based canvas editor.
---

# Drawing Plugin

Freehand drawing block extension for [Diaryx](https://diaryx.com). Provides an
iframe-based drawing canvas with pen, eraser, colors, sizes, undo/redo, and SVG
import.

## How It Works

The plugin declares an `EditorExtension` with `edit_mode: "Iframe"`. The host
renders a sandboxed iframe from the plugin's `get_component_html` response. The
iframe contains a self-contained drawing UI (canvas, tools, perfect-freehand
library) that communicates with the host via `postMessage`.

### Markdown Syntax

Drawings are stored as SVG attachments using image syntax with a `drawing:`
prefix:

```markdown
![drawing:My Sketch](_attachments/drawing-abc123.svg)
```

### PostMessage Protocol

| Direction | Type | Fields |
|-----------|------|--------|
| Host -> iframe | `init` | `theme`, `cssVars`, `data: { svg?, width, height, alt }` |
| Host -> iframe | `theme-update` | `theme`, `cssVars` |
| iframe -> host | `save` | `svg`, `width`, `height`, `alt` |
| iframe -> host | `cancel` | -- |

## Building

```bash
cargo build --target wasm32-wasip1 --release
```

The output WASM is at `target/wasm32-wasip1/release/diaryx_drawing_extism.wasm`.

## Installing

Copy the WASM file to your workspace:

```bash
cp target/wasm32-wasip1/release/diaryx_drawing_extism.wasm \
   <workspace>/.diaryx/plugins/diaryx.drawing/plugin.wasm
```

Or install via the Diaryx marketplace.

## Graceful Degradation

When the plugin is uninstalled, `![drawing:alt](path)` falls through to the
Image extension and renders as a static `<img>`. Reinstalling restores full
editing.
