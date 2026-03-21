---
schema_version: 2
generated_at: '2026-03-16T22:42:06.992621+00:00'
cdn_base: https://cdn.diaryx.org
plugins:
- id: diaryx.ai
  name: AI Assistant
  version: 0.1.5
  description: AI chat assistant powered by OpenAI-compatible APIs
  author: Diaryx Team
  license: PolyForm Shield 1.0.0
  repository: https://github.com/diaryx-org/plugin-ai
  categories:
  - assistant
  - writing
  tags:
  - ai
  - chat
  - assistant
  capabilities:
  - custom_commands
  summary: Chat assistant plugin for Diaryx.
  artifact:
    url: https://cdn.diaryx.org/plugins/artifacts/diaryx.ai/0.1.5/diaryx_ai_extism.wasm
    sha256: 1d2b6fbbd0db7508c54eb76fdba3add04bd07d7944dd0f10a2e8552564bbf6dd
    size: 447416
    published_at: '2026-03-16T18:44:25Z'
  ui:
  - slot: ToolbarButton
    id: ai-chat-toggle
    label: AI Assistant
  - slot: SidebarTab
    id: ai-chat
    label: AI
  - slot: SettingsTab
    id: ai-settings
    label: AI
  requested_permissions:
    defaults:
      http_requests:
        include:
        - openrouter.ai
      plugin_storage:
        include:
        - all
    reasons:
      http_requests: Send chat requests to the configured OpenAI-compatible API endpoint.
      plugin_storage: Persist conversation history and plugin settings between sessions.
- id: diaryx.audio
  name: Audio
  version: 0.1.4
  description: Audio recorder! Keep audio notes in Diaryx
  author: Diaryx Team
  license: PolyForm Shield 1.0.0
  repository: https://github.com/diaryx-org/plugin-audio
  categories:
  - audio
  tags:
  - recorder
  - audio
  - waveform
  capabilities: []
  summary: This plugin adds audio capturing to Diaryx!
  artifact:
    url: https://cdn.diaryx.org/plugins/artifacts/diaryx.audio/0.1.4/diaryx_audio_extism.wasm
    sha256: 91361e0ca3dadeb355e327e2090725a34b0f02283f036715dcb00aa79ca3f53c
    size: 207481
    published_at: '2026-03-16T22:28:40Z'
- id: diaryx.daily
  name: Daily
  version: 0.1.7
  description: Daily entry plugin with date hierarchy, navigation, and CLI surface
  author: Diaryx Team
  license: PolyForm Shield 1.0.0
  repository: https://github.com/diaryx-org/plugin-daily
  categories:
  - productivity
  - journaling
  tags:
  - daily
  - journal
  - calendar
  capabilities:
  - workspace_events
  - custom_commands
  summary: Daily entry workflow and navigation.
  artifact:
    url: https://cdn.diaryx.org/plugins/artifacts/diaryx.daily/0.1.7/diaryx_daily_extism.wasm
    sha256: 3f2a6c4c2c09081e2590f4226954d2c65f2c3322471135145f46d69ed02f4942
    size: 905436
    published_at: '2026-03-16T19:41:22Z'
  ui:
  - slot: SidebarTab
    id: daily-panel
    label: Daily
  - slot: CommandPaletteItem
    id: daily-open-today
    label: Open Today's Entry
  - slot: CommandPaletteItem
    id: daily-open-yesterday
    label: Open Yesterday's Entry
  cli:
  - name: daily
    about: Daily entry commands
- id: diaryx.drawing
  name: Drawing
  version: 0.1.3
  description: Draw a picture in your notes!
  author: Diaryx Team
  license: PolyForm Shield 1.0.0
  repository: https://github.com/diaryx-org/plugin-drawing
  categories:
  - drawing
  - colors
  tags:
  - picture
  - colors
  - drawing
  capabilities: []
  summary: This plugin lets you draw a picture in your notes using the wonderful `perfect-freehand`
    library!
  artifact:
    url: https://cdn.diaryx.org/plugins/artifacts/diaryx.drawing/0.1.3/diaryx_drawing_extism.wasm
    sha256: 5ccf3025555da55353b5308b8cf0a9749dece22059e08873a0d050f627159bda
    size: 214112
    published_at: '2026-03-16T22:24:08Z'
