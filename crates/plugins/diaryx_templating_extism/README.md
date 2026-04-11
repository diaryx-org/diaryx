---
title: "Templating"
description: "Creation-time templates and render-time body templating with Handlebars"
id: "diaryx.templating"
version: "0.1.3"
author: "Diaryx Team"
license: "PolyForm Shield 1.0.0"
repository: "https://github.com/diaryx-org/diaryx"
categories: ["productivity", "editor"]
tags: ["templates", "handlebars", "workflow"]
capabilities: ["workspace_events", "custom_commands"]
artifact:
  url: ""
  sha256: ""
  size: 0
  published_at: ""
ui:
  - slot: SettingsTab
    id: templating-settings
    label: "Templates"
  - slot: EditorExtension
    id: templateVariable
  - slot: EditorExtension
    id: conditionalBlock
  - slot: BlockPickerItem
    id: templating-if-else
    label: "If / Else"
requested_permissions:
  defaults:
    read_files:
      include: ["all"]
    edit_files:
      include: ["all"]
    create_files:
      include: ["all"]
    delete_files:
      include: ["all"]
    plugin_storage:
      include: ["all"]
  reasons:
    read_files: "Read workspace templates from the _templates directory."
    edit_files: "Update existing workspace templates when saving changes."
    create_files: "Create new workspace templates in the _templates directory."
    delete_files: "Remove workspace templates that are no longer needed."
    plugin_storage: "Persist templating plugin configuration for the current workspace."
---

# diaryx_templating_extism

Extism WASM guest plugin that provides all templating functionality for Diaryx.

## Overview

This plugin owns creation-time and render-time templating end-to-end:

- list, get, save, and delete workspace templates
- render body templates from frontmatter values and apply `:vis[...]` visibility directives during audience-aware rendering
- fast-path `HasTemplates` check for body content
- render creation-time templates with `{{variable}}` substitution
- editor extensions via `Builtin` manifest type for TemplateVariable and ConditionalBlock

### Render-time variables

In addition to every frontmatter key, the following virtual variables are exposed
to render-time templates:

- `{{ filename }}` — entry file stem
- `{{ filepath }}` — workspace-relative path
- `{{ extension }}` — file extension
- `{{ viewer_audience }}` — comma-joined string of the viewer's audience
  memberships (only present when `RenderBody` is called with an `audience`)
- `{{ viewer_audiences }}` — array form for `{{#each viewer_audiences}}…{{/each}}`

`RenderBody`'s `audience` parameter accepts either a single string (single
audience) or an array of strings (viewer belongs to multiple audiences). A
`:vis[...]{aud}` directive is included when its tag list overlaps the viewer's
audiences — see `diaryx_core::visibility::filter_body_for_audiences`.

## Commands

- `ListTemplates` — list workspace and built-in templates
- `GetTemplate` — get template content by name
- `SaveTemplate` — save template to workspace `_templates/` folder
- `DeleteTemplate` — delete a workspace template
- `RenderBody` — render body templates with Handlebars for a given body + frontmatter
- `HasTemplates` — fast-path check for `{{` in body
- `RenderCreationTemplate` — render a creation-time template with variable substitution

## Exports

- `manifest`
- `init`
- `shutdown`
- `handle_command`
- `get_config`
- `set_config`
- `on_event`

## Build

```bash
cargo build -p diaryx_templating_extism --target wasm32-unknown-unknown --release
```
