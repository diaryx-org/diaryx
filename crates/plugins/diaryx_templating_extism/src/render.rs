//! Render-time body templating (Handlebars + visibility directives).
//!
//! The implementation now lives in [`diaryx_render::template`] so the same
//! render-time templating can run server-side (ARK Layer 3). This module
//! re-exports it unchanged, keeping the plugin's call sites
//! (`render::render`, `render::render_for_audiences`, `render::has_templates`,
//! …) working exactly as before.

pub use diaryx_render::template::*;
