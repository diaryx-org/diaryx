---
title: lib
description: Shared libraries and components
part_of: "[README](/apps/web/src/README.md)"
contents:
  - "[README](/apps/web/src/lib/auth/README.md)"
  - "[README](/apps/web/src/lib/backend/README.md)"
  - "[README](/apps/web/src/lib/components/README.md)"
  - "[README](/apps/web/src/lib/device/README.md)"
  - "[README](/apps/web/src/lib/extensions/README.md)"
  - "[README](/apps/web/src/lib/history/README.md)"
  - "[README](/apps/web/src/lib/hooks/README.md)"
  - "[README](/apps/web/src/lib/marketplace/README.md)"
  - "[README](/apps/web/src/lib/namespace/README.md)"
  - "[README](/apps/web/src/lib/publish/README.md)"
  - "[README](/apps/web/src/lib/settings/README.md)"
  - "[README](/apps/web/src/lib/share/README.md)"
  - "[README](/apps/web/src/lib/sync/README.md)"
  - "[README](/apps/web/src/lib/storage/README.md)"
  - "[README](/apps/web/src/lib/stores/README.md)"
  - "[README](/apps/web/src/lib/wasm/README.md)"
exclude:
  - "*.lock"
---

# Lib

Shared libraries, components, and utilities for the web application.

## Structure

| Directory      | Purpose                                                                |
| -------------- | ---------------------------------------------------------------------- |
| `auth/`        | Authentication services and stores                                     |
| `backend/`     | Backend abstraction layer (WASM/Tauri)                                 |
| `components/`  | Reusable Svelte components                                             |
| `device/`      | Device identification                                                  |
| `extensions/`  | TipTap editor extensions                                               |
| `history/`     | Version history components                                             |
| `hooks/`       | Svelte hooks                                                           |
| `marketplace/` | Marketplace asset registry/apply logic (themes, typographies, bundles) |
| `namespace/`   | Namespace management services and host-side UI components               |
| `publish/`     | Publishing and export components                                       |
| `settings/`    | Settings panel components                                              |
| `share/`       | Share session components                                               |
| `sync/`        | Sync plugin host-side adapters/services                                |
| `storage/`     | Storage abstraction                                                    |
| `stores/`      | Svelte stores                                                          |
| `wasm/`        | Built WASM module                                                      |

## Validation

Workspace naming, URL normalization, and publishing slug validation live in
`diaryx_core::utils::naming` (Rust) and are exposed to the frontend via
Commands (`ValidateWorkspaceName`, `ValidatePublishingSlug`,
`NormalizeServerUrl`). The typed wrappers are in
`backend/api.ts`. Frontend components call these instead of duplicating
validation logic locally.

Workspace-root normalization for Tauri now also lives in
`workspace/rootPath.ts`. Callers that need the workspace directory or the
actual root index file should use those helpers or `api.resolveWorkspaceRootIndexPath(...)`
instead of hardcoding `README.md` / `index.md` filename assumptions.

Workspace switches now also refresh `pluginStore` against the newly-created
backend in `workspace/switchWorkspace.ts`, so marketplace/settings/sidebar
plugin surfaces do not momentarily show manifests from the previous workspace.

Rapid entry navigation now also uses `latestOnlyRunner.ts` to coalesce repeated
open-entry requests to the most recent target instead of letting every
intermediate click hit the backend.

## Onboarding Workspace Setup

Workspace creation and restore now flow through the welcome/onboarding
experience (`views/WelcomeScreen.svelte` + `controllers/onboardingController.ts`)
instead of a standalone add-workspace dialog.

The onboarding flow supports:

- local-first workspace creation from curated starter bundles
- provider-backed workspace creation after bundle selection
- remote workspace restore through provider-owned metadata
- Apple/Tauri built-in provider options such as iCloud Drive

Provider-backed restore still uses staged initialization so users see visible
forward motion during link/bootstrap flows even when the underlying provider
does not emit granular file-level progress.

