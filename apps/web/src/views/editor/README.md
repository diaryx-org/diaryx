---
title: Editor Views
description: Editor-related view components
part_of: '[README](/apps/web/src/views/README.md)'
attachments:
  - '[EditorContent.svelte](/apps/web/src/views/editor/EditorContent.svelte)'
  - '[EditorEmptyState.svelte](/apps/web/src/views/editor/EditorEmptyState.svelte)'
  - '[EditorFooter.svelte](/apps/web/src/views/editor/EditorFooter.svelte)'
exclude:
  - '*.lock'
---

# Editor Views

Editor-related view components.

## Files

| File | Purpose |
|------|---------|
| `EditorContent.svelte` | Main editor content area |
| `EditorEmptyState.svelte` | Empty state when no entry selected; for empty workspaces it shows a single `Initialize workspace` action that opens the setup flow |
| `EditorFooter.svelte` | Bottom action bar (audience, save state, plugin actions, command palette shortcut) |

## Tooltip Behavior

`EditorFooter.svelte` dismisses action-button tooltips on click and temporarily
suppresses the command-palette tooltip while the palette is open/closing.
It also uses controlled `onOpenChange` gating plus `ignoreNonKeyboardFocus` so
focus restoration and open-state races do not reopen it. After closing the
palette, the trigger is blurred and the tooltip remains blocked until the
pointer leaves the button once.
