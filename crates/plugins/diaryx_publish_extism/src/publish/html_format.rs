//! HTML output format for the publish pipeline.
//!
//! Implements [`PublishFormat`] using comrak for markdown-to-HTML conversion,
//! with custom syntax preprocessing (highlights, spoilers), link rewriting,
//! metadata pills, and a built-in CSS stylesheet.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use diaryx_render::page::{
    generate_atom_feed, generate_feed_link_tags, generate_robots_txt, generate_rss_feed,
    generate_seo_meta, generate_sitemap, html_escape, render_breadcrumb, render_full_breadcrumbs,
    render_site_nav, title_to_anchor,
};

use super::publish_format::PublishFormat;
use super::types::{FaviconAsset, PublishOptions, PublishTheme, PublishedPage, SiteNavigation};

/// HTML output format backed by comrak.
///
/// Optionally holds a [`PublishTheme`] to override the default CSS color variables.
pub struct HtmlFormat {
    theme: Option<PublishTheme>,
}

impl HtmlFormat {
    /// Create a new HtmlFormat with default styling.
    pub fn new() -> Self {
        Self { theme: None }
    }

    /// Create a new HtmlFormat with a theme that overrides default colors.
    pub fn with_theme(theme: PublishTheme) -> Self {
        Self { theme: Some(theme) }
    }

    /// Get the CSS stylesheet, optionally with theme overrides appended.
    fn css(&self) -> String {
        let base = get_base_css();
        match &self.theme {
            Some(theme) => {
                let overrides = theme.to_css_overrides();
                if overrides.is_empty() {
                    base.to_string()
                } else {
                    format!("{}\n/* ── Theme overrides ── */\n{}", base, overrides)
                }
            }
            None => base.to_string(),
        }
    }

    /// Get the favicon asset — user-provided if available, otherwise auto-generated
    /// from the theme accent color. Returns `None` only when there is no theme at all.
    fn favicon(&self) -> Option<FaviconAsset> {
        self.theme.as_ref().map(|t| t.favicon_or_default())
    }

    /// Generate the `<link rel="icon">` tag for the favicon, if available.
    fn favicon_link_tag(&self, prefix: &str) -> String {
        match self.favicon() {
            Some(fav) => format!(
                r#"<link rel="icon" type="{}" href="{}{}">"#,
                fav.mime_type, prefix, fav.filename
            ),
            None => String::new(),
        }
    }

