//! Publishing data types.
//!
//! This module contains the core data types for publishing operations.

use std::path::PathBuf;

use serde::Serialize;

/// Options for publishing
#[derive(Debug, Clone, Serialize)]
pub struct PublishOptions {
    /// Output as a single HTML file instead of multiple files
    pub single_file: bool,
    /// Site title (defaults to workspace title)
    pub title: Option<String>,
    /// Include audience filtering
    pub audience: Option<String>,
    /// Overwrite existing destination
    pub force: bool,
    /// Copy referenced attachment files to the output directory
    pub copy_attachments: bool,
    /// Audience tag assigned to entries with no explicit or inherited audience.
    /// When None, such entries are private (excluded from exports).
    pub default_audience: Option<String>,
    /// Base URL for sitemap, canonical URLs, og tags, and feeds.
    pub base_url: Option<String>,
    /// Generate sitemap.xml, robots.txt, and SEO meta tags (default true).
    pub generate_seo: bool,
    /// Generate feed.xml (Atom) and rss.xml (RSS) feeds (default true).
    pub generate_feeds: bool,
}

impl Default for PublishOptions {
    fn default() -> Self {
        Self {
            single_file: false,
            title: None,
            audience: None,
            force: false,
            copy_attachments: true,
            default_audience: None,
            base_url: None,
            generate_seo: true,
            generate_feeds: true,
        }
    }
}

/// A navigation link
#[derive(Debug, Clone, Serialize)]
pub struct NavLink {
    /// Link href (relative path or anchor)
    pub href: String,
    /// Display title
    pub title: String,
}

/// A processed file ready for publishing
#[derive(Debug, Clone, Serialize)]
pub struct PublishedPage {
    /// Original source path
    pub source_path: PathBuf,
    /// Destination filename (e.g., "index.html" or "my-entry.html")
    pub dest_filename: String,
    /// Page title
    pub title: String,
    /// Rendered content in the output format (body only, no wrapper)
    pub rendered_body: String,
    /// Original markdown body
    pub markdown_body: String,
    /// Navigation links to children (from contents property)
    pub contents_links: Vec<NavLink>,
    /// Navigation link to parent (from part_of property)
    pub parent_link: Option<NavLink>,
    /// Whether this is the root index
    pub is_root: bool,
    /// Raw frontmatter key-value pairs for metadata pill display
    pub frontmatter: indexmap::IndexMap<String, serde_yaml::Value>,
    /// Override title shown in navigation (from frontmatter `nav_title`)
    pub nav_title: Option<String>,
    /// Sort order among siblings in navigation (from frontmatter `nav_order`)
    pub nav_order: Option<i32>,
    /// Whether to hide this page from the navigation tree
    pub hide_from_nav: bool,
    /// Whether to hide this page from RSS/Atom feeds
    pub hide_from_feed: bool,
}

/// A node in the full site navigation tree.
#[derive(Debug, Clone, Serialize)]
pub struct SiteNavNode {
    pub title: String,
    pub href: String,
    pub is_current: bool,
    pub is_ancestor_of_current: bool,
    pub children: Vec<SiteNavNode>,
}

/// Full site navigation context for a specific page.
#[derive(Debug, Clone, Serialize)]
pub struct SiteNavigation {
    /// Full nav tree with current-page marking
    pub tree: Vec<SiteNavNode>,
    /// Breadcrumb trail from root to current page
    pub breadcrumbs: Vec<NavLink>,
}

/// Result of publishing operation
#[derive(Debug, Serialize)]
pub struct PublishResult {
    /// Pages that were published
    pub pages: Vec<PublishedPage>,
    /// Total files processed
    pub files_processed: usize,
    /// Number of attachment files copied to the output directory
    pub attachments_copied: usize,
}
