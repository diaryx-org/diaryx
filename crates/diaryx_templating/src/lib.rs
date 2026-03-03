//! Shared templating domain logic for Diaryx plugins.
//!
//! This crate is host-agnostic (no `diaryx_core` dependency) and provides two
//! templating systems:
//!
//! - **Creation-time** ([`creation`]): Simple `{{variable}}` substitution that runs
//!   once when an entry is created. Includes `Template`, `TemplateContext`, built-in
//!   templates, and `TemplateManager` for CRUD via a pluggable filesystem trait.
//!
//! - **Render-time** ([`render`]): Full Handlebars engine for `{{#if}}`, `{{#each}}`,
//!   `{{#for-audience}}`, etc. Runs on every view/publish. Custom helpers for
//!   audience-aware conditional rendering.

/// Creation-time template engine for creating entries with pre-defined structures.
pub mod creation;

/// Render-time body templating using Handlebars.
pub mod render;
