---
title: diaryx src
description: Source code for the Diaryx CLI application
part_of: '[README](/crates/diaryx/README.md)'
contents:
- '[README](/crates/diaryx/src/cli/README.md)'
exclude:
- '*.lock'
---
# Diaryx CLI Source

This directory contains the source code for the Diaryx CLI application.

## Structure

- `main.rs` - Application entry point, parses CLI arguments and dispatches commands
- `editor.rs` - Editor integration for opening files in the user's preferred editor
- `cli/` - Command implementations for the core CLI plus optional `plugins` and `edit` feature surfaces

&nbsp;