- id: diaryx.heic
  name: Heic
  version: 0.1.0
  description: Support for HEIC images in Diaryx
  author: Diaryx Team
  license: PolyForm Shield 1.0.0
  repository: https://github.com/diaryx-org/plugin-heic
  categories:
  - transcoder
  - images
  tags:
  - HEIC
  - transcoder
  - images
  - apple
  capabilities: []
  summary: This plugin provides support for HEIC images in Diaryx
  artifact:
    url: https://cdn.diaryx.org/plugins/artifacts/diaryx.heic/0.1.0/diaryx_heic_extism.wasm
    sha256: 36fab80ca1f2234105492843d630e07c48801c53cda9a16566719e4221afcd4e
    size: 165745
    published_at: '2026-03-16T22:26:14Z'
- id: diaryx.highlight
  name: Highlight
  version: 0.1.3
  description: Colored highlights for your notes!
  author: Diaryx Team
  license: PolyForm Shield 1.0.0
  repository: https://github.com/diaryx-org/plugin-highlight
  categories:
  - markdown
  - editor
  - colors
  - highlight
  tags:
  - highlights
  - markdown
  capabilities: []
  summary: The Diaryx Highlight plugin allows you to color up your notes with a beautiful
    palette of 10 colors.
  artifact:
    url: https://cdn.diaryx.org/plugins/artifacts/diaryx.highlight/0.1.3/diaryx_highlight_extism.wasm
    sha256: c1517ba53d0a9e9530fa3a8edb38b7bc232e114aeb2114b768e097495d75bc87
    size: 193826
    published_at: '2026-03-16T22:24:01Z'
- id: diaryx.import
  name: Import
  version: 0.1.4
  description: Import entries from Day One, Markdown directories, and other formats
  author: Diaryx Team
  license: PolyForm Shield 1.0.0
  repository: https://github.com/diaryx-org/plugin-import
  categories:
  - import
  - migration
  tags:
  - import
  - day-one
  - markdown
  capabilities:
  - custom_commands
  summary: Import entries from external formats.
  artifact:
    url: https://cdn.diaryx.org/plugins/artifacts/diaryx.import/0.1.4/diaryx_import_extism.wasm
    sha256: 387849493b7b19851046b9e9ab7c20e7eb391f4bcf01639d0ba1c1a99f3109ae
    size: 1224198
    published_at: '2026-03-16T18:37:15Z'
  cli:
  - name: import
    about: Import entries from external formats
  requested_permissions:
    defaults:
      read_files:
        include:
        - all
      edit_files:
        include:
        - all
      create_files:
        include:
        - all
    reasons:
      read_files: Read existing entries during import.
      edit_files: Update entry metadata during import.
      create_files: Create new entries from imported data.
- id: diaryx.math
  name: Math
  version: 0.1.6
  description: LaTeX math rendering with inline ($...$) and block ($$...$$) support
  author: Diaryx Team
  license: PolyForm Shield 1.0.0
  repository: https://github.com/diaryx-org/plugin-math
  categories:
  - editor
  - formatting
  tags:
  - math
  - latex
  - editor
  capabilities:
  - editor_extension
  summary: Render inline and block LaTeX.
  artifact:
    url: https://cdn.diaryx.org/plugins/artifacts/diaryx.math/0.1.6/diaryx_math_extism.wasm
    sha256: ee9095ffbecc850de7e192ababaebf3748667bab95060c04107893649767d0cd
    size: 425883
    published_at: '2026-03-16T18:44:31Z'
  ui:
  - slot: EditorExtension
    id: mathInline
    label: Math
  - slot: EditorExtension
    id: mathBlock
    label: Math Block
