---
title: "GitHub Sync"
description: "GitHub-backed workspace snapshot sync and commit history"
id: "diaryx.github"
version: "0.1.0"
author: "Diaryx Team"
license: "MIT"
repository: "https://github.com/diaryx-org/diaryx"
categories: ["sync", "storage"]
tags: ["github", "sync", "history"]
capabilities: ["custom_commands"]
artifact:
  url: ""
  sha256: ""
  size: 0
  published_at: ""
ui:
  - slot: SettingsTab
    id: github-settings
    label: "GitHub"
  - slot: SidebarTab
    id: github-history
    label: "GitHub"
  - slot: WorkspaceProvider
    id: diaryx.github
    label: "GitHub"
requested_permissions:
  defaults:
    plugin_storage:
      include: ["all"]
    http_requests:
      include: ["api.github.com", "github.com"]
    read_files:
      include: ["all"]
    edit_files:
      include: ["all"]
    create_files:
      include: ["all"]
  reasons:
    plugin_storage: "Store plugin config and GitHub credentials."
    http_requests: "Call GitHub OAuth, contents, and commits APIs."
    read_files: "Read local workspace files before snapshot upload."
    edit_files: "Restore snapshot files into the current workspace."
    create_files: "Create new files while downloading a remote workspace snapshot."
---

# diaryx_github_extism

This plugin treats GitHub as a versioned snapshot store for Diaryx workspaces.

## What It Does

- Links a workspace to a JSON snapshot file committed into a GitHub repository
- Uploads the current workspace as a new Git commit
- Lists linked remote workspaces from the configured repository folder
- Downloads a remote workspace snapshot into the current local workspace
- Adds a RightSidebar tab that shows recent commits for the linked workspace snapshot

## Auth

- Preferred: host-managed GitHub OAuth client ID with PKCE
- Fallback: paste a personal access token into the settings tab

## Build

```bash
cargo build -p diaryx_github_extism --target wasm32-unknown-unknown --release
```
