---
title: diaryx_ai_extism
description: Extism AI assistant plugin for Diaryx
author: adammharris
audience:
  - public
  - developers
part_of: "[README](/crates/README.md)"
attachments:
  - "[Cargo.toml](/crates/diaryx_ai_extism/Cargo.toml)"
  - "[lib.rs](/crates/diaryx_ai_extism/src/lib.rs)"
  - "[chat.rs](/crates/diaryx_ai_extism/src/chat.rs)"
  - "[ui.html](/crates/diaryx_ai_extism/src/ui.html)"
exclude:
  - "*.lock"
---

# diaryx_ai_extism

`diaryx_ai_extism` provides the `diaryx.ai` plugin used in Web/Tauri hosts.

## Features

- AI chat sidebar UI rendered via plugin iframe (`get_component_html`)
- Multi-conversation history persisted in plugin storage
- Tool-use loop for reading files (`read_file`, `list_files`)
- Two provider modes:
  - `byo`: user supplies OpenAI-compatible endpoint/key/model
  - `managed`: Diaryx Plus managed mode (no user API key required)

## Managed Mode

Managed mode expects host-injected command params for `chat` / `chat_continue`:

- `managed.server_url`
- `managed.auth_token`
- `managed.tier`

The plugin routes managed requests to:

- `POST {server_url}/api/ai/chat/completions`

Managed mode returns structured plugin errors:

- `plus_required`
- `managed_unavailable`

BYO mode keeps OpenAI-compatible behavior and normalizes endpoints to avoid
duplicate `/chat/completions` path appends.
