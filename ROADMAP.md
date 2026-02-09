---
title: ROADMAP
description: The plan for future Diaryx features
author: adammharris
created: 2025-12-05T12:06:55-07:00
updated: 2026-01-29T22:42:14-07:00
audience:
- public
part_of: '[README](/README.md)'
---

# Roadmap

## v0.12.0

### Git-Backed Version Control with CRDT Compaction

#### Motivation

Currently, every CRDT update is stored permanently in the SQLite `updates` table. The database grows without bound on both clients and the sync server. Git provides a natural solution: periodically snapshot the workspace state as a git commit, then compact (discard) the CRDT updates that preceded it. This keeps the CRDT layer thin and fast (a "sync buffer") while git preserves the full version history as clean, human-readable markdown.

Because sync is part of a paid plan, the server-side git integration should be coupled with the sync server infrastructure.

#### Authority Model: Git is Authoritative

Git is the source of truth. CRDT is a real-time sync buffer.

```
Edit → CRDT (sync buffer) → quiescent commit → Git (authoritative history)
                            → compact old CRDT updates from SQLite
                            → attachments uploaded to S3 (content-addressed)

Client connects → sync CRDT state (includes any post-commit changes)
Detected drift  → Git wins → rebuild CRDT from last commit
```

- **Git** is the canonical version history. If CRDT state ever drifts or corrupts, git is correct.
- **CRDT** handles real-time sync, conflict resolution, and buffering recent changes between commits.
- **Compaction** discards CRDT updates older than the last git commit, keeping SQLite small.

#### What Git Stores

The git repo contains only markdown files with frontmatter. Attachments are stored in S3 (content-addressed by hash) and referenced from frontmatter as string paths. The git repo stays small and fast.

```yaml
# Example frontmatter with attachment reference
---
title: Meeting Notes
attachments:
  - "[diagram.png](/_attachments/diagram.png)"
---
```

The actual attachment bytes live in S3, keyed by content hash. Git never touches binary files.

#### How Commits Work

1. **Materialize**: Export current CRDT state to markdown files (reuse `export_snapshot_zip` logic, but write to a git worktree instead of a ZIP)
2. **Upload**: Any new local attachments are uploaded to S3; frontmatter references are updated
3. **Stage**: Stage files explicitly using the CRDT file index (not `git add -A`, to avoid capturing untracked user files)
4. **Validate**: Run CRDT sanity checks (see below). If validation fails, skip the commit and log an anomaly for investigation. Do not auto-rebuild CRDT unless the failure persists across multiple attempts.
5. **Commit**: Create a git commit with a generated message (timestamp, device, summary of changed files)
6. **Compact**: Call `SqliteStorage::compact()` to discard old CRDT updates, keeping only a configurable number of recent updates

Sanity checks at step 4:
- Did all body documents materialize to non-empty content?
- Does the materialized file set match the workspace CRDT's file index?
- Are unchanged files byte-identical to the last commit?

This reuses the existing `WorkspaceCrdt::get_path()` and `BodyDocManager` logic to produce the same markdown+frontmatter files the CLI already works with.

#### Self-Healing

If sanity checks fail repeatedly (e.g., 3 consecutive failures), the CRDT is rebuilt from the last known-good git commit. On the server, this only happens when no clients are connected (since auto-commit already requires quiescence). On clients, it happens on the next manual or auto-commit attempt.

A single failure skips the commit and logs a warning. Transient issues (e.g., a body doc mid-sync) resolve themselves once the CRDT settles.

#### Server-Side (diaryx_sync_server)

Each workspace gets a bare git repository alongside its SQLite database:

```
<workspaces_dir>/
  <workspace_id>.db        # CRDT sync buffer (SQLite)
  <workspace_id>.git/      # Bare git repo (version history)
```

- **Auto-commit trigger**: The server commits when a workspace is quiescent — no CRDT changes received for a configurable interval (default: 30 minutes) AND no clients are connected. This ensures the CRDT state is stable and complete before materialization. A "dirty" flag is set by `DiaryxHook::on_change` and cleared after a successful commit. A maximum staleness threshold (default: 24 hours) forces a commit even if a client stays connected indefinitely.
- **Commit flow**: Materialize workspace CRDT → write to a temporary worktree → validate → `git commit` → compact CRDT updates
- **Paid plan gating**: Git history is a sync-tier feature. The server only runs auto-commit for workspaces with an active sync subscription.

New server config fields:

```toml
[git]
quiescence_minutes = 30              # Commit after this many minutes of no changes + no clients (0 = disabled)
max_staleness_hours = 24             # Force commit after this many hours even with connected clients
```

