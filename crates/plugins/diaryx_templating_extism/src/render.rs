//! Render-time body templating using Handlebars plus visibility-directive filtering.
//!
//! This module provides template rendering for entry bodies at view/publish time.
//! It is separate from the creation-time [`creation`](crate::creation) module:
//!
//! - **Creation-time** (`creation.rs`): Runs once when creating an entry. Operates on
//!   template files. Variables are date/time/title. Syntax is resolved and removed.
//! - **Render-time** (this module): Runs on every view/publish. Operates on entry files.
//!   Variables come from frontmatter. Raw `{{ }}` syntax is preserved in the file.
//!
//! When a publish target audience is supplied, this module also strips or keeps
//! `:vis[...]` and `:::vis{...}` directives before running Handlebars interpolation.

use std::path::Path;

use diaryx_core::visibility;
use handlebars::Handlebars;
use indexmap::IndexMap;
use serde_json::Value as JsonValue;
use serde_yaml_ng::Value as YamlValue;

/// Render-time body template renderer.
///
/// Wraps a configured [`Handlebars`] instance for frontmatter-driven rendering.
pub struct BodyTemplateRenderer {
    handlebars: Handlebars<'static>,
}

impl BodyTemplateRenderer {
    /// Create a new renderer.
    pub fn new() -> Self {
        let mut handlebars = Handlebars::new();

        // Don't escape HTML — we're producing markdown, not HTML
        handlebars.register_escape_fn(handlebars::no_escape);

        // Strict mode off: missing variables render as empty string
        handlebars.set_strict_mode(false);

        Self { handlebars }
    }

    /// Render template expressions in an entry body.
    ///
    /// Returns the rendered body with all `{{ }}` expressions resolved.
    pub fn render(&self, body: &str, context: &JsonValue) -> Result<String, String> {
        self.handlebars
            .render_template(body, context)
            .map_err(|e| format!("Template render error: {e}"))
    }
}

impl Default for BodyTemplateRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Check whether a body contains template expressions worth rendering.
///
/// This is a fast-path check to skip the Handlebars engine for plain markdown.
pub fn has_templates(body: &str) -> bool {
    has_handlebars_templates(body) || visibility::has_visibility_directives(body)
}

/// Fast-path check for Handlebars syntax specifically.
pub fn has_handlebars_templates(body: &str) -> bool {
    body.contains("{{")
}

/// Build a JSON template context from frontmatter and file metadata.
///
/// All frontmatter key-value pairs become template variables. Virtual properties
/// (`filename`, `filepath`, `extension`) are added from file metadata.
///
/// When `viewer_audiences` is non-empty, the context also exposes:
/// - `viewer_audience` — comma-joined string for direct interpolation
/// - `viewer_audiences` — array for `{{#each}}` iteration
pub fn build_context(
    frontmatter: &IndexMap<String, YamlValue>,
    file_path: &Path,
    workspace_root: Option<&Path>,
    viewer_audiences: &[&str],
) -> JsonValue {
    let mut map = serde_json::Map::new();

    // Convert all frontmatter values to JSON
    for (key, value) in frontmatter {
        map.insert(key.clone(), yaml_to_json(value));
    }

    // Virtual properties
    if let Some(stem) = file_path.file_stem().and_then(|s| s.to_str()) {
        map.insert("filename".to_string(), JsonValue::String(stem.to_string()));
    }

    if let Some(ext) = file_path.extension().and_then(|s| s.to_str()) {
        map.insert("extension".to_string(), JsonValue::String(ext.to_string()));
    }

    let filepath = if let Some(root) = workspace_root {
        file_path
            .strip_prefix(root)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string()
    } else {
        file_path.to_string_lossy().to_string()
    };
    map.insert("filepath".to_string(), JsonValue::String(filepath));

    if !viewer_audiences.is_empty() {
        map.insert(
            "viewer_audience".to_string(),
            JsonValue::String(viewer_audiences.join(", ")),
        );
        map.insert(
            "viewer_audiences".to_string(),
            JsonValue::Array(
                viewer_audiences
                    .iter()
                    .map(|a| JsonValue::String((*a).to_string()))
                    .collect(),
            ),
        );
    }

    JsonValue::Object(map)
}

