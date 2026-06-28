//! Extism-free rendering engine for Diaryx.
//!
//! This crate holds the markdown-to-HTML pipeline that was previously embedded
//! in the `diaryx_publish_extism` plugin. It is being extracted so the same
//! rendering can run server-side (ARK Layer 3: render-on-write in the sync
//! server and Cloudflare worker) as well as client-side in the publish plugin.
//!
//! It must remain portable to `wasm32-unknown-unknown` (the Cloudflare worker
//! target): no Extism, no host functions, no filesystem, no entropy/clock.

pub mod appearance;
pub mod html;
mod links;
mod markdown;
pub mod nav;
pub mod page;
#[cfg(feature = "templating")]
pub mod site;
#[cfg(feature = "templating")]
pub mod template;
pub mod types;

pub use appearance::{
    ColorPalette, ContentWidth, FaviconAsset, FontFamily, ThemeAppearance, TypographySettings,
};
pub use html::{HtmlRenderer, SiteStyle};
pub use links::{percent_decode, root_prefix, transform_links};
pub use markdown::{markdown_to_html, preprocess_custom_syntax};
pub use nav::{build_site_nav_tree, nav_for_page};
