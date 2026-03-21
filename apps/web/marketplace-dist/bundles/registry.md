---
schema_version: 1
generated_at: "2026-03-10T00:00:00Z"
bundles:
  - id: "bundle.default"
    name: "Default"
    version: "1.0.0"
    summary: "Recommended setup with all core plugins and a getting-started guide."
    description: "The recommended starting point for new users. Includes import, sync, publish, AI, daily notes, and templating plugins, plus a guided welcome workspace."
    author: "Diaryx Team"
    license: "PolyForm Shield 1.0.0"
    repository: "https://github.com/diaryx-org/diaryx"
    categories: ["onboarding", "recommended"]
    tags: ["default", "all-plugins", "getting-started"]
    icon: null
    screenshots: []
    artifact:
      url: "https://cdn.diaryx.org/bundles/artifacts/bundle.default/1.0.0/bundle.json"
      sha256: "982f1af669e2a20a503a4ac8ff0f12a9dc3365935efaafa95dcbf7bd9014d1e0"
      size: 1020
      published_at: "2026-03-10T00:00:00Z"
    theme_id: "default"
    typography_id: null
    typography: null
    plugins:
      - plugin_id: "diaryx.import"
        required: false
        enable: true
      - plugin_id: "diaryx.sync"
        required: false
        enable: true
      - plugin_id: "diaryx.publish"
        required: false
        enable: true
      - plugin_id: "diaryx.ai"
        required: false
        enable: true
      - plugin_id: "diaryx.daily"
        required: false
        enable: true
      - plugin_id: "diaryx.templating"
        required: false
        enable: true
    starter_workspace_id: "starter.getting-started"
    spotlight:
      - target: "workspace-tree"
        title: "Your workspace"
        description: "All your entries live here in a tree. Click any entry to open it."
        placement: "right"
      - target: "editor-area"
        title: "The editor"
        description: "Write and format content with the rich text editor."
        placement: "bottom"
      - target: "properties-panel"
        title: "Properties & history"
        description: "View metadata, attachments, and version history for the current entry."
        placement: "left"
      - target: "marketplace-button"
        title: "Marketplace"
        description: "Discover themes, plugins, and templates to customize your workspace."
        placement: "top"
      - target: "command-palette-button"
        title: "Command palette"
        description: "Quick access to all actions — press Cmd+K (or Ctrl+K) anytime."
        placement: "top"

  - id: "bundle.minimal"
    name: "Minimal"
    version: "1.0.0"
    summary: "A clean slate with no plugins — just you and your notes."
    description: "The lightest possible setup. No plugins, no extras — just a blank workspace. You can always add plugins later from the marketplace."
    author: "Diaryx Team"
    license: "PolyForm Shield 1.0.0"
    repository: "https://github.com/diaryx-org/diaryx"
    categories: ["onboarding", "minimal"]
    tags: ["minimal", "blank", "lightweight"]
    icon: null
    screenshots: []
    artifact:
      url: "https://cdn.diaryx.org/bundles/artifacts/bundle.minimal/1.0.0/bundle.json"
      sha256: "57a87fb062674ff40e7edf0054ba554468035648a675cc3849eb22745f646470"
      size: 370
      published_at: "2026-03-10T00:00:00Z"
    theme_id: "default"
    typography_id: null
    typography: null
    plugins: []
    starter_workspace_id: "starter.minimal"

  - id: "bundle.writer-focus"
    name: "Writer Focus"
    version: "1.0.0"
    summary: "Editorial theme + longform typography + core writing plugins."
    description: "A calm, print-like writing setup for essays and reflective journaling."
    author: "Diaryx Curated"
    license: "PolyForm Shield 1.0.0"
    repository: "https://github.com/diaryx-org/diaryx"
    categories: ["writing", "focus"]
    tags: ["editorial", "longform", "calm"]
    icon: null
    screenshots: []
    artifact:
      url: "https://cdn.diaryx.org/bundles/artifacts/bundle.writer-focus/1.0.0/bundle.json"
      sha256: "97ebc5962f3aea03da53fa6ccc143a910679e38dd8a4c1ac6a1b4dad13fc57b8"
      size: 606
      published_at: "2026-03-04T23:45:00Z"
    theme_id: "theme.paper-ink"
    typography_id: "typography.longform-serif"
    typography:
      lineHeight: 1.9
    plugins:
      - plugin_id: "diaryx.sync"
        required: true
        enable: true
      - plugin_id: "diaryx.publish"
        required: false
        enable: true
    starter_workspace_id: null

  - id: "bundle.night-ops"
    name: "Night Ops"
    version: "1.0.0"
    summary: "Neon dark palette + mono typography for technical work."
    description: "A dark technical bundle for code-adjacent notes, logs, and plans."
    author: "Diaryx Curated"
    license: "PolyForm Shield 1.0.0"
    repository: "https://github.com/diaryx-org/diaryx"
    categories: ["technical", "dark"]
    tags: ["neon", "mono", "ops"]
    icon: null
    screenshots: []
    artifact:
      url: "https://cdn.diaryx.org/bundles/artifacts/bundle.night-ops/1.0.0/bundle.json"
      sha256: "27ab6993ec8d22f81c6f2e1fd3ae91cb180f79535c172c629ade68de372359cc"
      size: 623
      published_at: "2026-03-04T23:45:00Z"
    theme_id: "theme.midnight-neon"
    typography_id: "typography.operator-mono"
    typography:
      baseFontSize: 16
      contentWidth: "wide"
    plugins:
      - plugin_id: "diaryx.sync"
        required: true
        enable: true
      - plugin_id: "diaryx.ai"
        required: false
        enable: true
    starter_workspace_id: null

  - id: "bundle.bright-studio"
    name: "Bright Studio"
    version: "1.0.0"
    summary: "Fresh colorway + compact system typography for active editing."
    description: "A daytime collaboration bundle with energetic visuals and compact type."
    author: "Diaryx Curated"
    license: "PolyForm Shield 1.0.0"
    repository: "https://github.com/diaryx-org/diaryx"
    categories: ["editing", "bright"]
    tags: ["studio", "compact", "fresh"]
    icon: null
    screenshots: []
    artifact:
      url: "https://cdn.diaryx.org/bundles/artifacts/bundle.bright-studio/1.0.0/bundle.json"
      sha256: "8e9324eb5be0e9934c7a412db167e9331804046d6a868f7c8803cc50903635f7"
      size: 718
      published_at: "2026-03-04T23:45:00Z"
    theme_id: "theme.citrus-bloom"
    typography_id: "typography.system-compact"
    typography:
      baseFontSize: 16
    plugins:
      - plugin_id: "diaryx.sync"
        required: true
        enable: true
      - plugin_id: "diaryx.daily"
        required: false
        enable: true
      - plugin_id: "diaryx.publish"
        required: false
        enable: false
    starter_workspace_id: null
---
# Diaryx Bundle Registry

Generated curated bundle registry for marketplace bootstrap.