    fn interactivity_script(&self) -> &'static str {
        r#"function clampIslandHeight(value) {
        if (!Number.isFinite(value) || value <= 0) return null;
        return Math.max(200, Math.min(Math.round(value), 4000));
    }

    function requestIslandMeasurement(frame) {
        if (!frame || !frame.contentWindow) return;
        try {
            frame.contentWindow.postMessage({ type: 'diaryx-html-attachment-measure' }, '*');
        } catch (_error) {}
    }

    function installSpoilers() {
        document.querySelectorAll('.spoiler-mark').forEach(function(el) {
            el.addEventListener('click', function() {
                el.classList.toggle('spoiler-hidden');
                el.classList.toggle('spoiler-revealed');
            });
        });
    }

    function installIslandResizeBridge() {
        document.querySelectorAll('iframe.diaryx-island').forEach(function(frame) {
            frame.addEventListener('load', function() {
                requestIslandMeasurement(frame);
                setTimeout(function() { requestIslandMeasurement(frame); }, 80);
            });
        });

        window.addEventListener('message', function(event) {
            var data = event.data;
            if (!data || data.type !== 'diaryx-html-attachment-size') return;

            var nextHeight = clampIslandHeight(Number(data.height));
            if (nextHeight === null) return;

            var frames = document.querySelectorAll('iframe.diaryx-island');
            for (var i = 0; i < frames.length; i += 1) {
                var frame = frames[i];
                if (frame.contentWindow === event.source) {
                    frame.style.height = String(nextHeight) + 'px';
                    break;
                }
            }
        });
    }

    installSpoilers();
    installIslandResizeBridge();"#
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
        let prefix = root_prefix(&page.dest_filename);
        let css_link = if single_file {
            format!("<style>{}</style>", self.css())
        } else {
            format!(r#"<link rel="stylesheet" href="{}style.css">"#, prefix)
        };
        let favicon_link = self.favicon_link_tag(&prefix);
        let interactivity_script = self.interactivity_script();

        let breadcrumb_html = render_breadcrumb(page, single_file);

        format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{page_title} - {site_title}</title>
    {css_link}
    {favicon_link}
</head>
<body>
    <main>
        <article>
            {breadcrumb}
            <div class="content">
                {content}
            </div>
        </article>
    </main>
    <footer>
        <p>Generated by <a href="https://diaryx.org">Diaryx</a></p>
    </footer>
    <script>{interactivity_script}</script>
</body>
</html>"#,
            page_title = html_escape(&page.title),
            site_title = html_escape(site_title),
            css_link = css_link,
            favicon_link = favicon_link,
            breadcrumb = breadcrumb_html,
            content = page.rendered_body,
            interactivity_script = interactivity_script,
        )
    }

    fn render_single_document(&self, pages: &[PublishedPage], site_title: &str) -> String {
        let mut sections = Vec::new();

        for page in pages {
            let anchor = title_to_anchor(&page.title);
            let breadcrumb = render_breadcrumb(page, true);

            sections.push(format!(
                r#"<section id="{anchor}">
    {breadcrumb}
    <div class="content">
        {content}
    </div>
</section>"#,
                anchor = html_escape(&anchor),
                breadcrumb = breadcrumb,
                content = page.rendered_body,
            ));
        }

        // Build table of contents
        let mut toc = String::from(r#"<nav class="toc"><h2>Table of Contents</h2><ul>"#);
        for page in pages {
            let anchor = title_to_anchor(&page.title);
            toc.push_str(&format!(
                r##"<li><a href="#{}">{}</a></li>"##,
                html_escape(&anchor),
                html_escape(&page.title)
            ));
        }
        toc.push_str("</ul></nav>");

        // For single-file output, inline the favicon as a data URI
        let favicon_link = match self.favicon() {
            Some(fav) => {
                use base64::Engine;
                let b64 = base64::engine::general_purpose::STANDARD.encode(&fav.data);
                format!(
                    r#"<link rel="icon" type="{}" href="data:{};base64,{}">"#,
                    fav.mime_type, fav.mime_type, b64
                )
            }
            None => String::new(),
        };

        let interactivity_script = self.interactivity_script();

        format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{site_title}</title>
    <style>{css}</style>
    {favicon_link}
</head>
<body>
    <main>
        {toc}
        {sections}
    </main>
    <footer>
        <p>Generated by <a href="https://diaryx.org">Diaryx</a></p>
    </footer>
    <script>{interactivity_script}</script>
</body>
</html>"#,
            site_title = html_escape(site_title),
            css = self.css(),
            favicon_link = favicon_link,
            toc = toc,
            sections = sections.join("\n<hr>\n"),
            interactivity_script = interactivity_script,
        )
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
        let prefix = root_prefix(&page.dest_filename);
        let css_link = if single_file {
            format!("<style>{}</style>", self.css())
        } else {
            format!(r#"<link rel="stylesheet" href="{}style.css">"#, prefix)
        };

        let favicon_link = self.favicon_link_tag(&prefix);
        let nav_html = render_site_nav(site_nav, &prefix);
        let breadcrumb_html = render_full_breadcrumbs(&site_nav.breadcrumbs, &prefix);
        let interactivity_script = self.interactivity_script();

        let has_nav = !site_nav.tree.is_empty();
        let body_class = if has_nav {
            r#" class="has-site-nav""#
        } else {
            ""
        };

        format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{page_title} - {site_title}</title>
    {css_link}
    {favicon_link}
    {seo_meta}
    {feed_links}
</head>
<body{body_class}>
    {site_nav}
    <div class="site-content">
    <main>
        <article>
            {breadcrumb}
            <div class="content">
                {content}
            </div>
        </article>
    </main>
    <footer>
        <p>Generated by <a href="https://diaryx.org">Diaryx</a></p>
    </footer>
    </div>
    <script>
    (function() {{
        // Nav hamburger toggle
        var toggle = document.querySelector('.nav-toggle');
        var nav = document.querySelector('.site-nav');
        if (toggle && nav) {{
            toggle.addEventListener('click', function(e) {{
                e.stopPropagation();
                nav.classList.toggle('is-open');
            }});
            document.addEventListener('click', function(e) {{
                if (!nav.contains(e.target)) nav.classList.remove('is-open');
            }});
        }}
        {interactivity_script}
    }})();
    </script>
</body>
</html>"#,
            page_title = html_escape(&page.title),
            site_title = html_escape(site_title),
            css_link = css_link,
            favicon_link = favicon_link,
            seo_meta = seo_meta,
            feed_links = feed_links,
            body_class = body_class,
            site_nav = nav_html,
            breadcrumb = breadcrumb_html,
            content = page.rendered_body,
            interactivity_script = interactivity_script,
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
        let prefix = root_prefix(&page.dest_filename);
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
        let mut assets = vec![("style.css".to_string(), self.css().into_bytes())];
        if let Some(fav) = self.favicon() {
            assets.push((fav.filename, fav.data));
        }
        assets
    }
}

