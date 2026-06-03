---
title: Diaryx
description: README/repo for the Diaryx project
author: adammharris
version: v1.5.1
updated: 2026-06-02T00:00:00-06:00
contents:
- '[AGENTS](/AGENTS.md)'
- '[CONTRIBUTING](/CONTRIBUTING.md)'
- '[LICENSE](/LICENSE.md)'
- '[PHILOSOPHY](/PHILOSOPHY.md)'
- '[Privacy Policy](/privacy.md)'
- '[README](/apps/README.md)'
- '[README](/crates/README.md)'
- '[ROADMAP](/ROADMAP.md)'
- '[TESTING](/TESTING.md)'
- '[Terms of Service](/terms.md)'
audience:
- public
- developers
- agents
exclude:
- '**/{target,dist}'
- '**/*.!(md|png|bin|json|wasm)'
workspace_config:
  link_format: markdown_root
  audience_colors:
    agents: bg-indigo-500
    developers: bg-rose-500
    public: bg-emerald-500
  theme_preset: default
  theme_accent_hue: null
  disabled_plugins: []
  audiences:
  - gates: []
    name: public
    share_actions:
    - kind: copy_link
      label: For group chat
  audiences_migrated: true
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
- '[Audience Filtering Demo](/_attachments/audience-filter-demo.html.md)'
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

## How it works: label a file, a folder, or a paragraph.

Tag any section of your writing with an audience — just you, family, close friends, or everyone. When you share, each person sees only what's meant for them.[^1]

![Audience Filtering Demo](_attachments/audience-filter-demo.html)

### We never hold your words hostage.

Diaryx is local-first. Your entries live in a folder you choose as plain Markdown files you can open in any text editor. Put that folder in iCloud Drive, Dropbox, Syncthing, Git, or another external sync tool if you want it available elsewhere.

- **Works everywhere**: Web, iOS, Mac, Windows, Linux, even CLI!
- **Plain Markdown**: No proprietary format. Your files are yours, readable forever.
- **Folder-based storage**: You choose where the workspace lives and can move or sync that folder with normal file tools.
- **Share your way**: Publish to your own diaryx.org subdomain, email a filtered version, or share in person.
- **Extend with plugins**: Add drawing, audio recording, colored highlights, and more through a growing plugin library.

---

**Use Diaryx for free at [app.diaryx.org](https://app.diaryx.org)!**

---

Read more:

- See the [Diaryx Roadmap](/ROADMAP.md)
- Read about the [Diaryx Philosophy](/PHILOSOPHY.md)

[^1]: Note that this is only an interactive demo.
