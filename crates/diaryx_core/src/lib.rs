#![doc = include_str!(concat!(env!("OUT_DIR"), "/README.md"))]
#![warn(missing_docs)]

/// Re-export of the `fig` crate so downstream crates (plugins, the extism host)
/// can name `fig::ToValue`/`fig::FromValue`/`fig::Value` to convert the core
/// types — which derive fig's traits instead of serde — without taking their
/// own `fig` dependency.
pub use fig;

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

/// Frontmatter parsing and manipulation utilities (YAML between `---` fences).
///
/// Formerly the `bookmatter` crate, consolidated into diaryx_core.
pub mod frontmatter;

/// Audience visibility directive filtering for markdown bodies
pub mod visibility;

/// Metadata-to-frontmatter conversion and file writing utilities
pub mod metadata_writer;

/// Centralized ARK blade minting (uuid-entropy plumbing in one place).
#[cfg(feature = "uuid")]
mod mint;

/// Server-namespace management (metadata lookup, deletion) shared across
/// CLI, Tauri, and Web hosts
pub mod namespace;

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

/// Core data types (FileMetadata, BinaryRef, history types)
pub mod types;

/// Plugin architecture for modular feature composition
pub mod plugin;

/// Publish pipeline: collect markdown sources, diff against server state, upload
/// via a `NamespaceProvider` port, and trigger the server-side render (ARK
/// Layer 3). Feature-gated (`publish`); the app shells provide the HTTP provider.
#[cfg(feature = "publish")]
pub mod publish;

/// YAML format primitives (Value, Mapping, Error, from_str, to_string).
///
/// Formerly the `bookmatter` crate, consolidated into diaryx_core.
pub mod yaml;

// Re-exports for backwards compatibility
pub use utils::date;
pub use utils::path as path_utils;

/// Re-export uuid so downstream crates don't need a separate dependency.
#[cfg(feature = "uuid")]
pub use uuid;

#[cfg(test)]
pub mod test_utils;
