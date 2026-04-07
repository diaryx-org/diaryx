//! Publishing data types.
//!
//! This module contains the core data types for publishing operations.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Color palette for a single mode (light or dark).
///
/// Maps the app's 26-color OKLch theme palette to the 11 CSS variables used
/// by the publish stylesheet. Values are CSS color strings (OKLch, hex, etc.).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PublishColorPalette {
    /// Page background (`--bg`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg: Option<String>,
    /// Primary text color (`--text`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Secondary/muted text (`--text-muted`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_muted: Option<String>,
    /// Accent/link color (`--accent`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
    /// Accent hover state (`--accent-hover`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent_hover: Option<String>,
    /// Border color (`--border`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border: Option<String>,
    /// Code/pre background (`--code-bg`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_bg: Option<String>,
    /// Surface background for floating elements (`--surface-bg`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_bg: Option<String>,
    /// Surface border (`--surface-border`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_border: Option<String>,
    /// Surface shadow (`--surface-shadow`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_shadow: Option<String>,
    /// Divider color (`--divider-color`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub divider_color: Option<String>,
}

impl PublishColorPalette {
    /// Generate CSS variable declarations for all set colors.
    pub fn to_css_vars(&self) -> String {
        let mut vars = String::new();
        let mappings: &[(&Option<String>, &str)] = &[
            (&self.bg, "--bg"),
            (&self.text, "--text"),
            (&self.text_muted, "--text-muted"),
            (&self.accent, "--accent"),
            (&self.accent_hover, "--accent-hover"),
            (&self.border, "--border"),
            (&self.code_bg, "--code-bg"),
            (&self.surface_bg, "--surface-bg"),
            (&self.surface_border, "--surface-border"),
            (&self.surface_shadow, "--surface-shadow"),
            (&self.divider_color, "--divider-color"),
        ];
        for (value, name) in mappings {
            if let Some(v) = value {
                vars.push_str(&format!("    {}: {};\n", name, v));
            }
        }
        vars
    }
}

/// A favicon asset ready for publishing.
#[derive(Debug, Clone)]
pub struct FaviconAsset {
    /// Filename (e.g. "favicon.svg", "favicon.png", "favicon.ico")
    pub filename: String,
    /// MIME type (e.g. "image/svg+xml", "image/png", "image/x-icon")
    pub mime_type: String,
    /// Raw file bytes
    pub data: Vec<u8>,
}

/// Theme configuration for published output.
///
/// Provides color overrides for the publish stylesheet. If not set, the
/// default hardcoded colors from `html_format_css.css` are used.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PublishTheme {
    /// Theme identifier (e.g. "default", "sepia", "nord").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Light mode color overrides.
    #[serde(default)]
    pub light: PublishColorPalette,
    /// Dark mode color overrides.
    #[serde(default)]
    pub dark: PublishColorPalette,
    /// Optional favicon for published sites. Not serialized (binary data).
    #[serde(skip)]
    pub favicon: Option<FaviconAsset>,
}

impl PublishTheme {
    /// Generate a CSS block that overrides the default `:root` and dark-mode
    /// variables with theme colors. Returns empty string if no colors are set.
    pub fn to_css_overrides(&self) -> String {
        let light_vars = self.light.to_css_vars();
        let dark_vars = self.dark.to_css_vars();

        if light_vars.is_empty() && dark_vars.is_empty() {
            return String::new();
        }

        let mut css = String::new();
        if !light_vars.is_empty() {
            css.push_str(&format!(":root {{\n{}}}\n", light_vars));
        }
        if !dark_vars.is_empty() {
            css.push_str(&format!(
                "@media (prefers-color-scheme: dark) {{\n  :root {{\n{}\n  }}\n}}\n",
                dark_vars
            ));
        }
        css
    }

    /// Create a theme from an app ThemeDefinition's color palettes.
    ///
    /// Maps the app's semantic color keys to publish CSS variables:
    /// - background → bg
    /// - foreground → text
    /// - muted-foreground → text-muted
    /// - primary → accent
    /// - primary (lightened) → accent-hover
    /// - border → border
    /// - secondary → code-bg
    /// - card → surface-bg
    pub fn from_app_palette(
        light: &std::collections::HashMap<String, String>,
        dark: &std::collections::HashMap<String, String>,
    ) -> Self {
        Self {
            id: None,
            light: Self::map_palette(light),
            dark: Self::map_palette(dark),
            favicon: None,
        }
    }

    /// Generate a fallback SVG favicon from the theme's accent color.
    ///
    /// Produces a simple circle using the light palette's accent color,
    /// falling back to a default blue if no accent is set.
    pub fn generate_favicon_svg(&self) -> FaviconAsset {
        let accent = self.light.accent.as_deref().unwrap_or("#6366f1");
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><circle cx="16" cy="16" r="14" fill="{}"/></svg>"#,
            accent
        );
        FaviconAsset {
            filename: "favicon.svg".to_string(),
            mime_type: "image/svg+xml".to_string(),
            data: svg.into_bytes(),
        }
    }

    /// Return the favicon if set, otherwise generate one from the accent color.
    pub fn favicon_or_default(&self) -> FaviconAsset {
        match &self.favicon {
            Some(f) => f.clone(),
            None => self.generate_favicon_svg(),
        }
    }

    fn map_palette(colors: &std::collections::HashMap<String, String>) -> PublishColorPalette {
        PublishColorPalette {
            bg: colors.get("background").cloned(),
            text: colors.get("foreground").cloned(),
            text_muted: colors.get("muted-foreground").cloned(),
            accent: colors.get("primary").cloned(),
            accent_hover: colors.get("ring").cloned(),
            border: colors.get("border").cloned(),
            code_bg: colors.get("secondary").cloned(),
            surface_bg: colors.get("card").cloned(),
            surface_border: colors.get("sidebar-border").cloned(),
            surface_shadow: None, // No direct mapping; keep default
            divider_color: None,  // No direct mapping; keep default
        }
    }
}

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
    /// Page description (from frontmatter `description`)
    pub description: Option<String>,
    /// Page author (from frontmatter `author`)
    pub author: Option<String>,
    /// Creation date (from frontmatter `created`)
    pub created: Option<String>,
    /// Last update date (from frontmatter `updated`)
    pub updated: Option<String>,
    /// Attachment paths (from frontmatter `attachments`)
    pub attachments: Vec<String>,
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
