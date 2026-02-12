---
title: Components
description: Reusable Svelte components
part_of: '[README](/apps/web/src/lib/README.md)'
contents:
  - '[README](/apps/web/src/lib/components/ui/README.md)'
attachments:
  - '[AttachmentPicker.svelte](/apps/web/src/lib/components/AttachmentPicker.svelte)'
  - '[AttachmentPickerNodeView.svelte](/apps/web/src/lib/components/AttachmentPickerNodeView.svelte)'
  - '[BubbleMenuComponent.svelte](/apps/web/src/lib/components/BubbleMenuComponent.svelte)'
  - '[FloatingMenuComponent.svelte](/apps/web/src/lib/components/FloatingMenuComponent.svelte)'
  - '[HighlightColorPicker.svelte](/apps/web/src/lib/components/HighlightColorPicker.svelte)'
  - '[TemplateEditorDialog.svelte](/apps/web/src/lib/components/TemplateEditorDialog.svelte)'
exclude:
  - '*.lock'
  - '*.test.ts'
---

# Components

Reusable Svelte components for the editor and UI.

## Files

| File | Purpose |
|------|---------|
| `AttachmentPicker.svelte` | Attachment selection dialog (normalizes ancestor/upload refs via backend link parser before thumbnail reads and insertion) |
| `AttachmentPickerNodeView.svelte` | Inline attachment node (same link-parser normalization path as dialog picker) |
| `BubbleMenuComponent.svelte` | TipTap bubble menu |
| `FloatingMenuComponent.svelte` | TipTap floating menu |
| `HighlightColorPicker.svelte` | Text highlight color picker |
| `TemplateEditorDialog.svelte` | Template editing dialog |

## UI Components

See `ui/` for shadcn-svelte based UI primitives.
