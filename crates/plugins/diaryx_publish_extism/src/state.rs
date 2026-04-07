//! Thread-local state management for the publish Extism guest.
//!
//! WASM is single-threaded, so we use `thread_local!` + `RefCell` to store
//! the plugin state. All access is through `with_state()` / `with_state_mut()`.

use std::cell::RefCell;
use std::path::Path;
use std::sync::Arc;

use diaryx_core::yaml_value::YamlValue;
use indexmap::IndexMap;

use crate::publish::BodyRenderer;
use crate::publish_plugin::PublishPlugin;
use diaryx_plugin_sdk::host;

use crate::host_fs::HostFs;

/// Body renderer that delegates to the templating plugin via `host::plugins::call`.
struct PluginBodyRenderer;

impl BodyRenderer for PluginBodyRenderer {
    fn has_templates(&self, body: &str) -> bool {
        let params = serde_json::json!({ "body": body });
        host::plugins::call("diaryx.templating", "HasTemplates", params)
            .ok()
            .and_then(|value| value.as_bool())
            .unwrap_or_else(|| {
                body.contains("{{") || body.contains(":vis[") || body.contains(":::vis{")
            })
    }

    fn render_body(
        &self,
        body: &str,
        frontmatter: &IndexMap<String, YamlValue>,
        file_path: &Path,
        workspace_root: Option<&Path>,
        audience: Option<&str>,
    ) -> Result<String, String> {
        let mut params = serde_json::json!({
            "body": body,
            "frontmatter": frontmatter,
            "file_path": file_path.to_string_lossy(),
        });
        if let Some(root) = workspace_root {
            params["workspace_root"] = serde_json::Value::String(root.to_string_lossy().into());
        }
        if let Some(aud) = audience {
            params["audience"] = serde_json::Value::String(aud.into());
        }
        let result = host::plugins::call("diaryx.templating", "RenderBody", params)?;
        result
            .as_str()
            .map(String::from)
            .ok_or_else(|| "RenderBody did not return a string".into())
    }
}

/// State held by the publish plugin guest for the lifetime of the WASM instance.
pub struct PluginState {
    /// The inner publish plugin (handles export commands).
    pub publish_plugin: PublishPlugin<HostFs>,
}

thread_local! {
    static STATE: RefCell<Option<PluginState>> = const { RefCell::new(None) };
}

/// Initialize the plugin state.
pub fn init_state() -> Result<(), String> {
    STATE.with(|s| {
        let mut borrow = s.borrow_mut();
        if borrow.is_some() {
            return Ok(()); // Already initialized
        }
        let renderer: Arc<dyn BodyRenderer> = Arc::new(PluginBodyRenderer);
        let plugin = PublishPlugin::with_renderer(HostFs, renderer);
        *borrow = Some(PluginState {
            publish_plugin: plugin,
        });
        Ok(())
    })
}

/// Access plugin state immutably.
pub fn with_state<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&PluginState) -> R,
{
    STATE.with(|s| {
        let borrow = s.borrow();
        let state = borrow
            .as_ref()
            .ok_or_else(|| "Plugin state not initialized".to_string())?;
        Ok(f(state))
    })
}

/// Shut down the plugin state.
pub fn shutdown_state() -> Result<(), String> {
    STATE.with(|s| {
        let mut borrow = s.borrow_mut();
        *borrow = None;
        Ok(())
    })
}