Browser sync now loads the Extism sync plugin from
`/plugins/diaryx_sync.wasm` with a runtime compatibility check. If the file is
an older wasm-bindgen-flavored artifact, loading fails fast with a rebuild
instruction instead of surfacing a low-level Extism import resolution error.

Extism sync guest calls are serialized in
`plugins/extismBrowserLoader.ts` so browser transport callbacks and host
events cannot re-enter the guest concurrently and trip internal `RefCell`
borrows.

Browser host-side sync wiring lives in `plugins/extismBrowserLoader.ts` and
`sync/providerPluginCommands.ts` / `sync/workspaceProviderService.ts` so the
web app remains a plugin host, provider commands prefer plugin-owned
runtime/config state, and sync logic stays in the external sync plugin.

`App.svelte` now returns first-run and missing-workspace recovery flows to the
welcome screen instead of reopening a separate add-workspace modal.

## Command Palette Dialog Sequencing

`CommandPalette.svelte` closes the palette and awaits a Svelte `tick()` before
running the selected command action. This prevents overlapping Radix dialogs
when a command opens another modal (for example, `New Entry`).

Favorite-command selection, command grouping, and mobile dismiss-threshold
rules now live in `commandPalette.ts` so the palette shell can stay focused on
rendering and gesture wiring while the data-shaping logic is unit-tested
directly.

`NewEntryModal.svelte` also guards its parent-picker root expansion effect so it
does not continuously rewrite `pickerExpanded`. This avoids reactive update-loop
errors that can leave overlapping dialogs/focus traps on screen.

`MoveEntryDialog.svelte` keeps its tree search, disabled-path collection, and
same-parent reorder planning in `moveEntryDialog.ts` so the modal shell stays
focused on rendering while the move rules are unit-tested directly.

The command palette can now be plugin-owned via a `UiContribution::CommandPalette`
surface contribution. When no plugin owns the surface, the built-in fallback is
limited to backup/import actions.

## Editor performance

`Editor.svelte` now preserves unsaved content across internal TipTap rebuilds
while avoiding full-document markdown serialization on every keystroke. Large
documents are serialized on demand for save/export/sync checks instead, which
keeps typing latency down in long notes.

Editor link clicks are intercepted at the DOM-event layer before the webview
can navigate. This keeps local note links inside the app on Tauri/dev builds
instead of falling through to a `localhost` browser open, while external
`http(s)` links still route out through the normal browser path.

Editor menu visibility rules that depend on focus handoff and explicit link
popover state now live in `editorMenuVisibility.ts` so BubbleMenu regressions
can be tested directly without relying on full TipTap child-component wiring.
The visibility picker is cleared when the BubbleMenu hides instead of forcing
the menu to stay visible after the editor selection disappears.
BubbleMenu placement is appended to `document.body`, uses fixed positioning,
and listens to the editor scroll parent so the first text selection in a
scrolled entry does not mix editor-content and viewport coordinates.
Attachment note refs such as `_attachments/widget.html.md` now also use the
preserved original filename (`widget.html`) as a media-type hint in
`Editor.svelte`, so uploaded HTML/video/audio attachments that are stored as
note-backed links still render through the right node view instead of falling
back to broken image handling.
Inline HTML attachment previews now also default to a taller embedded iframe
viewport (`420px`, with an explicit node height still taking precedence) so
full-page documents do not appear as an empty strip when their content starts
below the top fold.
Those HTML attachment previews now also honor the image node's stored
`width` / `height` attrs, so the existing media resize menu can adjust iframe
embeds and persist the size back into markdown the same way image sizing does.
When no explicit width is stored, the HTML embed wrapper now expands to the
full editor column instead of shrink-wrapping to the iframe's intrinsic width.
When no explicit height is stored, the host still accepts preview-size messages
from the iframe, but the embed now keeps normal iframe scrolling enabled so
interactive attachments can stay usable even before the preview bridge reports
their full content height.
Those HTML attachment iframes now also receive the same host-driven `init` /
`theme-update` message shape used by plugin iframes, including current Diaryx
CSS variables, so standalone HTML demos can adapt to the active workspace
theme when embedded in the editor.
Dragged attachment/image nodes now also normalize any accidental workspace
filesystem prefix back to a workspace-relative `_attachments/...` path during
drop insertion and markdown serialization, preventing drag-reorder flows from
persisting local absolute paths into the note body.
If an older in-memory editor state still contains a root-level inline image
node from the previous buggy insert path, `Editor.svelte` now normalizes that
shape back into a paragraph-wrapped image during invalid-content recovery
before rebuilding the editor instance.
On iOS Tauri, `Editor.svelte` also exposes the native-toolbar bridge for
audience visibility state and mutations so the Swift toolbar can offer the same
inline/block audience picker behavior as the web BubbleMenu.
`Editor.test.ts` now also includes focused coverage for the prop-driven
content-sync effect and the template-context refresh dispatch effect,
including the guarded error path when TipTap `setContent(...)` throws during
external entry refreshes and the guarded `templateContextChanged` metadata
dispatch path, which now logs and shows a one-time toast instead of crashing
when invalid editor content makes decoration refresh fail.

