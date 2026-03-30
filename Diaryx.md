---
title: Diaryx
description: README/repo for the Diaryx project
author: adammharris
version: v1.4.0
updated: 2026-03-29T22:05:56-06:00
contents:
- '[AGENTS](/AGENTS.md)'
- '[CONTRIBUTING](/CONTRIBUTING.md)'
- '[LICENSE](/LICENSE.md)'
- '[PHILOSOPHY](/PHILOSOPHY.md)'
- '[README](/apps/README.md)'
- '[README](/crates/README.md)'
- '[ROADMAP](/ROADMAP.md)'
- '[Scripts](/scripts/scripts.md)'
- '[Terms of Service](/terms.md)'
- '[Privacy Policy](/privacy.md)'
audience:
- public
- developers
- agents
exclude:
- '*.lock'
- '**/target'
- .git
- patches
- '**/dist'
attachments:
- '[flake.nix](/flake.nix)'
- '[release.toml](/release.toml)'
- '[Cargo.toml](/Cargo.toml)'
- rust-analyzer.toml
workspace_config:
  link_format: markdown_root
  audience_colors:
    agents: bg-indigo-500
    developers: bg-rose-500
    public: bg-emerald-500
  theme_preset: default
  theme_accent_hue: null
plugins:
  diaryx.publish:
    permissions:
      read_files:
        include:
        - all
        exclude: []
      edit_files:
        include:
        - all
        exclude: []
      create_files:
        include:
        - all
        exclude: []
  diaryx.sync:
    permissions:
      read_files:
        include:
        - all
        exclude: []
      edit_files:
        include:
        - all
        exclude: []
      create_files:
        include:
        - all
        exclude: []
      delete_files:
        include:
        - all
        exclude: []
      http_requests:
        include:
        - all
        exclude: []
      plugin_storage:
        include:
        - all
        exclude: []
---
<div>
  <p align="center">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="apps/web/public/icon-dark.png">
      <source media="(prefers-color-scheme: light)" srcset="apps/web/public/icon.png">
      <img alt="Diaryx icon" src="apps/web/public/icon.png" width="128">
    </picture>
  </p>
  <h1 align="center">Diaryx</h1>
  <p align="center"><strong>Your journal. Worth sharing.</strong></p>
</div>


Diaryx is a writing format and CMS software designed to make two previously difficult things about writing very easy:

- **Filtering your writing by its intended audience**, so that you can publish *once* and reach *everyone*.
- **Using a portable, readable plain text format without compromising features**. So you can use **bold**, *italic*, or even =={red}colored== =={blue}highlights==, and still read your file with whatever software you want.

---

Read more:

- See the [Diaryx Roadmap](/ROADMAP.md)
- Read about the [Diaryx Philosophy](/PHILOSOPHY.md)
