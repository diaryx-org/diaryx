---
title: "AI Assistant"
description: "AI chat assistant powered by OpenAI-compatible APIs"
id: "diaryx.ai"
version: "0.1.2"
author: "Diaryx Team"
license: "PolyForm Shield 1.0.0"
repository: "https://github.com/diaryx-org/diaryx"
categories: ["assistant", "writing"]
tags: ["ai", "chat", "assistant"]
capabilities: ["custom_commands"]
artifact:
  url: ""
  sha256: ""
  size: 0
  published_at: ""
ui:
  - slot: ToolbarButton
    id: ai-chat-toggle
    label: "AI Assistant"
  - slot: SidebarTab
    id: ai-chat
    label: "AI"
  - slot: SettingsTab
    id: ai-settings
    label: "AI"
requested_permissions:
  defaults:
    http_requests:
      include: ["openrouter.ai"]
    read_files:
      include: ["all"]
    edit_files:
      include: ["all"]
    plugin_storage:
      include: ["all"]
  reasons:
    http_requests: "Send chat requests to the configured OpenAI-compatible API endpoint."
    plugin_storage: "Persist conversation history and plugin settings between sessions."
    read_files: "Read existing conversation files so AI chat saves preserve Diaryx frontmatter and hierarchy metadata."
    edit_files: "Update the selected workspace conversation file with the latest chat transcript."
---

# diaryx_ai_extism

`diaryx_ai_extism` provides the `diaryx.ai` plugin used in Web/Tauri hosts.

## Features

- AI chat sidebar UI rendered via plugin iframe (`get_component_html`)
- Multi-conversation history persisted in plugin storage
- New conversations prompt for a workspace file and are added to the contents hierarchy
- Chat transcripts are mirrored into the selected workspace file while preserving frontmatter metadata
- Tool-use loop for reading files (`read_file`, `list_files`)
- Explicit per-request HTTP timeout for chat provider calls (avoids indefinite hangs)
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
