---
title: ROADMAP
description: The plan for future Diaryx features
author: adammharris
created: 2025-12-05T12:06:55-07:00
updated: 2026-01-29T22:42:14-07:00
audience:
  - public
part_of: "[README](/README.md)"
---

# ROADMAP

## Todo (short-term, medium priority)

- Update to VITE 8
- Upload to iOS/Mac App Store
- Also bundle for stores for various platforms
- Publish updated diaryx.org site
  - Include this ROADMAP in the site

## v1.4.0 - Bundles and Release

- Fleshing out plugin functionality
- Better onboarding
  - Bundles can include "spotlight" onboarding
  - Log in from onboarding screen
  - Browse bundles from onboarding screen
  - Set up terms and conditions on diaryx.org; update links in purchase section
- Release to app store!
- Stretch goal: make sync plugin work seamlessly

## v1.5.0 - Commercial Viability

- AI plugin: Integrate with agents, pin down Diaryx philosophy
- Publish plugin: Configure identity providers (email, phone#, maybe oauth?), perhaps other methods of publishing besides a website (email, others?)
- Test Obsidian plugin robustness
- Make sync plugin work if not already
- Flesh out import and export—deliver on the "import/export anywhere" promise.

## v1.6.0 - Real world usability

- Pages format---either use premium TipTap plugin, or make a bespoke one.
- Support for arbitrary rich/CSS styling via custom markdown syntax
- Quick notes! A key command similar to CMD+Space on Mac, or a similar "launchpad" shortcut, to record a note quickly. Needs thought for giving it a spot/attaching an audience quickly. Simplest method: pre-configured space for quick notes. Should Extism plugins be able to also be a Tauri plugin?
- AI agents sort notes for you?

## Would be cool (long term, low priority)

- Abstract away kinds of metadata—instead of YAML frontmatter, why not TOML, JSON, or other format? Why not endmatter or an arbitrary metadata code block?
- Integrate diaryx_sync crate. How much can go into core, how much in the plugin? Currently shared by sync plugin, share plugin, and diaryx_sync_server
- Interactive functionality in published Diaryx
- More servers/load balancing for `diaryx_sync_server` (microservices?)
- In the marketplace:
  - UI sounds packs
  - UI/keyboard haptics packs
  - UI animations packs
  - Themes can adjust size/padding of UI components

## Dreams (long term, high priority)

- Different UI chrome. The only webview needed is for the TipTap editor—everything else could theoretically use a different UI. Possibly test with a Tauri plugin to turn sidebars into SwiftUI, similar to the mobile toolbar plugin.
- Persistent identifier support for qualified files (ARK). Useful for academia/family history.
- Integration with FamilySearch API for family history records—import/export
