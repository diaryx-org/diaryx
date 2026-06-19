//! Publishing data types.
//!
//! The pure value types (`PublishOptions`, `NavLink`, `PublishedPage`,
//! `SiteNavNode`, `SiteNavigation`, `PublishResult`) now live in
//! `diaryx_render::types` so the rendering engine can be shared with the
//! server; they are re-exported here so existing paths keep working.
//!
//! Appearance types (colors, typography, favicon, theme) live in
//! `diaryx_core::appearance` and are re-exported here under their legacy
//! `Publish*` names for backward compatibility within this crate.

// ── Re-exports from core (legacy aliases) ───────────────────────────────────

pub use diaryx_core::appearance::{
    ColorPalette as PublishColorPalette, ContentWidth as PublishContentWidth, FaviconAsset,
    FontFamily as PublishFontFamily, ThemeAppearance as PublishTheme,
    TypographySettings as PublishTypography,
};

// ── Render value types (moved to diaryx_render) ─────────────────────────────

pub use diaryx_render::types::{
    NavLink, PublishOptions, PublishResult, PublishedPage, SiteNavNode, SiteNavigation,
};
