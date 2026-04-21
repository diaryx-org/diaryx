//! Native (non-WASM) adapters for `diaryx_core`.
//!
//! This crate mirrors the role of `diaryx_wasm`: it is the home for
//! platform-specific `FileSystem` implementations and platform conventions
//! that don't belong in the cross-platform core.
//!
//! It provides:
//!
//! - [`RealFileSystem`] — a `FileSystem` implementation backed by `std::fs`.
//! - Native [`Config`](diaryx_core::config::Config) helpers that resolve
//!   the user's config directory via [`dirs`] and implement blocking
//!   `_sync` wrappers via [`futures_lite::future::block_on`].
//! - [`block_on`] — a re-export of `futures_lite::future::block_on` so
//!   native callers don't each import `futures-lite` directly.
//!
//! # Example
//!
//! ```ignore
//! use diaryx_core::fs::SyncToAsyncFs;
//! use diaryx_core::workspace::Workspace;
//! use diaryx_native::{RealFileSystem, NativeConfigExt};
//! use diaryx_core::config::Config;
//!
//! // Real filesystem (std::fs) wrapped for async-first core APIs
//! let fs = SyncToAsyncFs::new(RealFileSystem);
//! let workspace = Workspace::new(fs);
//!
//! // Native config conventions (~/.config/diaryx/config.md, ~/diaryx default)
//! let config = Config::load()?;
//! # Ok::<(), diaryx_core::error::DiaryxError>(())
//! ```

#![warn(missing_docs)]

pub mod config;
pub mod fs;

pub use config::{NativeConfigExt, default_config};
pub use fs::RealFileSystem;

/// Block on a future until it resolves.
///
/// A re-export of [`futures_lite::future::block_on`] for convenience.
pub use futures_lite::future::block_on;