#### Client-Side (diaryx_core + CLI + Tauri)

Local workspaces use git for version history. CLI and Tauri share the same Rust code in `diaryx_core`. Materialization is always from CRDT state — on-disk files are an output of the commit process, not inputs.

- **Git repo location**: `<workspace_root>/.git/` (the workspace root IS the git repo)
- **Auto-commit**: Configurable in `config.toml`, default every 30 minutes of inactivity
- **Manual commit**: `diaryx commit` — materializes CRDT state and commits
- **CRDT compaction**: After commit, compact the local `.diaryx/crdt.db`

New config fields:

```toml
[git]
auto_commit = true                   # Enable auto-commit (default: true)
auto_commit_interval_minutes = 30    # Commit after this many minutes of inactivity (default: 30)
```

New CLI commands:

```
diaryx commit [-m <message>]   # Manual commit (materializes CRDT → git commit)
diaryx log                     # Show git log for workspace
diaryx diff                    # Show uncommitted CRDT changes vs last commit (stretch goal)
```

#### Web Client (apps/web)

The web client does not run git locally. It syncs via CRDT to the sync server, and the server handles git commits. The web UI exposes:

- **Version history view**: Fetch commit log from server API (`GET /api/workspaces/:id/history`)
- **Restore to version**: Disallowed while more than one client is connected to the workspace. When allowed, the server rebuilds the CRDT from the target commit and the single connected client receives the restored state via normal CRDT sync. (`POST /api/workspaces/:id/restore`)
- **Commit now button**: Trigger an immediate server-side commit (`POST /api/workspaces/:id/commit`)

