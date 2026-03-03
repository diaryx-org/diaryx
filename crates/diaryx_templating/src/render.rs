//! Render-time body templating using Handlebars.
//!
//! This module provides template rendering for entry bodies at view/publish time.
//! It is separate from the creation-time [`creation`](crate::creation) module:
//!
//! - **Creation-time** (`creation.rs`): Runs once when creating an entry. Operates on
//!   template files. Variables are date/time/title. Syntax is resolved and removed.
//! - **Render-time** (this module): Runs on every view/publish. Operates on entry files.
//!   Variables come from frontmatter. Raw `{{ }}` syntax is preserved in the file.
//!
//! ## Custom Helpers
//!
//! Two custom helpers are registered:
//!
//! - `contains` — Array membership test: `{{#if (contains audience "public")}}`
//! - `for-audience` — Sugar for `{{#if (contains audience "<value>")}}`

use std::path::Path;

use handlebars::{
    Context, Handlebars, Helper, HelperDef, HelperResult, Output, RenderContext, RenderError,
    RenderErrorReason, Renderable, ScopedJson,
};
use indexmap::IndexMap;
use serde_json::Value as JsonValue;
use serde_yaml::Value as YamlValue;

/// Render-time body template renderer.
///
/// Wraps a configured [`Handlebars`] instance with custom helpers registered.
pub struct BodyTemplateRenderer {
    handlebars: Handlebars<'static>,
}

impl BodyTemplateRenderer {
    /// Create a new renderer with custom helpers registered.
    pub fn new() -> Self {
        let mut handlebars = Handlebars::new();

        // Don't escape HTML — we're producing markdown, not HTML
        handlebars.register_escape_fn(handlebars::no_escape);

        // Strict mode off: missing variables render as empty string
        handlebars.set_strict_mode(false);

        handlebars.register_helper("contains", Box::new(ContainsHelper));
        handlebars.register_helper("for-audience", Box::new(ForAudienceHelper));

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
    body.contains("{{")
}

/// Build a JSON template context from frontmatter and file metadata.
///
/// All frontmatter key-value pairs become template variables. Virtual properties
/// (`filename`, `filepath`, `extension`) are added from file metadata.
pub fn build_context(
    frontmatter: &IndexMap<String, YamlValue>,
    file_path: &Path,
    workspace_root: Option<&Path>,
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

    JsonValue::Object(map)
}

/// Build a template context with an audience override for publish-time rendering.
///
/// When publishing for a specific audience, this replaces the entry's `audience`
/// array with just the target audience, so `{{#if (contains audience "public")}}`
/// resolves based on the publish target.
pub fn build_publish_context(
    frontmatter: &IndexMap<String, YamlValue>,
    file_path: &Path,
    workspace_root: Option<&Path>,
    target_audience: &str,
) -> JsonValue {
    let mut context = build_context(frontmatter, file_path, workspace_root);

    if let JsonValue::Object(ref mut map) = context {
        map.insert(
            "audience".to_string(),
            JsonValue::Array(vec![JsonValue::String(target_audience.to_string())]),
        );
    }

    context
}

/// One-shot render: build context and render body in one call.
pub fn render(
    body: &str,
    frontmatter: &IndexMap<String, YamlValue>,
    file_path: &Path,
    workspace_root: Option<&Path>,
) -> Result<String, String> {
    let renderer = BodyTemplateRenderer::new();
    let context = build_context(frontmatter, file_path, workspace_root);
    renderer.render(body, &context)
}

/// Convert a `serde_yaml::Value` to a `serde_json::Value`.
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
                        other => serde_yaml::to_string(other).ok()?.trim().to_string(),
                    };
                    Some((key, yaml_to_json(v)))
                })
                .collect();
            JsonValue::Object(obj)
        }
        YamlValue::Tagged(tagged) => yaml_to_json(&tagged.value),
    }
}

// ---------------------------------------------------------------------------
// Custom Helpers
// ---------------------------------------------------------------------------

/// `contains` helper — checks if an array contains a value.
///
/// Used as a subexpression: `{{#if (contains audience "public")}}`.
/// Returns a boolean JSON value.
#[derive(Clone, Copy)]
struct ContainsHelper;

impl HelperDef for ContainsHelper {
    fn call_inner<'reg: 'rc, 'rc>(
        &self,
        h: &Helper<'rc>,
        _r: &'reg Handlebars<'reg>,
        _ctx: &'rc Context,
        _rc: &mut RenderContext<'reg, 'rc>,
    ) -> std::result::Result<ScopedJson<'rc>, RenderError> {
        let array = h
            .param(0)
            .ok_or(RenderErrorReason::ParamNotFoundForIndex("contains", 0))?
            .value();
        let needle = h
            .param(1)
            .ok_or(RenderErrorReason::ParamNotFoundForIndex("contains", 1))?
            .value();

        let result = match array {
            JsonValue::Array(arr) => arr.contains(needle),
            _ => false,
        };

        Ok(ScopedJson::Derived(JsonValue::Bool(result)))
    }
}

