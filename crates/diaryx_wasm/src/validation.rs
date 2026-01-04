//! Validation operations for WASM.

use std::path::PathBuf;

use diaryx_core::validate::Validator;
use diaryx_core::workspace::Workspace;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::error::IntoJsResult;
use crate::state::with_fs;

// ============================================================================
// Types
// ============================================================================

/// Validation error returned to JavaScript
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum JsValidationError {
    BrokenPartOf { file: String, target: String },
    BrokenContentsRef { index: String, target: String },
}

impl From<diaryx_core::validate::ValidationError> for JsValidationError {
    fn from(err: diaryx_core::validate::ValidationError) -> Self {
        use diaryx_core::validate::ValidationError;
        match err {
            ValidationError::BrokenPartOf { file, target } => JsValidationError::BrokenPartOf {
                file: file.to_string_lossy().to_string(),
                target,
            },
            ValidationError::BrokenContentsRef { index, target } => {
                JsValidationError::BrokenContentsRef {
                    index: index.to_string_lossy().to_string(),
                    target,
                }
            }
        }
    }
}

/// Validation warning returned to JavaScript
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum JsValidationWarning {
    OrphanFile { file: String },
    CircularReference { files: Vec<String> },
    UnlinkedEntry { path: String, is_dir: bool },
}

impl From<diaryx_core::validate::ValidationWarning> for JsValidationWarning {
    fn from(warn: diaryx_core::validate::ValidationWarning) -> Self {
        use diaryx_core::validate::ValidationWarning;
        match warn {
            ValidationWarning::OrphanFile { file } => JsValidationWarning::OrphanFile {
                file: file.to_string_lossy().to_string(),
            },
            ValidationWarning::CircularReference { files } => {
                JsValidationWarning::CircularReference {
                    files: files
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect(),
                }
            }
            ValidationWarning::UnlinkedEntry { path, is_dir } => {
                JsValidationWarning::UnlinkedEntry {
                    path: path.to_string_lossy().to_string(),
                    is_dir,
                }
            }
        }
    }
}

/// Validation result returned to JavaScript
#[derive(Debug, Serialize)]
pub struct JsValidationResult {
    pub errors: Vec<JsValidationError>,
    pub warnings: Vec<JsValidationWarning>,
    pub files_checked: usize,
}

// ============================================================================
// DiaryxValidation Class
// ============================================================================

/// Validation operations for checking workspace integrity.
#[wasm_bindgen]
pub struct DiaryxValidation;

#[wasm_bindgen]
impl DiaryxValidation {
    /// Create a new DiaryxValidation instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// Validate workspace links.
    #[wasm_bindgen]
    pub fn validate(&self, workspace_path: &str) -> Result<JsValue, JsValue> {
        with_fs(|fs| {
            let validator = Validator::new(fs);
            let root_path = PathBuf::from(workspace_path);

            let ws = Workspace::new(fs);
            let root_index = ws
                .find_root_index_in_dir(&root_path)
                .js_err()?
                .or_else(|| ws.find_any_index_in_dir(&root_path).ok().flatten())
                .ok_or_else(|| {
                    JsValue::from_str(&format!("No workspace found at '{}'", workspace_path))
                })?;

            let result = validator.validate_workspace(&root_index).js_err()?;

            let js_result = JsValidationResult {
                errors: result
                    .errors
                    .into_iter()
                    .map(JsValidationError::from)
                    .collect(),
                warnings: result
                    .warnings
                    .into_iter()
                    .map(JsValidationWarning::from)
                    .collect(),
                files_checked: result.files_checked,
            };

            serde_wasm_bindgen::to_value(&js_result).js_err()
        })
    }
}

impl Default for DiaryxValidation {
    fn default() -> Self {
        Self::new()
    }
}
