//! Publishing pipeline — format-agnostic workspace publisher.
//!
//! The generic `Publisher` orchestrates workspace file collection, navigation
//! resolution, body template rendering, and delegates format-specific work
//! (body conversion, link rewriting, page wrapping) to a [`PublishFormat`] impl.
//!
//! Enable the `html-publish` feature for the built-in [`HtmlFormat`] (comrak-backed
//! markdown-to-HTML conversion).

pub mod body_renderer;
pub mod content_provider;
pub mod fs_content_provider;
#[cfg(feature = "html-publish")]
pub mod html_format;
pub mod publish_format;
pub(crate) mod publisher;
pub mod types;

// Re-export content provider types.
pub use body_renderer::{BodyRenderer, NoopBodyRenderer};
pub use content_provider::{ContentProvider, MaterializedFile};
pub use fs_content_provider::FilesystemContentProvider;
#[cfg(feature = "html-publish")]
pub use html_format::HtmlFormat;
pub use publish_format::PublishFormat;
pub use publisher::Publisher;
pub use publisher::{build_site_nav_tree, nav_for_page};
pub use types::{
    NavLink, PublishOptions, PublishResult, PublishedPage, SiteNavNode, SiteNavigation,
};
