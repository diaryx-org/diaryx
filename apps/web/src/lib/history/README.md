---
title: History
description: Version history components
part_of: '[README](/apps/web/src/lib/README.md)'
exclude:
  - '*.lock'
  - '**/*.ts'
---

# History

Version history components for time travel and diff viewing.

## Files

| File | Purpose |
|------|---------|
| `HistoryEntry.svelte` | Single history entry display |
| `HistoryPanel.svelte` | History panel container |
| `VersionDiff.svelte` | Version diff viewer |
| `GitHistoryPanel.svelte` | Git snapshot history panel (commit, restore) |

Plugin-owned history surfaces can also contribute RightSidebar tabs through the
generic sidebar extension points, as the GitHub sync plugin does for remote
commit history.
