---
title: diaryx_git
description: Git version history for Diaryx workspaces
part_of: '[README](/crates/README.md)'
attachments:
- '[Cargo.toml](/crates/diaryx_git/Cargo.toml)'
exclude:
- '*.lock'
---
# diaryx_git

Git-based version history for Diaryx workspaces. Commits materialized workspace state (markdown files with frontmatter) as git snapshots.

## Design

This crate takes **materialized file data** as input — a list of `{path, content}` pairs — rather than accessing CRDTs directly. This keeps it decoupled from `diaryx_sync`. The CLI obtains materialized files via the sync plugin's `MaterializeWorkspace` command.

## Public API

- `commit_workspace(files, repo, options, tracker)` — Write files to the git index and create a commit.
- `init_repo(path, kind)` / `open_repo(path)` — Initialize or open a git repository.
- `HealthTracker` — Tracks consecutive failures and recommends skip/rebuild actions.

## Dependencies

- `git2` — Git operations
- `diaryx_core` — Error types
- `chrono` — Timestamps