- id: diaryx.publish
  name: Publish
  version: 0.1.6
  description: Export and publish content with optional format conversion
  author: Diaryx Team
  license: PolyForm Shield 1.0.0
  repository: https://github.com/diaryx-org/plugin-publish
  categories:
  - publish
  - export
  tags:
  - publish
  - export
  - html
  capabilities:
  - workspace_events
  - custom_commands
  summary: Export and publish workspaces.
  artifact:
    url: https://cdn.diaryx.org/plugins/artifacts/diaryx.publish/0.1.6/diaryx_publish_extism.wasm
    sha256: 8d7a4490e0e489e65cbe2940ea4c9cf7423366d0f139d595d318ae3ddfb6cccb
    size: 1892377
    published_at: '2026-03-16T18:37:27Z'
  ui:
  - slot: SidebarTab
    id: publish-panel
    label: Publish
  - slot: CommandPaletteItem
    id: publish-export
    label: Export...
  - slot: CommandPaletteItem
    id: publish-site
    label: Publish Site
  cli:
  - name: publish
    about: Publish workspace as HTML
  - name: preview
    about: Preview published workspace
- id: diaryx.spoiler
  name: Spoiler
  version: 0.1.5
  description: Discord-style ||spoiler|| syntax to hide text until clicked
  author: Diaryx Team
  license: PolyForm Shield 1.0.0
  repository: https://github.com/diaryx-org/plugin-spoiler
  categories:
  - editor
  - formatting
  tags:
  - spoiler
  - markdown
  - editor
  capabilities:
  - editor_extension
  summary: Hide inline text with spoiler markup.
  artifact:
    url: https://cdn.diaryx.org/plugins/artifacts/diaryx.spoiler/0.1.5/diaryx_spoiler_extism.wasm
    sha256: 06dca8ed4cdb01c8d7f26cf011560049a3a19ce6bb095eae199526feeaa93d84
    size: 189229
    published_at: '2026-03-16T18:36:46Z'
  ui:
  - slot: EditorExtension
    id: spoiler
    label: Spoiler
- id: diaryx.storage.gdrive
  name: Google Drive Storage
  version: 0.1.0
  description: Google Drive as a filesystem backend
  author: Diaryx Team
  license: PolyForm Shield 1.0.0
  repository: https://github.com/diaryx-org/plugin-storage-gdrive
  categories:
  - storage
  - integration
  tags:
  - google-drive
  - storage
  - cloud
  capabilities:
  - custom_commands
  summary: Google Drive storage backend.
  artifact:
    url: https://cdn.diaryx.org/plugins/artifacts/diaryx.storage.gdrive/0.1.0/diaryx_storage_gdrive_extism.wasm
    sha256: d1819d2b59f9b2f7abce05b9614ca8485558869bb146e5acb03c69fc7ad06acd
    size: 362804
    published_at: '2026-03-05T01:14:10Z'
  ui:
  - slot: StorageProvider
    id: diaryx.storage.gdrive
    label: Google Drive
  - slot: SettingsTab
    id: gdrive-storage-settings
    label: Google Drive
- id: diaryx.storage.s3
  name: S3 Storage
  version: 0.1.0
  description: S3-compatible object storage as a filesystem backend
  author: Diaryx Team
  license: PolyForm Shield 1.0.0
  repository: https://github.com/diaryx-org/plugin-storage-s3
  categories:
  - storage
  - integration
  tags:
  - s3
  - storage
  - cloud
  capabilities:
  - custom_commands
  summary: S3-compatible storage backend.
  artifact:
    url: https://cdn.diaryx.org/plugins/artifacts/diaryx.storage.s3/0.1.0/diaryx_storage_s3_extism.wasm
    sha256: 49f7ba4f3fd698f87d2cc6cf2d1d045a8604d34d2c8eb7e93543c1fff62117f0
    size: 393864
    published_at: '2026-03-05T01:14:09Z'
  ui:
  - slot: StorageProvider
    id: diaryx.storage.s3
    label: Amazon S3
  - slot: SettingsTab
    id: s3-storage-settings
    label: S3 Storage
