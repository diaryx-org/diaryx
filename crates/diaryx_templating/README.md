# diaryx_templating

Shared templating domain logic used by the Templating plugin implementation.

This crate is host-agnostic (no `diaryx_core` dependency) and contains:

## Creation-time templating (`creation` module)

- `Template` struct with `render()` and `render_parsed()` for `{{variable}}` substitution
- `TemplateContext` with builder methods for template variable resolution
- `TemplateInfo` and `TemplateSource` types for listing available templates
- Built-in templates: `DEFAULT_NOTE_TEMPLATE`, `DEFAULT_DAILY_TEMPLATE`
- `TEMPLATE_VARIABLES` constant listing supported variables
- `substitute_variables()` / `parse_rendered_template()` helpers

## Render-time templating (`render` module)

- `BodyTemplateRenderer` — Handlebars-based engine for runtime body rendering
- `has_templates()` — fast-path check for `{{` in body content
- `build_context()` / `build_publish_context()` — build Handlebars context from frontmatter
- Custom Handlebars helpers: `contains`, `for-audience`
- `yaml_to_json()` helper for converting YAML frontmatter to JSON values
