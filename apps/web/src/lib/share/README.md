---
title: Share
description: Share session components
part_of: '[README](/apps/web/src/lib/README.md)'
attachments:
  - '[ShareTab.svelte](/apps/web/src/lib/share/ShareTab.svelte)'
  - '[LiveCollaborationPanel.svelte](/apps/web/src/lib/share/LiveCollaborationPanel.svelte)'
  - '[PublishingPanel.svelte](/apps/web/src/lib/share/PublishingPanel.svelte)'
exclude:
  - '*.lock'
---

# Share

Share components for real-time collaboration and static-site publishing. These live in the **left sidebar** Share tab (workspace-level concern).

## Audience Selection

Both `LiveCollaborationPanel` and `PublishingPanel` read the audience from the left sidebar's `AudienceFilter` via `templateContextStore.previewAudience` instead of maintaining their own audience dropdowns. This centralises audience selection to one place.

## Files

| File | Purpose |
|------|---------|
| `ShareTab.svelte` | Share top-level sub-tab shell (`Live Collaboration` and `Publishing`) |
| `LiveCollaborationPanel.svelte` | Share-session create/join/host UI; uses `templateContextStore.previewAudience` for audience filtering |
| `PublishingPanel.svelte` | Site publishing setup, publish-now actions, and access-token CRUD; uses `templateContextStore.previewAudience` for token audience |
