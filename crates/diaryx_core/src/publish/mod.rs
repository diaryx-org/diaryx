//! Publishing content provider abstraction.
//!
//! The publishing pipeline has moved to the `diaryx_publish` crate.
//! This module retains the [`ContentProvider`] trait and [`MaterializedFile`]
//! type, which are part of the core shared kernel.

pub mod content_provider;

// Re-export content provider types.
pub use content_provider::{ContentProvider, MaterializedFile};
