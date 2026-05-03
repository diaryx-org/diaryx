//! Shared types for import parsers and orchestration.

use diaryx_core::yaml;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// A single imported entry ready to be written to the workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedEntry {
    /// Entry title (e.g. email subject).
    pub title: String,
    /// Timestamp of the original item, if known.
    pub date: Option<chrono::DateTime<chrono::Utc>>,
    /// Markdown body content.
    pub body: String,
    /// Extra frontmatter fields (from, to, cc, etc.).
    pub metadata: IndexMap<String, yaml::Value>,
    /// Binary attachments extracted from the source.
    pub attachments: Vec<ImportedAttachment>,
}

/// A binary attachment extracted during import.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedAttachment {
    /// Suggested filename (from Content-Disposition or generated).
    pub filename: String,
    /// MIME content type.
    pub content_type: String,
    /// Raw file bytes.
    pub data: Vec<u8>,
}

/// Summary of an import operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    /// Number of entries successfully imported.
    pub imported: usize,
    /// Number of entries skipped (e.g. unparseable).
    pub skipped: usize,
    /// Human-readable error messages for skipped entries.
    pub errors: Vec<String>,
    /// Total number of attachments extracted.
    pub attachment_count: usize,
}
