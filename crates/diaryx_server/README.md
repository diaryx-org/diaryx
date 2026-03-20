---
title: diaryx_server
description: Platform-agnostic server core for Diaryx cloud adapters
author: adammharris
audience:
- public
- developers
part_of: '[README](/crates/README.md)'
contents:
- '[README](/crates/diaryx_server/src/README.md)'
attachments:
- '[Cargo.toml](/crates/diaryx_server/Cargo.toml)'
exclude:
- '*.lock'
---
# diaryx_server

Platform-agnostic Rust core for Diaryx server-side business logic.

This crate is intentionally independent from HTTP frameworks and cloud/runtime
bindings. It exposes shared domain types, capability traits, and use cases that
can be reused by native adapters (`diaryx_sync_server`) and future Cloudflare
adapters.

The shared core now includes typed `ServerCoreError` variants plus portable
domain-management flows so adapters can map consistent outcomes without relying
on string-matched error handling.
