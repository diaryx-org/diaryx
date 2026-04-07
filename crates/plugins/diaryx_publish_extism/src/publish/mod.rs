//! Publishing pipeline — format-agnostic workspace publisher.
//!
//! Moved from `diaryx_core::publish` to give the publish plugin full ownership
//! of the publish pipeline.

pub mod body_renderer;
pub mod content_provider;
pub mod email_format;
pub mod fs_content_provider;
pub mod html_format;
pub mod publish_format;
pub mod publisher;
pub mod types;

// Re-export content provider types.
pub use body_renderer::{BodyRenderer, NoopBodyRenderer};
pub use content_provider::{ContentProvider, MaterializedFile};
pub use fs_content_provider::FilesystemContentProvider;
pub use html_format::HtmlFormat;
pub use publish_format::PublishFormat;
pub use publisher::Publisher;
pub use publisher::RenderedFile;
pub use publisher::{build_site_nav_tree, nav_for_page};
pub use types::{
    FaviconAsset, NavLink, PublishColorPalette, PublishOptions, PublishResult, PublishTheme,
    PublishedPage, SiteNavNode, SiteNavigation,
};
