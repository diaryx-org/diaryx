---
title: "Templating Design"
description: "Design document for render-time templating in Diaryx"
created: 2026-02-21T00:00:00-07:00
audience:
- public
---

# Render-Time Templating

## Overview

Add Handlebars-style templating to Diaryx entry bodies. Frontmatter properties become template variables that are resolved at **view/publish time**, not at file creation time. The raw template syntax is preserved in the saved markdown file.

### Example

```markdown
---
title: Hello World
part_of: '[Index](/index.md)'
audience:
  - friends
  - family
  - public
links:
  - '[Link 1](https://link1.com)'
  - '[Link 2](https://link2.com)'
---

# {{ title }}

This entry is part of {{ part_of }}!

Viewable by:
{{#each audience}}
- {{this}}
{{/each}}

Links:
{{#each links}}
- {{this}}
{{/each}}

The filename is: {{ filename }}

{{#if viewable_by_public}}
Hello public audience!
{{/if}}
```

## Architecture

### Separation from Creation-Time Templates

The existing `template.rs` handles creation-time substitution (when running `diaryx create` or `diaryx today`). Render-time templating is a separate system:

| | Creation-time (existing `template.rs`) | Render-time (new) |
|---|---|---|
| **When** | `diaryx create`, `diaryx today` | Every view/publish |
| **Variables** | Date/time, title, filename | Frontmatter values + virtual props |
| **Syntax** | `{{variable}}` (string replace) | `{{variable}}`, `{{#each}}`, `{{#if}}` |
| **Persisted in file** | No (resolved before writing) | Yes (raw syntax stored) |
| **Engine** | Custom string replacement | `handlebars` crate |

### Template Engine

Use the `handlebars` crate (pure Rust, WASM-compatible). The Handlebars syntax (`{{#each}}`, `{{#if}}`, `{{this}}`) matches the desired syntax exactly.

### Rendering Pipeline

```
Read file
  → Parse frontmatter (existing)
  → Build template context:
      1. All frontmatter key-value pairs
      2. Virtual properties (filename, path, etc.)
      3. Computed booleans (viewable_by_<audience>, has_<property>)
  → Render body through Handlebars engine
  → Pass rendered markdown to display/publish pipeline
```

## Implementation Plan

### Phase 1: Core Engine (`crates/diaryx_core`)

#### 1.1 Add `handlebars` dependency

In `crates/diaryx_core/Cargo.toml`, add `handlebars` as an optional dependency behind a new `"templating"` feature flag. Enable it alongside the `"markdown"` feature in downstream crates.

```toml
[dependencies]
handlebars = { version = "6", optional = true }

[features]
templating = ["handlebars"]
```

#### 1.2 New module: `src/body_template.rs`

Core rendering logic, separate from the existing `template.rs` (creation-time templates):

```rust
/// Render-time body templating using Handlebars.
///
/// Takes an entry's frontmatter + file metadata and renders
/// template expressions in the body.

pub struct BodyTemplateRenderer {
    handlebars: Handlebars<'static>,
}

impl BodyTemplateRenderer {
    pub fn new() -> Self;

    /// Build a template context from frontmatter + file metadata
    pub fn build_context(
        frontmatter: &IndexMap<String, Value>,
        file_path: &Path,
    ) -> serde_json::Value;

    /// Render template expressions in the body
    pub fn render(&self, body: &str, context: &serde_json::Value) -> Result<String>;

    /// Check if a body contains template expressions
    pub fn has_templates(body: &str) -> bool;
}
```

#### 1.3 Virtual Properties

Properties computed from file metadata, not stored in frontmatter:

| Property | Source | Example |
|----------|--------|---------|
| `filename` | File path basename without extension | `hello-world` |
| `filepath` | Workspace-relative path | `notes/hello-world.md` |
| `extension` | File extension | `md` |

#### 1.4 Computed Booleans

Auto-generated from array properties:

- `audience: [friends, family, public]` generates:
  - `viewable_by_friends: true`
  - `viewable_by_family: true`
  - `viewable_by_public: true`
- General pattern: for any array property `foo` with value `bar`, generate `has_foo_bar: true`

