---
title: Cargo crates for Diaryx
author: adammharris
contents:
  - diaryx/README.md
  - diaryx_core/README.md
  - diaryx_wasm/README.md
  - LICENSE.md
---

This folder contains three crates for Diaryx.

- [`diaryx`](diaryx/README.md): CLI interface
- [`diaryx_core`](diaryx_core/README.md): Core functions shared across all Diaryx clients
- [`diaryx_wasm`](diaryx_wasm/README.md): WASM version of `diaryx_core` to be used in the web client at [`../apps/web`](../apps/web/README.md)

Cargo also copies the LICENSE.md file here for publishing crates. It is has the same content as `../LICENSE.md`.
