---
title: Diaryx
description: README/repo for the Diaryx project
author: adammharris
version: v1.4.1
updated: 2026-04-04T12:02:12-06:00
contents:
- '[AGENTS](/AGENTS.md)'
- '[CONTRIBUTING](/CONTRIBUTING.md)'
- '[LICENSE](/LICENSE.md)'
- '[PHILOSOPHY](/PHILOSOPHY.md)'
- '[Privacy Policy](/privacy.md)'
- '[README](/apps/README.md)'
- '[README](/crates/README.md)'
- '[ROADMAP](/ROADMAP.md)'
- '[Scripts](/scripts/scripts.md)'
- '[TESTING](/TESTING.md)'
- '[Terms of Service](/terms.md)'
audience:
- public
- developers
- agents
exclude:
- '*.lock'
- '**/target'
- '**/*.rs'
- '**/*.ts'
- .git
- patches
- '**/dist'
- '**/*.toml'
- deploy
- flake.nix
- '**/*.sh'
- '**/*.pkg'
workspace_config:
  link_format: markdown_root
  audience_colors:
    agents: bg-indigo-500
    developers: bg-rose-500
    public: bg-emerald-500
  theme_preset: default
  theme_accent_hue: null
  disabled_plugins: []
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
    public_audiences:
    - public
    audience_states:
      public:
        state: public
        email_on_publish: false
    namespace_id: e108abba-9268-4fa6-a786-f3b94cfe357e
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
    workspace_id: 8fd558fa-1c3c-4260-a506-0f52866089e0
links:
- '[ROADMAP](/ROADMAP.md)'
- '[PHILOSOPHY](/PHILOSOPHY.md)'
attachments:
- '[icon-dark.png](/_attachments/icon-dark.png.md)'
- '[icon.png](/_attachments/icon.png.md)'
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
  <p align="center"><strong>Your writing. Worth sharing.</strong></p>
</div>


Diaryx is a writing space that lets you choose who sees what. So you can write honestly and share without fear.

- [Start writing](https://app.diaryx.org)

---

### The problem: you already censor yourself.

Most writing tools force a choice. Either your writing is private, locked away where nobody benefits from it, or it's public, exposed to everyone equally. There's no middle ground.

Diaryx gives you that middle ground. Write once, then decide which parts are for which people.

### How it works: label a file, a folder, or a paragraph.

Tag any section of your writing with an audience — just you, family, close friends, or everyone. When you share, each person sees only what's meant for them.[^1]

> Public:
>
>
>
> We're launching the redesigned homepage next week. The new layout prioritizes clarity and puts the product demo front and center.
>
>
>
> ~~Team only:~~
>
>
>
> ~~We're still waiting on final assets from the design contractor. If they slip past Wednesday, we'll need a fallback plan — Sara has a simplified version ready.~~
>
>
>
> ||Private note:||
>
>
>
>  ||I'm not confident the contractor will deliver on time. Starting to think we should have hired in-house for this. Lesson for next quarter.||

### We never hold your words hostage.

Diaryx is local-first. Your entries live on your device as plain Markdown files you can open in any text editor. Sync is optional, and you can leave any time with everything you wrote.

- **Works everywhere**: Web, iOS, Mac, Windows, Linux, even CLI!
- **Plain Markdown**: No proprietary format. Your files are yours, readable forever.
- **Share your way**: Publish to your own diaryx.org subdomain, email a filtered version, or share in person.
- **Extend with plugins**: Add drawing, audio recording, cloud sync, =={red}colored== =={brown}highlights==, and more through a growing plugin library.

---

**Use Diaryx for free at [app.diaryx.org](https://app.diaryx.org)!**

---

Read more:

- See the [Diaryx Roadmap](/ROADMAP.md)
- Read about the [Diaryx Philosophy](/PHILOSOPHY.md)

[^1]: (editor’s note: a better filtering demo is forthcoming)
