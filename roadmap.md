---
title: Roadmap
description: The plan for future Diaryx features
author: adammharris
created: 2025-12-05T12:06:55-07:00
updated: 2025-12-07T01:20:00-07:00
audience:
    - public
part_of: README.md
---

# Roadmap

## Road to v0.3.0

Here are features I want to implement before a full v0.3.0 release.

### ✅ Filtering/Export feature (implemented)

The `diaryx export` command filters and exports workspace files by audience. Here's how it works:

- Any file may have an `audience` property, which is an array of string values. Each value represents a group that may see this file.
- If a file has no `audience` property, it inherits from its parent (via `part_of`). If the root has no audience, files default to private.
- The special value `private` in the audience array means the file is never exported, regardless of other values.
- Children cannot expand access beyond their parent — they can only narrow it.
- When exporting, the `contents` array is automatically updated to exclude filtered children.
- The `audience` property is removed from exported files by default (use `--keep-audience` to preserve it).

**Usage:**

```bash
diaryx export --audience family ~/path/to/export
diaryx export --audience family ~/export --verbose --dry-run
diaryx export --audience work ~/export --force --keep-audience
```

### Undo/redo

I would like `diaryx undo` and `diaryx redo` commands to undo/redo any command that was previously done, because it is easy to make mistakes.

## Future considerations

### Attachments/Images

When exporting, referenced images and attachments (e.g., `![photo](./images/vacation.jpg)`) are not currently copied. This could be a future enhancement.

### Link validation

A command to validate that all `part_of`/`contents` references are still valid, and that exported workspaces have no broken internal links.
