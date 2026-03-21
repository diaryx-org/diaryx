---
schema_version: 1
generated_at: "2026-03-20T00:00:00Z"
bundles:
  - id: "bundle.minimal"
    name: "Minimal"
    version: "1.0.0"
    summary: "A clean workspace with no plugins"
    description: "Start with a plain workspace. Add plugins later from the marketplace."
    author: "Diaryx Team"
    license: "PolyForm Shield 1.0.0"
    categories: ["starter"]
    tags: ["minimal", "clean"]
    icon: null
    screenshots: []
    theme_id: "theme.default"
    plugins: []
  - id: "bundle.default"
    name: "Default"
    version: "1.0.0"
    summary: "Recommended starter bundle"
    description: "The default Diaryx experience with sync and publish."
    author: "Diaryx Team"
    license: "PolyForm Shield 1.0.0"
    categories: ["starter"]
    tags: ["default"]
    icon: null
    screenshots: []
    theme_id: "theme.default"
    starter_workspace_id: "starter.minimal"
    plugins:
      - plugin_id: "diaryx.sync"
        required: false
        enable: true
      - plugin_id: "diaryx.publish"
        required: false
        enable: true
---
# Bundle Registry
