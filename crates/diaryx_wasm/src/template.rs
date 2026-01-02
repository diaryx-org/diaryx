//! Template operations for WASM.

use std::path::{Path, PathBuf};

use diaryx_core::fs::FileSystem;
use diaryx_core::template::TemplateManager;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::error::IntoJsResult;
use crate::state::{with_fs, with_fs_mut};

// ============================================================================
// Types
// ============================================================================

/// Template info returned to JavaScript
#[derive(Debug, Serialize)]
pub struct JsTemplateInfo {
    pub name: String,
    pub path: String,
    pub source: String,
}

// ============================================================================
// DiaryxTemplate Class
// ============================================================================

/// Template operations for managing entry templates.
#[wasm_bindgen]
pub struct DiaryxTemplate;

#[wasm_bindgen]
impl DiaryxTemplate {
    /// Create a new DiaryxTemplate instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// List available templates.
    #[wasm_bindgen]
    pub fn list(&self, workspace_path: Option<String>) -> Result<JsValue, JsValue> {
        with_fs(|fs| {
            let mut manager = TemplateManager::new(fs);

            if let Some(ref ws_path) = workspace_path {
                manager = manager.with_workspace_dir(Path::new(ws_path));
            }

            let templates: Vec<JsTemplateInfo> = manager
                .list()
                .into_iter()
                .map(|t| JsTemplateInfo {
                    name: t.name,
                    path: t
                        .path
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    source: format!("{}", t.source),
                })
                .collect();

            serde_wasm_bindgen::to_value(&templates).js_err()
        })
    }

    /// Get a template's content.
    #[wasm_bindgen]
    pub fn get(&self, name: &str, workspace_path: Option<String>) -> Result<String, JsValue> {
        with_fs(|fs| {
            let mut manager = TemplateManager::new(fs);

            if let Some(ref ws_path) = workspace_path {
                manager = manager.with_workspace_dir(Path::new(ws_path));
            }

            manager
                .get(name)
                .map(|t| t.raw_content.clone())
                .ok_or_else(|| JsValue::from_str(&format!("Template not found: {}", name)))
        })
    }

    /// Save a user template.
    #[wasm_bindgen]
    pub fn save(&self, name: &str, content: &str, workspace_path: &str) -> Result<(), JsValue> {
        with_fs_mut(|fs| {
            let templates_dir = PathBuf::from(workspace_path).join("_templates");

            fs.create_dir_all(&templates_dir).js_err()?;

            let template_path = templates_dir.join(format!("{}.md", name));
            fs.write_file(&template_path, content).js_err()
        })
    }

    /// Delete a user template.
    #[wasm_bindgen]
    pub fn delete(&self, name: &str, workspace_path: &str) -> Result<(), JsValue> {
        with_fs_mut(|fs| {
            let template_path = PathBuf::from(workspace_path)
                .join("_templates")
                .join(format!("{}.md", name));

            fs.delete_file(&template_path).js_err()
        })
    }
}

impl Default for DiaryxTemplate {
    fn default() -> Self {
        Self::new()
    }
}
