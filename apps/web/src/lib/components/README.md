---
title: Components
description: Reusable Svelte components
part_of: '[README](/apps/web/src/lib/README.md)'
contents:
  - '[README](/apps/web/src/lib/components/ui/README.md)'
exclude:
  - '*.lock'
  - '**/*.ts'
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
| `BubbleMenuComponent.svelte` | TipTap bubble menu; stays measurable while hidden so Floating UI can position it on the first text selection, and the editor keeps it mounted while focus moves into the link insertion popover and while that popover stays open so desktop/Tauri link editing does not collapse on the first click. Local note link insert/remove actions also call backend `AddLink` / `RemoveLink` commands with the current markdown snapshot so `links` / `link_of` frontmatter stays aligned with editor actions without duplicate relations. |
| `filePickerEntries.ts` | Shared entry collection/filter helpers for picker popovers; de-duplicates canonical paths before keyed Svelte lists render, which avoids duplicate-key crashes when a workspace tree exposes the same file twice. |
| `FindBar.svelte` | In-editor find surface; search commands now fail closed with a one-time toast when the active TipTap document is already in an invalid state, so search UI does not cascade additional runtime errors. |
| `FloatingMenuComponent.svelte` | TipTap floating menu; the add-block trigger is delegated through `document` clicks and defers insertion until focus settles so Playwright and manual clicks both open the inline block picker on the first press. |
| `HighlightColorPicker.svelte` | Text highlight color picker |
| `HtmlBlockNodeView.svelte` | Raw HTML block preview/editor shell; preview sanitization now also rewrites `<picture><source srcset>` image candidates in addition to `<img src>` so local images render correctly in Tauri and web preview flows. |
| `MoreStylesPicker.svelte` | Overflow formatting menu (strikethrough, inline code, spoiler) |
| `PluginStatusItems.svelte` | Footer status-bar renderer for plugin-contributed items; fetches status once on mount and then refreshes on `SyncStatusChanged`/`SyncProgress` filesystem events instead of polling, eliminating the previous 3-second interval that flooded the IPC channel. |
| `PluginIframe.svelte` | Sandboxed plugin iframe host with direct `get_component_html` loading when the runtime exposes it, command bridge fallback for older/plugin-command-only guests, managed-context injection for `diaryx.ai`, and response-shape normalization for plugin HTML payloads. |
| `PluginSidebarPanel.svelte` | Sidebar wrapper for plugin component refs (Builtin/Declarative/Iframe). |
| `SpotlightOverlay.svelte` | Marketplace onboarding spotlight overlay; geometry, tooltip placement, and swipe interpretation live in `spotlightOverlay.ts` so the component shell stays thin and the interaction rules can be unit-tested directly. |
| `UpgradeBanner.svelte` | Shared upsell surface for Plus-gated features (used for managed AI gating). |
| `VisibilityPicker.svelte` | Audience picker used by the bubble menu; tracks TipTap selection/transaction updates so existing inline and block audiences render as selected, edits enclosing `:::vis` blocks when the selection is already inside one, otherwise wraps clean whole-block selections in a block directive and falls back to inline `:vis[...]` for partial-block text. |

## UI Components

See `ui/` for shadcn-svelte based UI primitives.