// ============================================================================
// Helper functions
// ============================================================================

/// Compute the relative prefix to get from a page back to the site root.
pub(crate) use diaryx_render::root_prefix;

// ============================================================================
// CSS
// ============================================================================

/// Get the built-in base CSS stylesheet (without theme overrides).
fn get_base_css() -> &'static str {
    include_str!("html_format_css.css")
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
    fn test_render_page_installs_html_attachment_resize_listener() {
        let page = make_page("index.html", "Home", true);
        let format = HtmlFormat::new();
        let rendered = format.render_page(&page, "My Site", false);

        assert!(rendered.contains("diaryx-html-attachment-measure"));
        assert!(rendered.contains("diaryx-html-attachment-size"));
        assert!(rendered.contains("iframe.diaryx-island"));
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
    fn test_html_format_with_theme_includes_overrides() {
        use crate::publish::types::{PublishColorPalette, PublishTheme};

        let theme = PublishTheme {
            id: Some("custom".into()),
            light: PublishColorPalette {
                bg: Some("#ff0000".into()),
                ..Default::default()
            },
            dark: Default::default(),
            ..Default::default()
        };

        let format = HtmlFormat::with_theme(theme);
        let css = format.css();

        // Should contain the base CSS
        assert!(css.contains("body {"));
        // Should contain the theme override
        assert!(css.contains("Theme overrides"));
        assert!(css.contains("--bg: #ff0000"));
    }

    #[test]
    fn test_html_format_default_no_overrides() {
        let format = HtmlFormat::new();
        let css = format.css();

        assert!(css.contains("body {"));
        assert!(!css.contains("Theme overrides"));
    }

    #[test]
    fn test_html_format_with_theme_renders_themed_page() {
        use crate::publish::types::{PublishColorPalette, PublishTheme};

        let theme = PublishTheme {
            id: None,
            light: PublishColorPalette {
                bg: Some("oklch(0.98 0 0)".into()),
                ..Default::default()
            },
            dark: Default::default(),
            ..Default::default()
        };

        let format = HtmlFormat::with_theme(theme);
        let page = make_page("index.html", "Home", true);
        let html = format.render_page(&page, "Test Site", true);

        // Inline CSS should include the theme override
        assert!(html.contains("--bg: oklch(0.98 0 0)"));
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

    #[test]
    fn test_themed_static_assets_include_overrides() {
        use crate::publish::types::{PublishColorPalette, PublishTheme};

        let theme = PublishTheme {
            id: None,
            light: PublishColorPalette {
                accent: Some("hotpink".into()),
                ..Default::default()
            },
            dark: Default::default(),
            ..Default::default()
        };

        let format = HtmlFormat::with_theme(theme);
        let assets = format.static_assets();

        // CSS + auto-generated favicon
        assert_eq!(assets.len(), 2);
        assert_eq!(assets[0].0, "style.css");
        let css = String::from_utf8(assets[0].1.clone()).unwrap();
        assert!(css.contains("--accent: hotpink"));

        // Favicon is auto-generated from accent color
        assert_eq!(assets[1].0, "favicon.svg");
        let svg = String::from_utf8(assets[1].1.clone()).unwrap();
        assert!(svg.contains("hotpink"));
    }
}
