---
title: ROADMAP
description: The plan for future Diaryx features
author: adammharris
created: 2025-12-05T12:06:55-07:00
updated: 2026-06-02T00:00:00-06:00
part_of: '[Diaryx](/Diaryx.md)'
link_of:
- '[Diaryx](/Diaryx.md)'
attachments: []
link: '[ROADMAP](/ROADMAP.md)'
---
# ROADMAP

Immediate goal


## Unorganized ideas

- Pages format—either use premium TipTap plugin, or make a bespoke one.
- Support for arbitrary rich/CSS styling via custom markdown syntax
- Quick notes! A key command similar to CMD+Space on Mac, or a similar "launchpad" shortcut, to record a note quickly. Needs thought for giving it a spot/attaching an audience quickly. Simplest method: pre-configured space for quick notes. Should Extism plugins be able to also be a Tauri plugin?

## Would be cool (long term, low priority)

- Abstract away kinds of metadata—instead of YAML frontmatter, why not TOML, JSON, or other format? Why not endmatter or an arbitrary metadata code block?
- Revisit first-party sync only after the folder-based workspace model is solid; any future sync should build on local folders instead of a separate sidecar workspace location.
- Interactive functionality in published Diaryx
- [x] Cleaned up first-party sync code from `diaryx_sync_server`/Cloudflare backend while keeping auth/account/publishing functionality.

**AI plugin**

- [ ] Pin down Diaryx philosophy with AI, brainstorm good AI integrations
- [ ] “AI iframe” plugin, similar to Claude’s interactive diagrams?

## Dreams (long term, high priority)

- Different UI chrome. The only webview needed is for the TipTap editor—everything else could theoretically use a different UI. Possibly test with a Tauri plugin to turn sidebars into SwiftUI, similar to the mobile toolbar plugin.
- Persistent identifier support for qualified files (ARK). Useful for academia/family history.
- Integration with FamilySearch API for family history records—import/export
- Per-audience workspace settings
- [x] First-party sync has been removed completely, so the backend and plugins now focus only on local folder workspaces, auth, billing, and publishing.
