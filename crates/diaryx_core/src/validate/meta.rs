//! Validation result wrappers with precomputed metadata for UI consumers.
//!
//! These wrappers let frontends (CLI, web, Tauri) render warnings and errors
//! without switching on the underlying enum variant. Every field below is
//! derived from the raw [`super::types::ValidationWarning`] /
//! [`super::types::ValidationError`] in the corresponding `From` impl.

use serde::{Deserialize, Serialize};

use super::detail::{error_detail, warning_detail};
use super::types::{ValidationError, ValidationResult, ValidationWarning};

/// A validation warning with computed metadata for frontend display.
///
/// `description` is a short header ("Missing backlink"), `detail` is a
/// one-line contextual summary ("note.md should list README.md in link_of"),
/// and `primary_path` is the workspace-relative path (or platform path if no
/// root is known) of the file most associated with the warning — it exists so
/// consumers can render "jump to file" UI without switching on variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct ValidationWarningWithMeta {
    /// The warning data
    #[serde(flatten)]
    pub warning: ValidationWarning,
    /// Short human-readable header (e.g. "Missing backlink").
    pub description: String,
    /// One-line contextual summary with concrete file/value info.
    pub detail: String,
    /// Primary file associated with this warning, if any. Platform path as a
    /// lossy UTF-8 string; consumers should treat it as opaque.
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub primary_path: Option<String>,
    /// Whether this warning can be auto-fixed
    pub can_auto_fix: bool,
    /// Whether the associated file can be viewed in editor
    pub is_viewable: bool,
    /// Whether this warning supports choosing a different parent
    pub supports_parent_picker: bool,
    /// Whether this warning should bubble up to the nearest ancestor index
    /// when rendered in a tree view (orphan-style warnings). UIs can filter
    /// on this instead of hardcoding a type list.
    pub inherits_to_parent: bool,
}

impl From<ValidationWarning> for ValidationWarningWithMeta {
    fn from(warning: ValidationWarning) -> Self {
        Self {
            description: warning.description().to_string(),
            detail: warning_detail(&warning),
            primary_path: warning
                .file_path()
                .map(|p| p.to_string_lossy().into_owned()),
            can_auto_fix: warning.can_auto_fix(),
            is_viewable: warning.is_viewable(),
            supports_parent_picker: warning.supports_parent_picker(),
            inherits_to_parent: warning.inherits_to_parent(),
            warning,
        }
    }
}

/// A validation error with computed metadata for frontend display.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct ValidationErrorWithMeta {
    /// The error data
    #[serde(flatten)]
    pub error: ValidationError,
    /// Short human-readable header (e.g. "Broken part_of reference").
    pub description: String,
    /// One-line contextual summary with concrete file/value info.
    pub detail: String,
    /// Primary file associated with this error. Platform path as a lossy
    /// UTF-8 string; consumers should treat it as opaque.
    pub primary_path: String,
}

impl From<ValidationError> for ValidationErrorWithMeta {
    fn from(error: ValidationError) -> Self {
        Self {
            description: error.description().to_string(),
            detail: error_detail(&error),
            primary_path: error.file_path().to_string_lossy().into_owned(),
            error,
        }
    }
}

/// Validation result with computed metadata for frontend display.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct ValidationResultWithMeta {
    /// Validation errors with metadata
    pub errors: Vec<ValidationErrorWithMeta>,
    /// Validation warnings with metadata
    pub warnings: Vec<ValidationWarningWithMeta>,
    /// Number of files checked
    pub files_checked: usize,
}

impl From<ValidationResult> for ValidationResultWithMeta {
    fn from(result: ValidationResult) -> Self {
        result.with_metadata()
    }
}
