---
title: Share
description: Share session components
part_of: "[README](/apps/web/src/lib/README.md)"
attachments:
  - "[ShareTab.svelte](/apps/web/src/lib/share/ShareTab.svelte)"
  - "[LiveCollaborationPanel.svelte](/apps/web/src/lib/share/LiveCollaborationPanel.svelte)"
  - "[PublishingPanel.svelte](/apps/web/src/lib/share/PublishingPanel.svelte)"
exclude:
  - "*.lock"
---

# Share

Share components for real-time collaboration. These live in the **left sidebar** Share tab (workspace-level concern).

## Audience Selection

`LiveCollaborationPanel` reads the audience from the left sidebar's `AudienceFilter` via `templateContextStore.previewAudience` instead of maintaining its own audience dropdown. This centralises audience selection to one place.

Publishing UI moved to the left sidebar **Publish** tab (`publish/PublishTab.svelte`).

## Files

| File                            | Purpose                                                                                                                                                           |
| ------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ShareTab.svelte`               | Share top-level panel shell (`Live Collaboration`)                                                                                                                |
| `LiveCollaborationPanel.svelte` | Share-session create/join/host UI; uses `templateContextStore.previewAudience` for audience filtering                                                             |
| `PublishingPanel.svelte`        | Site publishing setup, publish-now actions, and access-token CRUD; rendered by the Publish tab and uses `templateContextStore.previewAudience` for token audience |
