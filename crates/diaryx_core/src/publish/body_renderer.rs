//! Trait for render-time body template rendering.
//!
//! Decouples the publish pipeline from any specific template engine.
//! Implementations can delegate to an in-process Handlebars engine,
//! a plugin via `host_plugin_command`, or a noop passthrough.

use std::path::Path;

use indexmap::IndexMap;
use serde_yaml::Value as YamlValue;

/// Trait for rendering body templates at publish/export time.
pub trait BodyRenderer: Send + Sync {
    /// Fast-path check: does `body` contain template syntax?
    fn has_templates(&self, body: &str) -> bool;

    /// Render template expressions in `body` using frontmatter context.
    fn render_body(
        &self,
        body: &str,
        frontmatter: &IndexMap<String, YamlValue>,
        file_path: &Path,
        workspace_root: Option<&Path>,
        audience: Option<&str>,
    ) -> Result<String, String>;
}

/// No-op renderer that returns the body unchanged.
pub struct NoopBodyRenderer;

impl BodyRenderer for NoopBodyRenderer {
    fn has_templates(&self, _body: &str) -> bool {
        false
    }

    fn render_body(
        &self,
        body: &str,
        _frontmatter: &IndexMap<String, YamlValue>,
        _file_path: &Path,
        _workspace_root: Option<&Path>,
        _audience: Option<&str>,
    ) -> Result<String, String> {
        Ok(body.to_string())
    }
}
