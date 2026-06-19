//! HTML output format for the publish pipeline.
//!
//! Thin [`PublishFormat`] adapter over [`diaryx_render`]'s portable HTML
//! renderer. All markdown conversion, link rewriting, page-shell assembly, and
//! appearance handling live in `diaryx_render`; this type just wires the
//! plugin's trait to it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use diaryx_render::HtmlRenderer;
use diaryx_render::page::{
    generate_atom_feed, generate_feed_link_tags, generate_robots_txt, generate_rss_feed,
    generate_seo_meta, generate_sitemap,
};

use super::publish_format::PublishFormat;
use super::types::{PublishOptions, PublishTheme, PublishedPage, SiteNavigation};

/// HTML output format backed by [`diaryx_render::HtmlRenderer`].
///
/// Optionally holds a [`PublishTheme`] (via the renderer) to override the
/// default CSS color variables.
pub struct HtmlFormat {
    renderer: HtmlRenderer,
}

impl HtmlFormat {
    /// Create a new HtmlFormat with default styling.
    pub fn new() -> Self {
        Self {
            renderer: HtmlRenderer::new(),
        }
    }

    /// Create a new HtmlFormat with a theme that overrides default colors.
    pub fn with_theme(theme: PublishTheme) -> Self {
        Self {
            renderer: HtmlRenderer::with_theme(theme),
        }
    }
}

impl Default for HtmlFormat {
    fn default() -> Self {
        Self::new()
    }
}

impl PublishFormat for HtmlFormat {
    fn output_extension(&self) -> &str {
        "html"
    }

    fn preprocess_body(&self, markdown: &str) -> String {
        diaryx_render::preprocess_custom_syntax(markdown)
    }

    fn convert_body(&self, preprocessed_markdown: &str) -> String {
        diaryx_render::markdown_to_html(preprocessed_markdown)
    }

    fn transform_links(
        &self,
        html: &str,
        current_path: &Path,
        path_to_filename: &HashMap<PathBuf, String>,
        workspace_dir: &Path,
        dest_filename: &str,
    ) -> String {
        diaryx_render::transform_links(
            html,
            current_path,
            path_to_filename,
            workspace_dir,
            dest_filename,
        )
    }

    fn render_page(&self, page: &PublishedPage, site_title: &str, single_file: bool) -> String {
        self.renderer.render_page(page, site_title, single_file)
    }

    fn render_single_document(&self, pages: &[PublishedPage], site_title: &str) -> String {
        self.renderer.render_single_document(pages, site_title)
    }

    fn render_page_with_context(
        &self,
        page: &PublishedPage,
        site_title: &str,
        single_file: bool,
        site_nav: &SiteNavigation,
        seo_meta: &str,
        feed_links: &str,
    ) -> String {
        self.renderer.render_page_with_context(
            page,
            site_title,
            single_file,
            site_nav,
            seo_meta,
            feed_links,
        )
    }

    fn render_seo_meta(
        &self,
        page: &PublishedPage,
        site_title: &str,
        options: &PublishOptions,
    ) -> String {
        if !options.generate_seo {
            return String::new();
        }
        generate_seo_meta(page, site_title, options.base_url.as_deref().unwrap_or(""))
    }

    fn render_feed_links(&self, page: &PublishedPage) -> String {
        let prefix = diaryx_render::root_prefix(&page.dest_filename);
        generate_feed_link_tags(&prefix)
    }

    fn supplementary_files(
        &self,
        pages: &[PublishedPage],
        options: &PublishOptions,
    ) -> Vec<(String, Vec<u8>)> {
        let base_url = match options.base_url.as_deref() {
            Some(url) if !url.is_empty() => url.trim_end_matches('/'),
            _ => return vec![],
        };

        let mut files = Vec::new();

        if options.generate_seo {
            files.push((
                "sitemap.xml".to_string(),
                generate_sitemap(pages, base_url).into_bytes(),
            ));

            let is_public = true; // Conservative default; audiences handled at serve time
            files.push((
                "robots.txt".to_string(),
                generate_robots_txt(base_url, is_public).into_bytes(),
            ));
        }

        if options.generate_feeds {
            // Extract site metadata from root page
            let root = pages.iter().find(|p| p.is_root);
            let site_title = options
                .title
                .as_deref()
                .or_else(|| root.map(|r| r.title.as_str()))
                .unwrap_or("Site");
            let site_description = root.and_then(|r| r.description.as_deref()).unwrap_or("");
            let site_author = root.and_then(|r| r.author.as_deref()).unwrap_or("");

            files.push((
                "feed.xml".to_string(),
                generate_atom_feed(pages, site_title, base_url, site_description, site_author)
                    .into_bytes(),
            ));
            files.push((
                "rss.xml".to_string(),
                generate_rss_feed(pages, site_title, base_url, site_description, site_author)
                    .into_bytes(),
            ));
        }

        files
    }