/// `for-audience` block helper — sugar for `{{#if (contains audience "<value>")}}`.
///
/// Usage: `{{#for-audience "public"}}...{{/for-audience}}`
#[derive(Clone, Copy)]
struct ForAudienceHelper;

impl HelperDef for ForAudienceHelper {
    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper<'rc>,
        r: &'reg Handlebars<'reg>,
        ctx: &'rc Context,
        rc: &mut RenderContext<'reg, 'rc>,
        out: &mut dyn Output,
    ) -> HelperResult {
        let target = h
            .param(0)
            .ok_or(RenderErrorReason::ParamNotFoundForIndex("for-audience", 0))?
            .value()
            .as_str()
            .ok_or_else(|| {
                RenderErrorReason::ParamTypeMismatchForName(
                    "for-audience",
                    "0".to_string(),
                    "string".to_string(),
                )
            })?;

        // Look up `audience` from the current context
        let audience = ctx.data().get("audience");

        let matches = match audience {
            Some(JsonValue::Array(arr)) => arr.contains(&JsonValue::String(target.to_string())),
            _ => false,
        };

        let tmpl = if matches { h.template() } else { h.inverse() };

        if let Some(t) = tmpl {
            t.render(r, ctx, rc, out)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_frontmatter(yaml: &str) -> IndexMap<String, YamlValue> {
        serde_yaml::from_str(yaml).unwrap()
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
    fn test_contains_helper_match() {
        let fm = make_frontmatter(
            r#"
audience:
  - friends
  - public
"#,
        );
        let body = "{{#if (contains audience \"public\")}}PUBLIC{{/if}}";
        let result = render(body, &fm, Path::new("test.md"), None).unwrap();
        assert_eq!(result, "PUBLIC");
    }

    #[test]
    fn test_contains_helper_no_match() {
        let fm = make_frontmatter(
            r#"
audience:
  - friends
"#,
        );
        let body = "{{#if (contains audience \"public\")}}PUBLIC{{/if}}";
        let result = render(body, &fm, Path::new("test.md"), None).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_contains_helper_non_array() {
        let fm = make_frontmatter("audience: public");
        let body = "{{#if (contains audience \"public\")}}PUBLIC{{/if}}";
        let result = render(body, &fm, Path::new("test.md"), None).unwrap();
        // Non-array always returns false
        assert_eq!(result, "");
    }

    #[test]
    fn test_for_audience_helper_match() {
        let fm = make_frontmatter(
            r#"
audience:
  - friends
  - public
"#,
        );
        let body = "{{#for-audience \"public\"}}PUBLIC{{/for-audience}}";
        let result = render(body, &fm, Path::new("test.md"), None).unwrap();
        assert_eq!(result, "PUBLIC");
    }

    #[test]
    fn test_for_audience_helper_no_match() {
        let fm = make_frontmatter(
            r#"
audience:
  - friends
"#,
        );
        let body = "{{#for-audience \"public\"}}PUBLIC{{/for-audience}}";
        let result = render(body, &fm, Path::new("test.md"), None).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_for_audience_helper_else() {
        let fm = make_frontmatter(
            r#"
audience:
  - friends
"#,
        );
        let body = "{{#for-audience \"public\"}}PUBLIC{{else}}PRIVATE{{/for-audience}}";
        let result = render(body, &fm, Path::new("test.md"), None).unwrap();
        assert_eq!(result, "PRIVATE");
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
        assert!(has_templates("{{#each items}}{{this}}{{/each}}"));
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
    fn test_build_publish_context_overrides_audience() {
        let fm = make_frontmatter(
            r#"
audience:
  - friends
  - family
  - public
"#,
        );
        let ctx = build_publish_context(&fm, Path::new("test.md"), None, "public");
        let audience = ctx.get("audience").unwrap();
        assert_eq!(
            audience,
            &JsonValue::Array(vec![JsonValue::String("public".to_string())])
        );
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
        let ctx = build_context(&fm, Path::new("test.md"), None);
        assert_eq!(ctx.get("string_val").unwrap(), "hello");
        assert_eq!(ctx.get("number_val").unwrap(), 42);
        assert_eq!(ctx.get("bool_val").unwrap(), true);
        assert!(ctx.get("null_val").unwrap().is_null());
        assert!(ctx.get("list_val").unwrap().is_array());
        assert!(ctx.get("map_val").unwrap().is_object());
    }

    #[test]
    fn test_full_example() {
        let fm = make_frontmatter(
            r#"
title: Hello World
audience:
  - friends
  - public
links:
  - "[Link 1](https://link1.com)"
  - "[Link 2](https://link2.com)"
"#,
        );
        let body = r#"# {{ title }}

{{#each links}}
- {{this}}
{{/each}}

{{#if (contains audience "public")}}
Hello public!
{{/if}}"#;

        let result = render(body, &fm, Path::new("hello.md"), None).unwrap();
        assert!(result.contains("# Hello World"));
        assert!(result.contains("[Link 1](https://link1.com)"));
        assert!(result.contains("[Link 2](https://link2.com)"));
        assert!(result.contains("Hello public!"));
    }
}
