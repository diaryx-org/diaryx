---
title: Roadmap
description: The plan for future Diaryx features
author: adammharris
created: 2025-12-05T12:06:55-07:00
updated: 2025-12-15T13:07:12-07:00
audience:
  - public
part_of: README.md
---

# Roadmap

## v0.5.0

- Including attachments in publish and export
  New `attachments` property to declare attachments. Include in validate command below.
- Link validation
  A command to validate that all `part_of`/`contents` references are still valid, and that exported workspaces have no broken internal links. Maybe consolidate `diaryx normalize` into a validate command?

## Future considerations

## Better documentation

We have just one README file right now.

### Sync

Via git? When to fetch? Update on edit?

### Undo/redo

I would like `diaryx undo` and `diaryx redo` commands to undo/redo any command that was previously done, because it is easy to make mistakes.
