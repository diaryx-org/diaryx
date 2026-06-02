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

Diaryx is still in its infancy. It aims to solve the audience filtering problem and serve as a personal CMS focused on sharing. As a project, we prioritize items that connect people to each other, especially in ways that would previously be impossible. To do this, we want to allow a monolithic collection of personal writing to be easily filterable and shareable.

## v1.5.0 - Commercial Viability

**One-click website publishing**

- [ ] (partially implemented) Alternative theming support for website publishing
- [x] Support rich interactive content in websites (like Ammon's demo)

**Other**

- [ ] Publish Obsidian plugin
- [ ] Remove first-party sync setup from the app in favor of folder-based workspaces and external sync tools
- [ ] Make plugin-import and plugin-pandoc work
- [ ] Finish TestFlight and publish to app store

## v1.6.0 - Real world usability

- Pages format—either use premium TipTap plugin, or make a bespoke one.
- Support for arbitrary rich/CSS styling via custom markdown syntax
- Quick notes! A key command similar to CMD+Space on Mac, or a similar "launchpad" shortcut, to record a note quickly. Needs thought for giving it a spot/attaching an audience quickly. Simplest method: pre-configured space for quick notes. Should Extism plugins be able to also be a Tauri plugin?
- AI agents sort notes for you?

## Would be cool (long term, low priority)

- Abstract away kinds of metadata—instead of YAML frontmatter, why not TOML, JSON, or other format? Why not endmatter or an arbitrary metadata code block?
- Revisit first-party sync only after the folder-based workspace model is solid; any future sync should build on local folders instead of a separate sidecar workspace location.
- Interactive functionality in published Diaryx
- Split or rename `diaryx_sync_server`/Cloudflare backend code so auth/account/publishing functionality remains while first-party sync code can be removed cleanly.
- In the marketplace:
  - UI sounds packs
  - UI/keyboard haptics packs
  - UI animations packs
  - Themes can adjust size/padding of UI components

**AI plugin**

- [ ] Pin down Diaryx philosophy with AI, brainstorm good AI integrations
- [ ] “AI iframe” plugin, similar to Claude’s interactive diagrams?

## Dreams (long term, high priority)

- Different UI chrome. The only webview needed is for the TipTap editor—everything else could theoretically use a different UI. Possibly test with a Tauri plugin to turn sidebars into SwiftUI, similar to the mobile toolbar plugin.
- Persistent identifier support for qualified files (ARK). Useful for academia/family history.
- Integration with FamilySearch API for family history records—import/export
- Per-audience workspace settings
- The backend takes plugins and handles only server compute primitives, auth, billing, and publishing. First-party sync should be removed or rebuilt later on top of folder-based workspaces.
