---
title: diaryx src
description: Source code for the Diaryx CLI application
part_of: '[README](/crates/diaryx/README.md)'
contents:
  - '[README](/crates/diaryx/src/cli/README.md)'
attachments:
  - '[main.rs](/crates/diaryx/src/main.rs)'
  - '[editor.rs](/crates/diaryx/src/editor.rs)'
exclude:
  - '*.lock'
---

# Diaryx CLI Source

This directory contains the source code for the Diaryx CLI application.

## Structure

- `main.rs` - Application entry point, parses CLI arguments and dispatches commands
- `editor.rs` - Editor integration for opening files in the user's preferred editor
- `cli/` - Command implementations for all CLI subcommands