/// One-shot render: build context and render body in one call.
pub fn render(
    body: &str,
    frontmatter: &IndexMap<String, YamlValue>,
    file_path: &Path,
    workspace_root: Option<&Path>,
) -> Result<String, String> {
    let preprocessed = visibility::strip_visibility_directives(body);
    let renderer = BodyTemplateRenderer::new();
    let context = build_context(frontmatter, file_path, workspace_root, &[]);
    if has_handlebars_templates(&preprocessed) {
        renderer.render(&preprocessed, &context)
    } else {
        Ok(preprocessed)
    }
}

/// Audience-aware render: first apply visibility directives, then interpolate
/// any remaining Handlebars variables from frontmatter.
pub fn render_for_audience(
    body: &str,
    frontmatter: &IndexMap<String, YamlValue>,
    file_path: &Path,
    workspace_root: Option<&Path>,
    target_audience: &str,
) -> Result<String, String> {
    render_for_audiences(
        body,
        frontmatter,
        file_path,
        workspace_root,
        &[target_audience],
    )
}

/// Multi-audience render: filters visibility directives against any of the
/// supplied viewer audiences and exposes them via the `viewer_audience(s)`
/// template variables.
pub fn render_for_audiences(
    body: &str,
    frontmatter: &IndexMap<String, YamlValue>,
    file_path: &Path,
    workspace_root: Option<&Path>,
    viewer_audiences: &[&str],
) -> Result<String, String> {
    let filtered = visibility::filter_body_for_audiences(body, viewer_audiences);
    let renderer = BodyTemplateRenderer::new();
    let context = build_context(frontmatter, file_path, workspace_root, viewer_audiences);

    if has_handlebars_templates(&filtered) {
        renderer.render(&filtered, &context)
    } else {
        Ok(filtered)
    }
}

