//! Publishing data types.
//!
//! This module contains the core data types for publishing operations.

use std::path::PathBuf;

use serde::Serialize;

/// Options for publishing
#[derive(Debug, Clone, Serialize)]
pub struct PublishOptions {
    /// Output as a single HTML file instead of multiple files
    pub single_file: bool,
    /// Site title (defaults to workspace title)
    pub title: Option<String>,
    /// Include audience filtering
    pub audience: Option<String>,
    /// Overwrite existing destination
    pub force: bool,
    /// Copy referenced attachment files to the output directory
    pub copy_attachments: bool,
}

impl Default for PublishOptions {
    fn default() -> Self {
        Self {
            single_file: false,
            title: None,
            audience: None,
            force: false,
            copy_attachments: true,
        }
    }
}

/// A navigation link
#[derive(Debug, Clone, Serialize)]
pub struct NavLink {
    /// Link href (relative path or anchor)
    pub href: String,
    /// Display title
    pub title: String,
}

/// A processed file ready for publishing
#[derive(Debug, Clone, Serialize)]
pub struct PublishedPage {
    /// Original source path
    pub source_path: PathBuf,
    /// Destination filename (e.g., "index.html" or "my-entry.html")
    pub dest_filename: String,
    /// Page title
    pub title: String,
    /// HTML content (body only, no wrapper)
    pub html_body: String,
    /// Original markdown body
    pub markdown_body: String,
    /// Navigation links to children (from contents property)
    pub contents_links: Vec<NavLink>,
    /// Navigation link to parent (from part_of property)
    pub parent_link: Option<NavLink>,
    /// Whether this is the root index
    pub is_root: bool,
    /// Raw frontmatter key-value pairs for metadata pill display
    pub frontmatter: indexmap::IndexMap<String, serde_yaml::Value>,
}

/// Result of publishing operation
#[derive(Debug, Serialize)]
pub struct PublishResult {
    /// Pages that were published
    pub pages: Vec<PublishedPage>,
    /// Total files processed
    pub files_processed: usize,
    /// Number of attachment files copied to the output directory
    pub attachments_copied: usize,
}
