#![doc = include_str!(concat!(env!("OUT_DIR"), "/README.md"))]
#![warn(missing_docs)]

/// Workspace appearance: theme colors, typography, and favicon resolution
pub mod appearance;

/// Authentication module for sync server
pub mod auth;

/// Billing tier model and feature gates
pub mod billing;

/// Command pattern API for unified command execution
pub mod command;
pub use command::{Command, Response};

/// Unified Diaryx API - the main entry point
pub mod diaryx;

/// Command handler - execute() implementation for Diaryx
mod command_handler;

/// Configuration options
pub mod config;

/// Entry docs
pub mod entry;

/// Error (common error types)
pub mod error;

/// Export (for backup or filtering by audience property)
pub mod export;

/// Filesystem abstraction
pub mod fs;

/// Search (query frontmatter or search content)
pub mod search;

/// Frontmatter parsing and manipulation utilities
pub mod frontmatter;

/// Audience visibility directive filtering for markdown bodies
pub mod visibility;

/// Metadata-to-frontmatter conversion and file writing utilities
pub mod metadata_writer;

/// Validate (check workspace link integrity)
pub mod validate;

/// Portable path link parsing and formatting for frontmatter link properties
/// (e.g., part_of/contents/attachments)
pub mod link_parser;

/// Utility functions (date parsing, path calculations)
pub mod utils;

/// Workspace (specify a directory to work in)
pub mod workspace;

/// Multi-workspace registry types shared across frontends
pub mod workspace_registry;

/// Core data types (FileMetadata, BinaryRef, CrdtStorage trait, history types)
pub mod types;

/// Plugin architecture for modular feature composition
pub mod plugin;

/// Minimal YAML value type for dynamic frontmatter manipulation
pub mod yaml_value;
pub use yaml_value::YamlValue;

// Re-exports for backwards compatibility
pub use utils::date;
pub use utils::path as path_utils;

/// Re-export uuid so downstream crates don't need a separate dependency.
#[cfg(feature = "uuid")]
pub use uuid;

#[cfg(test)]
pub mod test_utils;