/// Convert a `serde_yaml_ng::Value` to a `serde_json::Value`.
pub fn yaml_to_json(value: &YamlValue) -> JsonValue {
    match value {
        YamlValue::Null => JsonValue::Null,
        YamlValue::Bool(b) => JsonValue::Bool(*b),
        YamlValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                JsonValue::Number(i.into())
            } else if let Some(u) = n.as_u64() {
                JsonValue::Number(u.into())
            } else if let Some(f) = n.as_f64() {
                serde_json::Number::from_f64(f)
                    .map(JsonValue::Number)
                    .unwrap_or(JsonValue::Null)
            } else {
                JsonValue::Null
            }
        }
        YamlValue::String(s) => JsonValue::String(s.clone()),
        YamlValue::Sequence(seq) => JsonValue::Array(seq.iter().map(yaml_to_json).collect()),
        YamlValue::Mapping(map) => {
            let obj: serde_json::Map<String, JsonValue> = map
                .iter()
                .filter_map(|(k, v)| {
                    let key = match k {
                        YamlValue::String(s) => s.clone(),
                        other => serde_yaml_ng::to_string(other).ok()?.trim().to_string(),
                    };
                    Some((key, yaml_to_json(v)))
                })
                .collect();
            JsonValue::Object(obj)
        }
        YamlValue::Tagged(tagged) => yaml_to_json(&tagged.value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_frontmatter(yaml: &str) -> IndexMap<String, YamlValue> {
        serde_yaml_ng::from_str(yaml).unwrap()
    }

    #[test]
    fn test_simple_variable() {
        let fm = make_frontmatter("title: Hello World");
        let body = "# {{ title }}";
        let result = render(body, &fm, Path::new("test.md"), None).unwrap();
        assert_eq!(result, "# Hello World");
    }

    #[test]
    fn test_missing_variable_renders_empty() {
        let fm = make_frontmatter("title: Hello");
        let body = "Author: {{ author }}";
        let result = render(body, &fm, Path::new("test.md"), None).unwrap();
        assert_eq!(result, "Author: ");
    }

    #[test]
    fn test_each_block() {
        let fm = make_frontmatter(
            r#"
links:
  - one
  - two
  - three
"#,
        );
        let body = "{{#each links}}{{this}}\n{{/each}}";
        let result = render(body, &fm, Path::new("test.md"), None).unwrap();
        assert_eq!(result, "one\ntwo\nthree\n");
    }

    #[test]
    fn test_if_block() {
        let fm = make_frontmatter("draft: true");
        let body = "{{#if draft}}DRAFT{{/if}}";
        let result = render(body, &fm, Path::new("test.md"), None).unwrap();
        assert_eq!(result, "DRAFT");
    }

    #[test]
    fn test_if_block_false() {
        let fm = make_frontmatter("draft: false");
        let body = "{{#if draft}}DRAFT{{/if}}";
        let result = render(body, &fm, Path::new("test.md"), None).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_inline_visibility_directive_match() {
        let fm = make_frontmatter(
            r#"
title: Hello
"#,
        );
        let body = "Before :vis[{{ title }}]{public} after";
        let result = render_for_audience(body, &fm, Path::new("test.md"), None, "public").unwrap();
        assert_eq!(result, "Before Hello after");
    }

    #[test]
    fn test_inline_visibility_directive_no_match() {
        let fm = make_frontmatter("title: Hello");
        let body = "Before :vis[{{ title }}]{public} after";
        let result = render_for_audience(body, &fm, Path::new("test.md"), None, "friends").unwrap();
        assert_eq!(result, "Before  after");
    }

    #[test]
    fn test_block_visibility_directive_match() {
        let fm = make_frontmatter("title: Hello");
        let body = "Intro\n:::vis{public}\n{{ title }}\n:::\nOutro";
        let result = render_for_audience(body, &fm, Path::new("test.md"), None, "public").unwrap();
        assert_eq!(result, "Intro\nHello\nOutro");
    }

    #[test]
    fn test_block_visibility_directive_no_match() {
        let fm = make_frontmatter("title: Hello");
        let body = "Intro\n:::vis{public}\n{{ title }}\n:::\nOutro";
        let result = render_for_audience(body, &fm, Path::new("test.md"), None, "friends").unwrap();
        assert_eq!(result, "Intro\nOutro");
    }

    #[test]
    fn test_nested_visibility_directives() {
        let fm = make_frontmatter("title: Hello");
        let body = "A :vis[outer :vis[inner]{public} end]{public}";
        let result = render_for_audience(body, &fm, Path::new("test.md"), None, "public").unwrap();
        assert_eq!(result, "A outer inner end");
    }

    #[test]
    fn test_visibility_directives_are_stripped_without_audience_filter() {
        let fm = make_frontmatter("title: Hello");
        let body = "Before :vis[{{ title }}]{public} after";
        let result = render(body, &fm, Path::new("test.md"), None).unwrap();
        assert_eq!(result, "Before Hello after");
    }

    #[test]
    fn test_virtual_property_filename() {
        let fm = make_frontmatter("title: Test");
        let body = "File: {{ filename }}";
        let result = render(body, &fm, Path::new("notes/hello-world.md"), None).unwrap();
        assert_eq!(result, "File: hello-world");
    }

    #[test]
    fn test_virtual_property_filepath() {
        let fm = make_frontmatter("title: Test");
        let body = "Path: {{ filepath }}";
        let result = render(
            body,
            &fm,
            Path::new("/workspace/notes/hello.md"),
            Some(Path::new("/workspace")),
        )
        .unwrap();
        assert_eq!(result, "Path: notes/hello.md");
    }

    #[test]
    fn test_virtual_property_extension() {
        let fm = make_frontmatter("title: Test");
        let body = "Ext: {{ extension }}";
        let result = render(body, &fm, Path::new("test.md"), None).unwrap();
        assert_eq!(result, "Ext: md");
    }

    #[test]
    fn test_has_templates() {
        assert!(has_templates("Hello {{ title }}"));
        assert!(has_templates(":vis[Hello]{public}"));
        assert!(has_templates(":::vis{public}\nHello\n:::\n"));
        assert!(!has_templates("Hello World"));
        assert!(!has_templates("No templates here"));
    }

    #[test]
    fn test_nested_blocks() {
        let fm = make_frontmatter(
            r#"
show: true
items:
  - a
  - b
"#,
        );
        let body = "{{#if show}}{{#each items}}{{this}}{{/each}}{{/if}}";
        let result = render(body, &fm, Path::new("test.md"), None).unwrap();
        assert_eq!(result, "ab");
    }

    #[test]
    fn test_yaml_to_json_types() {
        let fm = make_frontmatter(
            r#"
string_val: hello
number_val: 42
bool_val: true
null_val: null
list_val:
  - a
  - b
map_val:
  key: value
"#,
        );
        let ctx = build_context(&fm, Path::new("test.md"), None, &[]);
        assert_eq!(ctx.get("string_val").unwrap(), "hello");
        assert_eq!(ctx.get("number_val").unwrap(), 42);
        assert_eq!(ctx.get("bool_val").unwrap(), true);
        assert!(ctx.get("null_val").unwrap().is_null());
        assert!(ctx.get("list_val").unwrap().is_array());
        assert!(ctx.get("map_val").unwrap().is_object());
    }

    #[test]
    fn test_viewer_audience_variable_single() {
        let fm = make_frontmatter("title: Hi");
        let body = "Hello, {{ viewer_audience }}!";
        let result = render_for_audience(body, &fm, Path::new("test.md"), None, "family").unwrap();
        assert_eq!(result, "Hello, family!");
    }

    #[test]
    fn test_viewer_audience_variable_multi() {
        let fm = make_frontmatter("title: Hi");
        let body = "Hello, {{ viewer_audience }}!";
        let result = render_for_audiences(
            body,
            &fm,
            Path::new("test.md"),
            None,
            &["family", "friends"],
        )
        .unwrap();
        assert_eq!(result, "Hello, family, friends!");
    }

    #[test]
    fn test_viewer_audiences_each_block() {
        let fm = make_frontmatter("title: Hi");
        let body = "{{#each viewer_audiences}}- {{this}}\n{{/each}}";
        let result = render_for_audiences(
            body,
            &fm,
            Path::new("test.md"),
            None,
            &["family", "friends"],
        )
        .unwrap();
        assert_eq!(result, "- family\n- friends\n");
    }

    #[test]
    fn test_viewer_audience_empty_when_no_audience() {
        let fm = make_frontmatter("title: Hi");
        let body = "[{{ viewer_audience }}]";
        let result = render(body, &fm, Path::new("test.md"), None).unwrap();
        assert_eq!(result, "[]");
    }

    #[test]
    fn test_multi_audience_visibility_filter() {
        let fm = make_frontmatter("title: Hi");
        let body = ":vis[family-only]{family} :vis[friends-only]{friends}";
        let result = render_for_audiences(
            body,
            &fm,
            Path::new("test.md"),
            None,
            &["family", "friends"],
        )
        .unwrap();
        assert_eq!(result, "family-only friends-only");
    }

    #[test]
    fn test_full_example() {
        let fm = make_frontmatter(
            r#"
title: Hello World
links:
  - "[Link 1](https://link1.com)"
  - "[Link 2](https://link2.com)"
"#,
        );
        let body = r#"# {{ title }}

{{#each links}}
- {{this}}
{{/each}}

 :vis[Hello public!]{public}"#;

        let result = render_for_audience(body, &fm, Path::new("hello.md"), None, "public").unwrap();
        assert!(result.contains("# Hello World"));
        assert!(result.contains("[Link 1](https://link1.com)"));
        assert!(result.contains("[Link 2](https://link2.com)"));
        assert!(result.contains("Hello public!"));
    }
}
