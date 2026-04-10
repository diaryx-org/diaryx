//! Workspace link validation and fixing.
//!
//! This module provides functionality to validate `part_of` and `contents`
//! references within a workspace, detecting broken links and other structural
//! issues, and optionally fixing them.
//!
//! # Async-first Design
//!
//! This module uses `AsyncFileSystem` for all filesystem operations.
//! For synchronous contexts (CLI, tests), wrap a sync filesystem with
//! `SyncToAsyncFs` and use `futures_lite::future::block_on()`.
//!
//! # Module layout
//!
//! The module is split by concern so `validate/mod.rs` stays thin:
//!
//! - [`types`] — the `ValidationError`, `ValidationWarning`, and
//!   `ValidationResult` enums plus their `description` / `can_auto_fix` /
//!   `file_path` / `is_viewable` / `supports_parent_picker` metadata impls.
//! - [`meta`] — `*WithMeta` wrappers that attach precomputed metadata (short
//!   description, one-line detail, primary path, UI-facing booleans) so
//!   consumers can render results without switching on variants.
//! - [`detail`] — per-variant one-line contextual summaries used by
//!   `*WithMeta::detail` and by CLI/TUI output.
//! - [`check`] — pure helpers for portability, canonical-link equivalence,
//!   and duplicate detection, plus the shared `find_index_in_directory`
//!   routine.
//! - [`validator`] — the async `Validator` that walks a workspace tree and
//!   emits warnings/errors.
//! - [`fixer`] — the async `ValidationFixer` that applies auto-fixes,
//!   dispatched from [`fixer::ValidationFixer::fix_warning`] so callers
//!   don't need variant-specific fix calls.

mod check;
mod detail;
pub mod fixer;
pub mod meta;
pub mod types;
mod validator;

pub use fixer::{FixResult, ValidationFixer};
pub use meta::{ValidationErrorWithMeta, ValidationResultWithMeta, ValidationWarningWithMeta};
pub use types::{InvalidAttachmentRefKind, ValidationError, ValidationResult, ValidationWarning};
pub use validator::Validator;

#[cfg(test)]
mod tests;