## Sidebar Layout

- Left sidebar: built-in `Files` tab plus plugin-contributed tabs.
- The left sidebar control rows keep workspace-local actions together: workspace selector, validation, and marketplace in the top row; sign-in/account, settings, and collapse in the footer row.
- Right sidebar: built-in `Properties` tab plus plugin-contributed tabs.
- Marketplace: opened from a dedicated modal surface
  (`MarketplaceDialog.svelte`): desktop `Dialog`, mobile `Drawer`.
  The desktop dialog now clamps to the available viewport height and keeps the
  marketplace body scrollable so small Tauri windows do not clip the surface.
- Marketplace tabs now own appearance customization (`Themes`, `Typography`, `Bundles`) in addition to plugin browsing, with curated + local registries for themes and typography presets.
- `mobileSwipe.ts` centralizes app-shell gesture gating so sidebar-open swipes are edge-triggered and modal/drawer or text-selection gestures do not leak through to the shell.
- Plugin sidebars are host-rendered with `components/PluginSidebarPanel.svelte`.
- Status-bar plugin items are host-rendered with `components/PluginStatusItems.svelte`, which only displays plugin-reported status and leaves plugin-specific actions to the plugin itself.
- `RightSidebar.svelte` resets collapse-button tooltip state when collapsing to
  prevent stale tooltip visibility when reopening the panel.
- `RightSidebar.svelte` commits title edits through the blur path only; pressing
  Enter now blurs the field without firing a second duplicate title-change
  request first.
- `RightSidebar.svelte` keeps `datetime-local` frontmatter editors on local wall
  time and writes RFC3339 values with the current local offset instead of
  round-tripping through UTC, so `updated` and similar fields display the same
  local time they store.
- `RightSidebar.svelte` now probes attachment local availability through
  attachment path resolution + file existence checks instead of reading full
  attachment bytes, keeping the properties panel responsive in media-heavy
  entries.
- Sidebar attachment preview clicks now reuse the shared attachment
  resolver/blob cache for previewable media instead of calling
  `GetAttachmentData` and creating a fresh preview blob on every open, which
  makes repeat previews effectively instant and keeps the first open on the
  binary read path.
- `RightSidebar.svelte` now also treats the singular attachment-note
  `attachment` property as an attachment target instead of a generic note link:
  previewable media opens in the preview dialog and non-previewable files
  download through the backend bytes path.
- The attachment preview dialog now opens images with a cached thumbnail
  immediately when one already exists, then swaps to the full media when ready.
  Videos and audio use the same preview surface. On Tauri, the full-preview
  step prefers a native `asset:` URL for local verified media files and falls
  back to the blob resolver when native loading is unavailable.