    fn static_assets(&self) -> Vec<(String, Vec<u8>)> {
        self.renderer.static_assets()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_page(dest: &str, title: &str, is_root: bool) -> PublishedPage {
        PublishedPage {
            source_path: PathBuf::from(format!("/workspace/{}", dest.replace(".html", ".md"))),
            dest_filename: dest.to_string(),
            title: title.to_string(),
            rendered_body: "<p>Hello world</p>".to_string(),
            markdown_body: "Hello world".to_string(),
            contents_links: vec![],
            parent_link: None,
            is_root,
            description: None,
            author: None,
            created: None,
            updated: None,
            attachments: vec![],
            nav_title: None,
            nav_order: None,
            hide_from_nav: false,
            hide_from_feed: false,
            file_ark: None,
            source_markdown: String::new(),
        }
    }

    #[test]
    fn test_supplementary_files_with_base_url() {
        let format = HtmlFormat::new();
        let root = make_page("index.html", "Home", true);
        let leaf = make_page("post.html", "Post", false);

        let options = PublishOptions {
            base_url: Some("https://example.com".to_string()),
            generate_seo: true,
            generate_feeds: true,
            ..Default::default()
        };

        let files = format.supplementary_files(&[root, leaf], &options);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();

        assert!(names.contains(&"sitemap.xml"));
        assert!(names.contains(&"robots.txt"));
        assert!(names.contains(&"feed.xml"));
        assert!(names.contains(&"rss.xml"));
    }

    #[test]
    fn test_supplementary_files_without_base_url() {
        let format = HtmlFormat::new();
        let options = PublishOptions::default();
        let files = format.supplementary_files(&[], &options);
        assert!(files.is_empty());
    }

    #[test]
    fn test_supplementary_files_seo_only() {
        let format = HtmlFormat::new();
        let root = make_page("index.html", "Home", true);

        let options = PublishOptions {
            base_url: Some("https://example.com".to_string()),
            generate_seo: true,
            generate_feeds: false,
            ..Default::default()
        };

        let files = format.supplementary_files(&[root], &options);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();

        assert!(names.contains(&"sitemap.xml"));
        assert!(names.contains(&"robots.txt"));
        assert!(!names.contains(&"feed.xml"));
        assert!(!names.contains(&"rss.xml"));
    }

    #[test]
    fn test_publish_theme_css_overrides() {
        use crate::publish::types::{PublishColorPalette, PublishTheme};

        let theme = PublishTheme {
            id: Some("test".into()),
            light: PublishColorPalette {
                bg: Some("oklch(1 0 0)".into()),
                text: Some("oklch(0.2 0 0)".into()),
                accent: Some("oklch(0.5 0.2 250)".into()),
                ..Default::default()
            },
            dark: PublishColorPalette {
                bg: Some("oklch(0.1 0 0)".into()),
                text: Some("oklch(0.9 0 0)".into()),
                ..Default::default()
            },
            ..Default::default()
        };

        let css = theme.to_css_overrides();
        assert!(css.contains("--bg: oklch(1 0 0)"));
        assert!(css.contains("--text: oklch(0.2 0 0)"));
        assert!(css.contains("--accent: oklch(0.5 0.2 250)"));
        assert!(css.contains("prefers-color-scheme: dark"));
        assert!(css.contains("--bg: oklch(0.1 0 0)"));
        assert!(css.contains("--text: oklch(0.9 0 0)"));
    }

    #[test]
    fn test_publish_theme_empty_no_overrides() {
        use crate::publish::types::PublishTheme;

        let theme = PublishTheme::default();
        let css = theme.to_css_overrides();
        assert!(css.is_empty());
    }

    #[test]
    fn test_publish_theme_from_app_palette() {
        use crate::publish::types::PublishTheme;

        let mut light = std::collections::HashMap::new();
        light.insert("background".into(), "oklch(1 0 0)".into());
        light.insert("foreground".into(), "oklch(0.1 0 0)".into());
        light.insert("primary".into(), "oklch(0.5 0.2 250)".into());
        light.insert("muted-foreground".into(), "oklch(0.6 0 0)".into());
        light.insert("border".into(), "oklch(0.9 0 0)".into());

        let dark = std::collections::HashMap::new();

        let theme = PublishTheme::from_app_palette(&light, &dark);

        assert_eq!(theme.light.bg.as_deref(), Some("oklch(1 0 0)"));
        assert_eq!(theme.light.text.as_deref(), Some("oklch(0.1 0 0)"));
        assert_eq!(theme.light.accent.as_deref(), Some("oklch(0.5 0.2 250)"));
        assert_eq!(theme.light.text_muted.as_deref(), Some("oklch(0.6 0 0)"));
        assert_eq!(theme.light.border.as_deref(), Some("oklch(0.9 0 0)"));
        assert!(theme.dark.bg.is_none());
    }
}
