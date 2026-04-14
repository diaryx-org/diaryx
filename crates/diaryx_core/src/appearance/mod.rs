//! Workspace appearance: theme colors, typography, and favicon resolution.
//!
//! This module provides the canonical types and async resolution logic for
//! reading a workspace's appearance settings from `.diaryx/` config files.
//! The frontend writes these files; Rust consumers (publish, CLI, etc.) read
//! them through this module.

mod presets;
mod resolve;
mod types;

// ── Path constants ──────────────────────────────────────────────────────────

/// Path to theme settings (selected preset ID).
pub const THEMES_SETTINGS_PATH: &str = ".diaryx/themes/settings.json";

/// Path to theme library (full theme definitions).
pub const THEMES_LIBRARY_PATH: &str = ".diaryx/themes/library.json";

/// Path to typography settings (selected preset ID + overrides).
pub const TYPOGRAPHIES_SETTINGS_PATH: &str = ".diaryx/typographies/settings.json";

/// Path to typography library (full typography definitions).
pub const TYPOGRAPHIES_LIBRARY_PATH: &str = ".diaryx/typographies/library.json";

/// Directory containing theme assets (including favicons).
pub const THEMES_DIR: &str = ".diaryx/themes";

/// Favicon filename candidates in order of preference: `(filename, mime_type)`.
pub const FAVICON_CANDIDATES: &[(&str, &str)] = &[
    ("favicon.svg", "image/svg+xml"),
    ("favicon.png", "image/png"),
    ("favicon.ico", "image/x-icon"),
];

// ── Re-exports ──────────────────────────────────────────────────────────────

pub use presets::builtin_typography_defaults;
pub use resolve::{resolve_appearance, resolve_favicon, resolve_theme_colors, resolve_typography};
pub use types::{
    ColorPalette, ContentWidth, FaviconAsset, FontFamily, ThemeAppearance, TypographySettings,
};
