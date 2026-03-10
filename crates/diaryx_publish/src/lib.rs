#![doc = "Publishing pipeline for Diaryx workspaces."]
#![doc = ""]
#![doc = "Converts workspace markdown files to HTML for sharing."]
#![doc = ""]
#![doc = "# Key Types"]
#![doc = ""]
#![doc = "- [`Publisher`] — main entry point for publishing"]
#![doc = "- [`FilesystemContentProvider`] — reads content from the local filesystem"]
#![doc = "- [`ContentProvider`] (re-exported from `diaryx_core`) — trait for content sources"]

mod fs_content_provider;
pub mod plugin;
mod publisher;
#[cfg(feature = "templating")]
mod template_render;
mod types;

pub use fs_content_provider::FilesystemContentProvider;
pub use plugin::{AudienceAccessState, AudiencePublishConfig, PublishPlugin, PublishPluginConfig};
pub use publisher::Publisher;
pub use types::{NavLink, PublishOptions, PublishResult, PublishedPage};

// Re-export content provider types from diaryx_core for convenience.
pub use diaryx_core::publish::{ContentProvider, MaterializedFile};
