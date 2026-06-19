//! Publish pipeline (ARK Layer 3 client side).
//!
//! Collect a workspace's audience-scoped markdown **sources**, diff them against
//! what the namespace already holds, upload the changed sources + attachments
//! via a [`NamespaceProvider`], then trigger the server-side render. Rendering
//! itself is server-side now, so this path is light: no comrak/handlebars.
//!
//! Feature-gated (`publish`). The app shells provide the [`NamespaceProvider`]
//! implementation (native via reqwest, web via fetch).

pub mod collect;
pub mod plan;
pub mod provider;
pub mod service;
pub mod source;

pub use collect::collect_audience_sources;
pub use provider::{NamespaceProvider, ObjectMeta};
pub use service::{PublishOutcome, PublishService};
pub use source::{Attachment, AudienceInput, SourceFile};
