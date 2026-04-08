---
title: "Highlight"
description: "Colored highlight syntax with ==text== and =={color}text=="
id: "diaryx.highlight"
version: "0.1.0"
author: "Diaryx Team"
license: "PolyForm Shield 1.0.0"
repository: "https://github.com/diaryx-org/diaryx"
categories: ["editor", "formatting"]
tags: ["highlight", "markdown", "editor", "color"]
capabilities: ["editor_extension"]
artifact:
  url: ""
  sha256: ""
  size: 0
  published_at: ""
ui:
  - slot: EditorExtension
    id: coloredHighlight
    label: "Highlight"
---

# diaryx_highlight_extism

Extism WASM guest plugin that provides colored highlight syntax.

## Overview

This plugin contributes an `InlineMark` editor extension with attribute support via the plugin manifest system. The host generates a TipTap `Mark` extension from the manifest declaration with `color` attribute handling.

## Features

- `==text==` input rule — typing `==highlighted==` auto-converts to a yellow highlight mark
- `=={color}text==` input rule — typing `=={red}important==` converts to a red highlight
- `==text==` and `=={color}text==` paste rules — pasted syntax converts automatically
- `Mod-Shift-H` keyboard shortcut to toggle yellow highlight on selection
- 10 predefined colors: red, orange, yellow (default), green, cyan, blue, violet, pink, brown, grey
- CSS injected from manifest (light and dark mode)
- BubbleMenu color picker integration

## Plugin ID

`diaryx.highlight`

## Build

```bash
cargo build -p diaryx_highlight_extism --target wasm32-wasip1 --release
```