- Attachment uploads and picker inserts now classify files as
  image/video/audio/file so previewable media insert directly into the editor
  instead of only images taking the fast embed path.
- Attachment/media markdown serialization now wraps destinations containing
  whitespace in angle brackets, so uploaded videos and other attachments with
  spaced filenames persist as valid CommonMark embeds instead of degrading to
  plain text on reload.
- Image nodes support Obsidian-style inline resize via `![alt|WIDTHxHEIGHT](src)`.
  The image context menu includes a **Resize** submenu with percentage presets
  (25%, 50%, 75%, 100%) and a custom size prompt. Dimensions are stored as
  `width`/`height` node attributes and round-trip through markdown.
- `LeftSidebar.svelte` dismisses the Settings-button tooltip on click so it
  does not remain visible after opening Settings, temporarily suppresses the
  tooltip while Settings is open/closing, and uses controlled tooltip open
  gating plus `ignoreNonKeyboardFocus` to prevent reopen on dialog focus
  restore and transition races. Settings/Marketplace footer tooltips also
  blur their triggers on dialog close and require one pointer-leave before
  opening again.
- `LeftSidebar.svelte` supports desktop multi-select (`Cmd`/`Ctrl` toggle,
  `Shift` range) with a small bulk-action bar. Sidebar delete requests are
  expanded to selected descendants and executed child-first so index entries
  with non-empty `contents` can be removed from the UI without manual
  leaf-by-leaf deletion.
- On Tauri desktop, `LeftSidebar.svelte` also exposes a context-menu action to
  reveal the selected entry in Finder/Explorer/the system file manager via the
  backend's opener-backed `revealInFileManager()` helper. The action is hidden
  on mobile because Tauri does not support reveal flows there.
- `WorkspaceSelector.svelte` can now attach an existing local Tauri folder to a
  listed remote workspace. The remote picker keeps `download` for
  remote-wins restore and adds `link` for local-folder attach, with explicit
  `Already in sync` vs `Upload local` policies before the workspace is linked.
- `windowDrag.ts` centralizes Tauri desktop window dragging for shared chrome
  surfaces. Sidebar/header/footer drag handlers use it and automatically skip
  interactive descendants such as buttons, links, inputs, and elements marked
  with `data-window-drag-exclude`.

## Plugin-Contributed Surfaces

Sync/share/history/publish behavior is plugin-contributed.
The web host keeps only generic infrastructure:

- Browser plugin runtime + typed command routing
- Generic iframe/component rendering
- Workspace tree/editor refresh from backend filesystem events

The host does not keep a web-specific CRDT bridge module.

Marketplace installs and removals now refresh plugin manifests and TipTap
editor extensions in-place on both browser and Tauri runtimes, so editor
features such as spoiler/math activate without a manual page reload. When a
plugin is removed mid-session, Diaryx keeps a preserve-only fallback extension
alive until the next reload so custom markdown syntax is not stripped from
open notes.

Left sidebar tree context menus can also be plugin-owned via
`UiContribution::ContextMenu { target: LeftSidebarTree, ... }`. When no plugin
owns this surface, the built-in fallback context menu is limited to
backup/import actions.

## Sidebar Tree Performance

`LeftSidebar.svelte` pre-groups validation errors by path for O(1) row lookups
instead of scanning the full error list per rendered node. This keeps folder
expand/collapse interactions responsive in larger workspaces.

The tree renderer also deduplicates children by `path` before keyed rendering,
so duplicate references from upstream data do not crash Svelte keyed `each`
blocks.

During file switches, `App.svelte` passes a pending `activeEntryPath` into
`LeftSidebar.svelte` so the newly clicked row highlights immediately even when
the backend is still resolving the next entry (for example, while attachment
loads are being canceled).
