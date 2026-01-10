---
title: Roadmap
description: The plan for future Diaryx features
author: adammharris
created: 2025-12-05T12:06:55-07:00
updated: 2026-01-098T22:42:14-07:00
audience:
  - public
part_of: README.md
---

# Roadmap

## v0.8.0

### Template support for web frontend

### Improved Sync/Backup

### Cross-platform import

Import from Obsidian (add all part_of/contents properties + index files)

## Other considerations

### Refactor for Backend interface in diaryx_wasm

How much to integrate into diaryx_core?

### Better documentation

We have just one README file right now.

### Undo/redo

I would like `diaryx undo` and `diaryx redo` commands to undo/redo any command that was previously done, because it is easy to make mistakes.

### Encryption

Ideally hot-swappable similar to backup backends. Maybe Cryptomator?

### Math/diagrams

TipTap has an extension for LaTeX, but I would like to support Mermaid diagrams and Typst syntax as well. Maybe there is a way to swap parsers and return an image?