- id: diaryx.sync
  name: Sync
  version: 0.1.6
  description: Real-time CRDT sync across devices
  author: Diaryx Team
  license: PolyForm Shield 1.0.0
  repository: https://github.com/diaryx-org/plugin-sync
  categories:
  - sync
  - collaboration
  tags:
  - sync
  - crdt
  - realtime
  capabilities:
  - workspace_events
  - file_events
  - crdt_commands
  - sync_transport
  - custom_commands
  summary: Realtime multi-device workspace sync.
  artifact:
    url: https://cdn.diaryx.org/plugins/artifacts/diaryx.sync/0.1.6/diaryx_sync_extism.wasm
    sha256: 99aac49823ea74190b5f2aed5f8aa3527dcd70a1124bf3be8aed6c5856a5d87d
    size: 2398613
    published_at: '2026-03-16T18:37:34Z'
  ui:
  - slot: SettingsTab
    id: sync-settings
    label: Sync
  - slot: SidebarTab
    id: share
    label: Share
  - slot: SidebarTab
    id: snapshots
    label: Snapshots
  - slot: SidebarTab
    id: history
    label: History
  - slot: StatusBarItem
    id: sync-status
    label: Sync
  - slot: WorkspaceProvider
    id: diaryx.sync
    label: Diaryx Sync
  cli:
  - name: sync
    about: Sync workspace across devices
- id: diaryx.templating
  name: Templating
  version: 0.1.5
  description: Creation-time templates and render-time body templating with Handlebars
  author: Diaryx Team
  license: PolyForm Shield 1.0.0
  repository: https://github.com/diaryx-org/plugin-templating
  categories:
  - productivity
  - editor
  tags:
  - templates
  - handlebars
  - workflow
  capabilities:
  - workspace_events
  - custom_commands
  summary: Creation-time and render-time templating.
  artifact:
    url: https://cdn.diaryx.org/plugins/artifacts/diaryx.templating/0.1.5/diaryx_templating_extism.wasm
    sha256: f12d0cd5abde477855e3165383bd2a0e50ef4774fa9f929cc7e90ee1a1af1d61
    size: 1020303
    published_at: '2026-03-16T18:37:09Z'
  ui:
  - slot: SettingsTab
    id: templating-settings
    label: Templates
  - slot: EditorExtension
    id: templateVariable
  - slot: EditorExtension
    id: conditionalBlock
  - slot: BlockPickerItem
    id: templating-if-else
    label: If / Else
  - slot: BlockPickerItem
    id: templating-for-audience
    label: For Audience
---

# Diaryx Plugin Registry

Generated at 2026-03-16T22:42:06.992621+00:00

**14** plugins available.

## AI Assistant
**ID:** `diaryx.ai` | **Version:** 0.1.5
Chat assistant plugin for Diaryx.

## Audio
**ID:** `diaryx.audio` | **Version:** 0.1.4
This plugin adds audio capturing to Diaryx!

## Daily
**ID:** `diaryx.daily` | **Version:** 0.1.7
Daily entry workflow and navigation.

## Drawing
**ID:** `diaryx.drawing` | **Version:** 0.1.3
This plugin lets you draw a picture in your notes using the wonderful `perfect-freehand` library!

## Heic
**ID:** `diaryx.heic` | **Version:** 0.1.0
This plugin provides support for HEIC images in Diaryx

## Highlight
**ID:** `diaryx.highlight` | **Version:** 0.1.3
The Diaryx Highlight plugin allows you to color up your notes with a beautiful palette of 10 colors.

## Import
**ID:** `diaryx.import` | **Version:** 0.1.4
Import entries from external formats.

## Math
**ID:** `diaryx.math` | **Version:** 0.1.6
Render inline and block LaTeX.

## Publish
**ID:** `diaryx.publish` | **Version:** 0.1.6
Export and publish workspaces.

## Spoiler
**ID:** `diaryx.spoiler` | **Version:** 0.1.5
Hide inline text with spoiler markup.

## Google Drive Storage
**ID:** `diaryx.storage.gdrive` | **Version:** 0.1.0
Google Drive storage backend.

## S3 Storage
**ID:** `diaryx.storage.s3` | **Version:** 0.1.0
S3-compatible storage backend.

## Sync
**ID:** `diaryx.sync` | **Version:** 0.1.6
Realtime multi-device workspace sync.

## Templating
**ID:** `diaryx.templating` | **Version:** 0.1.5
Creation-time and render-time templating.
