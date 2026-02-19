---
title: Command-line module
description: The main CLI command implementation module
part_of: '[README](/crates/diaryx/src/README.md)'
author: adammharris
audience:
- public
contents:
- '[README](/crates/diaryx/src/cli/nav/README.md)'
- '[README](/crates/diaryx/src/cli/sync/README.md)'
attachments:
- '[mod.rs](/crates/diaryx/src/cli/mod.rs)'
- '[args.rs](/crates/diaryx/src/cli/args.rs)'
- '[attachment.rs](/crates/diaryx/src/cli/attachment.rs)'
- '[config.rs](/crates/diaryx/src/cli/config.rs)'
- '[content.rs](/crates/diaryx/src/cli/content.rs)'
- '[entry.rs](/crates/diaryx/src/cli/entry.rs)'
- '[export.rs](/crates/diaryx/src/cli/export.rs)'
- '[normalize.rs](/crates/diaryx/src/cli/normalize.rs)'
- '[property.rs](/crates/diaryx/src/cli/property.rs)'
- '[publish.rs](/crates/diaryx/src/cli/publish.rs)'
- '[search.rs](/crates/diaryx/src/cli/search.rs)'
- '[sort.rs](/crates/diaryx/src/cli/sort.rs)'
- '[template.rs](/crates/diaryx/src/cli/template.rs)'
- '[util.rs](/crates/diaryx/src/cli/util.rs)'
- '[workspace.rs](/crates/diaryx/src/cli/workspace.rs)'
- '[git.rs](/crates/diaryx/src/cli/git.rs)'
- '[import.rs](/crates/diaryx/src/cli/import.rs)'
exclude:
- '*.lock'
---
# Command-line module

In the Diaryx CLI, this module provides the majority of the functionality.

## Git Version History Commands

- `diaryx commit` — Snapshot workspace state as a git commit and compact CRDT storage.
Options: `--message <msg>`, `--skip-validation`.
- `diaryx log` — Show git commit history. Options: `--count <n>` (default: 20).

## Import Commands

- `diaryx import email <source>` — Import `.eml` files, directories of `.eml` files, or `.mbox` archives.
  Options: `--folder <name>` (default: "emails"), `--dry-run`, `--verbose`.

&nbsp;
