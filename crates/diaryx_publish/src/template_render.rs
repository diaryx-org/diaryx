//! Render-time body templating using Handlebars.
//!
//! This is kept local to `diaryx_publish` so publish/export logic can be
//! published independently from `diaryx_templating`.

use std::path::Path;

use handlebars::{
    Context, Handlebars, Helper, HelperDef, HelperResult, Output, RenderContext, RenderError,
    RenderErrorReason, Renderable, ScopedJson,
};
use indexmap::IndexMap;
use serde_json::Value as JsonValue;
use serde_yaml::Value as YamlValue;

/// Render-time body template renderer.
pub struct BodyTemplateRenderer {
    handlebars: Handlebars<'static>,
}

impl BodyTemplateRenderer {
    /// Create a new renderer with custom helpers registered.
    pub fn new() -> Self {
        let mut handlebars = Handlebars::new();

        // We're producing markdown content, not escaped HTML.
        handlebars.register_escape_fn(handlebars::no_escape);
        handlebars.set_strict_mode(false);
        handlebars.register_helper("contains", Box::new(ContainsHelper));
        handlebars.register_helper("for-audience", Box::new(ForAudienceHelper));

        Self { handlebars }
    }

    /// Render template expressions in a body.
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

/// Fast-path check to skip rendering for plain markdown.
pub fn has_templates(body: &str) -> bool {
    body.contains("{{")
}

/// Build a JSON template context from frontmatter and file metadata.
pub fn build_context(
    frontmatter: &IndexMap<String, YamlValue>,
    file_path: &Path,
    workspace_root: Option<&Path>,
) -> JsonValue {
    let mut map = serde_json::Map::new();

    for (key, value) in frontmatter {
        map.insert(key.clone(), yaml_to_json(value));
    }

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

/// Build a context with an audience override for publish-time rendering.
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

/// `contains` helper — checks if an array contains a value.
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

/// `for-audience` block helper — sugar for audience-array checks.
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
