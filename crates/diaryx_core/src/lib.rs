//! # `diaryx_core`
//!
//! This is the `diaryx_core` library!
//! It contains shared code for the Diaryx clients.
//!
//! There are three Diaryx clients right now:
//! 1. Command-line (`diaryx`)
//! 2. Web (via `diaryx_wasm`)
//! 3. Tauri (via Tauri backend)
//!
//! Diaryx is an opinionated journaling method that makes careful use of frontmatter
//! so that journal entries are queryable and useable well into the future.


#![warn(missing_docs)]

/// Config docs
pub mod config;

/// Date docs
pub mod date;

/// Entry docs
pub mod entry;

/// Error docs
pub mod error;

/// Export docs
pub mod export;

/// Filesystem docs
pub mod fs;

pub mod publish;
pub mod search;
pub mod template;

/// Workspace docs
pub mod workspace;
