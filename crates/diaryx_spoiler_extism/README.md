---
title: diaryx_spoiler_extism
description: Spoiler mark Extism guest plugin for Diaryx
part_of: '[README](/crates/README.md)'
---

# diaryx_spoiler_extism

Extism WASM guest plugin that provides Discord-style `||spoiler||` syntax.

## Overview

This plugin contributes an `InlineMark` editor extension via the plugin manifest system. The host (web/CLI) generates a TipTap `Mark` extension from the manifest declaration — no render export is needed since marks wrap inline content directly.

## Features

- `||text||` input rule — typing `||hidden||` auto-converts to a spoiler mark
- `||text||` paste rule — pasted spoiler syntax converts automatically
- `Mod-Shift-S` keyboard shortcut to toggle spoiler on selection
- Click-to-reveal behavior (hidden → revealed → hidden)
- Click-outside resets all revealed spoilers
- CSS injected from manifest (no hardcoded styles in host)
- Insert command in MoreStylesPicker dropdown

## Plugin ID

`diaryx.spoiler`

## Build

```bash
cargo build -p diaryx_spoiler_extism --target wasm32-wasip1 --release
```