The naming convention for `audience` specifically uses `viewable_by_` as a special case, since "audience" has semantic meaning in Diaryx.

#### 1.5 Integration Points

Add a `render_body_template()` method to `DiaryxApp`:

```rust
impl<FS: AsyncFileSystem> DiaryxApp<FS> {
    /// Render template expressions in an entry's body
    pub async fn render_body_template(&self, path: &Path) -> Result<String>;
}
```

### Phase 2: Publishing Integration

#### 2.1 Update `publish/mod.rs`

In `process_file()`, add template rendering before markdown-to-HTML conversion:

```rust
// Current flow:
let parsed = frontmatter::parse_or_empty(&content)?;
let html_body = self.markdown_to_html(&parsed.body);

// New flow:
let parsed = frontmatter::parse_or_empty(&content)?;
let rendered_body = body_template::render(&parsed.body, &parsed.frontmatter, path)?;
let html_body = self.markdown_to_html(&rendered_body);
```

#### 2.2 Audience-Aware Rendering

When publishing with `PublishOptions.audience`, pass the target audience to the template context so `{{#if viewable_by_public}}` blocks resolve correctly based on the publish target, not just the entry's own audience list.

This enables a powerful pattern: an entry can contain content that only appears when published for a specific audience.

### Phase 3: WASM Bindings

#### 3.1 New command in `DiaryxBackend`

Add a `RenderTemplate` command to the WASM backend:

```rust
Command::RenderTemplate { path } => {
    let rendered = app.render_body_template(&path).await?;
    Response::Content(rendered)
}
```

#### 3.2 TypeScript API

Add to `api.ts`:

```typescript
async renderTemplate(path: string): Promise<string> {
    return await this.execute("RenderTemplate", { path });
}
```

### Phase 4: Web Editor UI

#### 4.1 Template Preview Mode

Two approaches (not mutually exclusive):

**A. Rendered preview panel** (simpler):
- Add a toggle button to switch between "edit" (raw template syntax) and "preview" (rendered output)
- Preview calls `api.renderTemplate(path)` and displays the result

**B. Inline rendering** (richer, more complex):
- Create TipTap extensions that understand template syntax and render inline

Recommended: start with **A**, add **B** incrementally.

#### 4.2 TipTap Extensions (Phase 4B)

Three new extensions following existing patterns:

##### TemplateVariableNode (inline)
- Matches `{{ varname }}` in markdown
- Renders as a pill/chip showing the resolved value
- Click to see variable name
- Markdown round-trip: preserves `{{ varname }}` syntax

##### TemplateEachBlock (block)
- Matches `{{#each list}}...{{/each}}`
- Renders the repeated content with resolved values
- Visual indicator showing it's a template block
- Collapsed/expanded view toggle

##### TemplateIfBlock (block)
- Matches `{{#if condition}}...{{/if}}`
- Shows/hides content based on condition resolution
- Visual indicator (border, background color) showing conditional content
- Optional: audience-aware coloring (e.g., "public only" gets a distinct style)

##### AudienceBlock (specialized wrapper)
- Sugar for `{{#if viewable_by_<audience>}}`
- UI: colored sidebar border indicating audience
- Toolbar button to wrap selected content in an audience block
- Audience selector dropdown

All extensions should follow the patterns documented in `apps/web/docs/tiptap-custom-extensions.md`:
- Always register tokenizers regardless of `enabled` state
- Use `.extend()` for `renderMarkdown`
- Support markdown round-trip

#### 4.3 Insertion UI Helpers

Add toolbar/menu items for:
- **Insert variable**: dropdown of available frontmatter properties + virtual props
- **Insert each block**: select an array property, insert `{{#each}}`/`{{/each}}` skeleton
- **Insert if block**: select a boolean/computed property, insert `{{#if}}`/`{{/if}}` skeleton
- **Wrap in audience block**: select audience tag, wrap selection in `{{#if viewable_by_<audience>}}`

### Phase 5: CLI Integration

#### 5.1 Preview Command

Add template rendering to the CLI `show`/`cat` command:

```bash
# Show raw content (existing behavior)
diaryx show entry.md

# Show rendered content (new flag)
diaryx show --rendered entry.md
```

#### 5.2 Publish Command

Template rendering is automatic during publish (Phase 2). No CLI changes needed beyond ensuring the feature flag is enabled.

### Phase 6: Update Existing Templates

#### 6.1 Built-in Template Enhancements

Consider updating built-in templates to demonstrate the new syntax:

```markdown
---
title: "{{title}}"
created: {{timestamp}}
audience: []
---

# {{title}}

```

Note: Creation-time `{{title}}` (resolved at create) and render-time `{{ title }}` (resolved at view) use the same syntax but are processed at different times. If both systems are active on the same file, creation-time runs first and resolves its variables, leaving any remaining `{{ }}` expressions for render-time.

To avoid ambiguity, consider using a different delimiter for render-time templates, e.g., `{{% title %}}` or `{{ =title }}`. However, this adds complexity and may not be worth it since creation-time templates are only processed once (at file creation) and their variables are always resolved.

#### 6.2 Documentation

- Update `crates/diaryx_core/src/template.rs` module docs to reference the new system
- Add template syntax reference to user documentation
- Update `apps/web/docs/tiptap-custom-extensions.md` with the new extension patterns

## Open Design Questions

### 1. Delimiter Collision

Both creation-time and render-time templates use `{{ }}`. Options:

- **A. No change** — Creation-time always resolves first; anything left is render-time. Works because creation-time variables (`{{timestamp}}`, `{{date}}`) don't overlap with frontmatter keys.
- **B. Different delimiters** — Render-time uses `{{% %}}` or `{{{ }}}`. More explicit but unfamiliar syntax.
- **C. Opt-in flag** — Add a frontmatter property `template: true` to enable render-time templating on a per-file basis.

**Recommendation**: Option A (no change) with Option C as a safety measure. Only render templates in files that have `template: true` or contain recognized template block syntax (`{{#each}}`, `{{#if}}`).

### 2. Escaping

How should users include literal `{{ }}` in their content? Handlebars uses `\{{ }}` for escaping. This should be documented.

### 3. Error Handling

What happens when a template references a nonexistent variable?

- **A. Silent empty string** — `{{ nonexistent }}` renders as `""`
- **B. Leave as-is** — `{{ nonexistent }}` renders as `{{ nonexistent }}`
- **C. Warning** — Render but show a warning in the editor

**Recommendation**: Option B (leave as-is) for graceful degradation. Files with template syntax are still readable even without the template engine.

### 4. Security

Handlebars supports custom helpers and partials. Should we limit what's available?

**Recommendation**: Start with a locked-down renderer — no custom helpers, no partials, no file includes. Only allow built-in Handlebars features (`#each`, `#if`, `#unless`, `#with`, `this`, `@index`, `@first`, `@last`). This prevents any template injection concerns.

### 5. Performance

Template rendering adds a processing step. Considerations:

- Cache rendered output per file hash + frontmatter hash
- Only render files that contain template syntax (quick regex check)
- For the editor: debounce re-rendering on frontmatter changes

### 6. CRDT Interaction

When using CRDT sync, template syntax is part of the document text. Rendering happens on each client independently. No CRDT changes needed since the raw template syntax is what gets synced, not the rendered output.

## Dependencies

| Crate | Version | Purpose | WASM-compatible |
|-------|---------|---------|-----------------|
| `handlebars` | 6.x | Template engine | Yes |

No other new dependencies required. The `handlebars` crate depends on `serde` and `serde_json`, which are already in the dependency tree.

## Implementation Order

The recommended implementation order, with each phase being independently shippable:

1. **Core engine** (body_template.rs) — Foundation
2. **Publishing** — Most immediate user value
3. **WASM bindings** — Required for web integration
4. **Editor preview toggle** — Quick win for web users
5. **CLI integration** — `--rendered` flag
6. **TipTap extensions** — Rich editing experience
7. **Insertion helpers** — Quality-of-life UI
8. **Template updates** — Polish

Each phase can be shipped independently. Phase 1-2 provides the core value. Phase 3-4 brings it to the web. Phase 5-8 is polish.
