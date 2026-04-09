---
title: Hooks
description: Svelte hooks
part_of: '[README](/apps/web/src/lib/README.md)'
exclude:
  - '*.lock'
  - '**/*.ts'
---

# Hooks

Svelte hooks for reusable reactive logic.

## Files

| File | Purpose |
|------|---------|
| `useContextMenu.svelte.ts` | Context menu management |
| `useMobile.svelte.ts` | Mobile device detection |
| `useAttachmentPicker.svelte.ts` | Shared attachment picker state, upload handling, and selection payload shaping for note-backed attachment refs (including preserving the original uploaded filename so host insert handlers can still recognize HTML/media attachments after the backend wraps them in `.md` attachment notes). |
