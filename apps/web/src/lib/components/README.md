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
  - '[DrawingBlockNodeView.svelte](/apps/web/src/lib/components/DrawingBlockNodeView.svelte)'
  - '[DrawingCanvas.svelte](/apps/web/src/lib/components/DrawingCanvas.svelte)'
  - '[FloatingMenuComponent.svelte](/apps/web/src/lib/components/FloatingMenuComponent.svelte)'
  - '[HighlightColorPicker.svelte](/apps/web/src/lib/components/HighlightColorPicker.svelte)'
  - '[MoreStylesPicker.svelte](/apps/web/src/lib/components/MoreStylesPicker.svelte)'
  - '[PluginStatusItems.svelte](/apps/web/src/lib/components/PluginStatusItems.svelte)'
  - '[PluginIframe.svelte](/apps/web/src/lib/components/PluginIframe.svelte)'
  - '[PluginSidebarPanel.svelte](/apps/web/src/lib/components/PluginSidebarPanel.svelte)'
  - '[UpgradeBanner.svelte](/apps/web/src/lib/components/UpgradeBanner.svelte)'
exclude:
  - '*.lock'
  - '*.test.ts'
---

# Components

Reusable Svelte components for the editor and UI.

## Files

| File | Purpose |
|------|---------|
| `AttachmentPicker.svelte` | Attachment selection dialog (normalizes ancestor/upload refs via backend link parser, classifies attachments by media kind, and lazy-loads cached thumbnails only as image tiles approach the viewport). |
| `AttachmentPickerNodeView.svelte` | Inline attachment node (same normalized attachment lookup path and media-kind classification as the dialog picker, with the same viewport-gated thumbnail loading). |
| `BlockPickerNodeView.svelte` | Inline block picker node view; delays outside-click listeners until the opening interaction finishes so the picker stays open on the first click. |
| `BubbleMenuComponent.svelte` | TipTap bubble menu |
| `DrawingBlockNodeView.svelte` | Drawing block node view (view mode + edit overlay) |
| `DrawingCanvas.svelte` | Freehand drawing editor with perfect-freehand |
| `FloatingMenuComponent.svelte` | TipTap floating menu; the add-block trigger is delegated through `document` clicks and defers insertion until focus settles so Playwright and manual clicks both open the inline block picker on the first press. |
| `HighlightColorPicker.svelte` | Text highlight color picker |
| `MoreStylesPicker.svelte` | Overflow formatting menu (strikethrough, inline code, spoiler) |
| `PluginStatusItems.svelte` | Footer status-bar renderer for plugin-contributed items; the host displays plugin-reported status but does not hardcode sync-specific actions. |
| `PluginIframe.svelte` | Sandboxed plugin iframe host with direct `get_component_html` loading when the runtime exposes it, command bridge fallback for older/plugin-command-only guests, managed-context injection for `diaryx.ai`, and response-shape normalization for plugin HTML payloads. |
| `PluginSidebarPanel.svelte` | Sidebar wrapper for plugin component refs (Builtin/Declarative/Iframe). |
| `UpgradeBanner.svelte` | Shared upsell surface for Plus-gated features (used for managed AI gating). |

## UI Components

See `ui/` for shadcn-svelte based UI primitives.
