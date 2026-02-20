//! Import external formats into Diaryx entries.
//!
//! This module provides parsers that convert external data formats (emails, etc.)
//! into [`ImportedEntry`] values. The parsers are pure functions — they do no
//! filesystem I/O — so callers (CLI, WASM, etc.) decide how to persist results.
//!
//! The [`orchestrate`] submodule provides async orchestration for writing parsed
//! entries into a workspace, building the date-based folder hierarchy with proper
//! `part_of`/`contents` links.
//!
//! # Feature flags
//!
//! Each format lives behind its own feature flag:
//!
//! | Format   | Feature          | Crate dependencies                                |
//! |----------|------------------|---------------------------------------------------|
//! | Email    | `import-email`   | `mailparse`, `mbox-reader`, `html-to-markdown-rs` |
//! | Day One  | `import-dayone`  | *(none — pure JSON parsing)*                      |
//! | Markdown | `import-markdown`| *(none — uses `frontmatter` crate module)*        |

#[cfg(feature = "import-email")]
pub mod email;

#[cfg(feature = "import-dayone")]
pub mod dayone;

#[cfg(feature = "import-markdown")]
pub mod markdown;

pub mod orchestrate;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

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
    pub metadata: IndexMap<String, serde_yaml::Value>,
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

/// Options controlling import behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportOptions {
    /// Base folder name for imported entries (default: `"emails"`).
    pub base_folder: String,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            base_folder: "emails".to_string(),
        }
    }
}

/// Summary of an import operation.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
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