Future option: [wasm-git](https://github.com/petersalomonsen/wasm-git) (libgit2 compiled to WASM) could enable local git in the browser via OPFS, making the web client a full peer. This is not planned for v0.12.0.

#### Controlled Git Interop

Diaryx is not a general-purpose git server. Git is used internally for clean, elegant version history. Users interact with git through Diaryx's own import/export flows, which handle attachment resolution seamlessly.

**Export** ("Download my data"):
- Diaryx packages the workspace as a self-contained directory:
  - All markdown files with frontmatter
  - All attachments downloaded from S3 and placed at their referenced paths
  - Full `.git/` history
- The result is a normal git repo AND a normal folder of markdown files
- Users can push this to their own GitHub/GitLab for backup

**Import** ("Bring my data in"):
- User points Diaryx at a directory of markdown files (with or without `.git/`)
- Diaryx ingests: parses frontmatter, uploads attachment files to S3, builds CRDT state
- If `.git/` history exists, it is preserved as the starting point

This means users can leave Diaryx at any time with a complete, portable copy of their data — markdown files, attachments, and full version history.

#### Client Capabilities

| | CLI | Tauri | Web |
|---|---|---|---|
| Shared code | `diaryx_core` | `diaryx_core` | `diaryx_wasm` |
| Git impl | `git2` (native) | `git2` (native) | Server API |
| Local git repo | Yes | Yes | No |
| Local files | Real filesystem | Real filesystem | OPFS |
| CRDT storage | `.diaryx/crdt.db` | `.diaryx/crdt.db` | OPFS SQLite |
| Offline history | Yes | Yes | No (cached view) |
| Commit | Local | Local | Server-side |
| Attachments | Local disk + S3 | Local disk + S3 | S3 direct |

#### New Feature Flag

A new `git` feature flag is added to `diaryx_core` alongside the existing CRDT features. No module rename.

- New feature: `git = ["crdt-sqlite", "dep:git2"]`
- Existing features unchanged: `crdt`, `crdt-sqlite`, `native-sync`
- New `crdt::git` submodule (behind the `git` feature) for git operations alongside the existing CRDT code

#### Implementation Phases

**Phase 1: Core git operations (`diaryx_core`)**
- New `crdt::git` submodule with functions for:
  - `init_repo(path)` — initialize a git repo at workspace root
  - `commit_workspace(storage, workspace_crdt, body_docs, repo_path, message)` — materialize + validate + commit
  - `compact_after_commit(storage, keep_recent)` — compact CRDT updates after successful commit
  - `rebuild_crdt_from_git(repo_path, storage)` — self-healing: rebuild CRDT from last git commit
- Use `git2` crate (libgit2 bindings) — no dependency on system git
- Reuse `WorkspaceCrdt` / `BodyDocManager` / frontmatter serialization for materialization
- Refactor `export_snapshot_zip` materialization logic from `diaryx_sync_server` into `diaryx_core` so both server and clients share it

**Phase 2: CLI + Tauri integration**
- `diaryx commit`, `diaryx log` commands (`diaryx diff` is a stretch goal)
- Auto-commit background task (runs during `diaryx sync` or as a standalone daemon)
- Config fields for git settings
- Tauri gets the same git functionality via shared `diaryx_core` code

**Phase 3: Server integration (`diaryx_sync_server`)**
- Background task that commits quiescent workspaces (no changes + no clients, or max staleness exceeded)
- New API endpoints: history, restore (single-client only), manual commit, export (with attachments from S3)
- Import endpoint: accepts directory/ZIP, uploads attachments to S3, builds CRDT + git
- CRDT sanity validation on commit; self-healing after repeated failures
- Compaction runs after each successful commit

**Phase 4: Web UI**
- Version history panel (commit log with timestamps, diffs)
- "Commit now" button
- Restore-to-version flow (disabled with >1 connected client)
- Export/import with attachment resolution

#### Compaction Strategy

- **After each git commit**: Compact all CRDT documents in the workspace, keeping only `keep_recent` updates (default: 50). The snapshot in the `documents` table is updated to the full current state.
- **Without git**: No automatic compaction (current behavior). Users can still manually compact via CLI.
- **Safety**: Compaction only runs after a successful commit. If the commit or validation fails, no compaction occurs. Compaction never runs independently of a successful commit.
- **Server timing**: On the server, compaction is safe because auto-commit only fires when no clients are connected. No active sync sessions are affected.

#### Fine-Grained History

The existing `HistoryManager` provides per-update time-travel within the CRDT update window (the updates retained after compaction). After a commit + compaction, only the most recent updates (default: 50) remain — older fine-grained history is replaced by git commit-level snapshots. TipTap editor undo/redo covers the primary use case for keystroke-level reversal. Per-update time-travel may be revisited in a future version if needed.

#### Design Decisions

- **Git is authoritative**: If CRDT ever drifts, git wins and CRDT is rebuilt. This provides a reliability backstop against CRDT sync glitches.
- **Quiescent commits**: The server only commits when no clients are connected and no changes are in flight. This eliminates races between commit/compaction and active sync, and ensures the materialized state is complete and stable.
- **Conservative self-healing**: A single validation failure skips the commit but preserves CRDT state. Only repeated failures trigger a CRDT rebuild from git. This avoids discarding valid edits due to transient issues.
- **Explicit file staging**: Commits stage files listed in the CRDT file index rather than using `git add -A`. This prevents accidentally committing untracked user files in client-side repos.
- **git2 crate, not shell git**: Avoids dependency on system git installation. Works on all native platforms.
- **Bare repos on server**: No working tree needed — commits are built programmatically from CRDT state using a temporary worktree.
- **Standard repos on client**: The workspace IS the git repo, so users can use normal git tools alongside Diaryx.
- **CRDT is the source for materialization**: On-disk files are always an output of the commit process, never an input. The CRDT state is authoritative for what gets committed.
- **Markdown files, not CRDT blobs**: Git stores human-readable markdown, not raw CRDT binary data. Git history is useful even outside Diaryx.
- **Attachments in S3, not git**: Binary files are stored in S3 (content-addressed by hash) and referenced from frontmatter. Git stays small and fast. Export bundles everything together seamlessly.
- **Controlled interop, not a git server**: Diaryx is not a general-purpose git host. Import/export go through Diaryx APIs that handle attachment resolution, validation, and CRDT construction.
- **No git conflicts**: The server commits from a single CRDT source of truth — there are never merge conflicts. Client-side repos are single-writer (the local CRDT). Multi-device reconciliation is handled by CRDT sync, not git merge.
- **Restore requires single client**: Version restore is only allowed when at most one client is connected. This avoids the complexity of force-reverting multiple live editors and can be relaxed in a future version if needed.
- **Web uses server APIs for now**: The web client relies on the server for git operations. A future option (wasm-git) could enable local git in the browser via OPFS.
- **No module rename**: The `crdt` module and feature flags keep their existing names. Git operations are added as a new `crdt::git` submodule behind a new `git` feature flag. This avoids a large rename churn across the codebase.

## Other considerations

### Cross-platform import

Import from Obsidian (add all part_of/contents properties + index files)

Perhaps is already possible for Obsidian with validation fixes. The hard part is handling Wikilinks, which needs design decisions

### Undo/redo

I would like `diaryx undo` and `diaryx redo` commands to undo/redo any command that was previously done, because it is easy to make mistakes.

### Encryption

Ideally hot-swappable similar to backup backends. Maybe Cryptomator?

### Math/diagrams

TipTap has an extension for LaTeX, but I would like to support Mermaid diagrams and Typst syntax as well. Maybe there is a way to swap parsers and return an image?
