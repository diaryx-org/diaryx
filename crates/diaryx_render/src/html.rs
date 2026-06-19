//! Full HTML document assembly: wraps rendered page bodies in the site shell
//! (head, nav, breadcrumbs, footer, interactivity script) and produces the
//! static CSS/favicon assets.
//!
//! Appearance is a *caller-supplied* input ([`SiteStyle`]) with built-in
//! defaults. A publishing client can pass a color theme, fully-custom CSS, or a
//! custom favicon; when it passes nothing, the server-side default styling
//! (the bundled stylesheet, no favicon) is used. The same renderer runs
//! client-side (publish plugin) and server-side (ARK Layer 3 render-on-write).

use diaryx_core::appearance::{FaviconAsset, ThemeAppearance};

use crate::links::root_prefix;
use crate::page::{
    html_escape, render_breadcrumb, render_full_breadcrumbs, render_site_nav, title_to_anchor,
};
use crate::types::{PublishedPage, SiteNavigation};

/// Caller-supplied appearance for the rendered site.
///
/// All fields are optional; an empty `SiteStyle` yields the built-in default
/// styling. Precedence:
/// - CSS: [`custom_css`](Self::custom_css) replaces the stylesheet entirely;
///   otherwise the bundled base CSS is used, with [`theme`](Self::theme) color
///   overrides appended when present.
/// - Favicon: [`custom_favicon`](Self::custom_favicon) wins; otherwise the
///   theme's favicon (or its accent-derived default) is used; otherwise none.
#[derive(Debug, Clone, Default)]
pub struct SiteStyle {
    /// Color theme (palette + optional favicon). `None` → default palette.
    pub theme: Option<ThemeAppearance>,
    /// Fully custom stylesheet, replacing the built-in CSS entirely.
    pub custom_css: Option<String>,
    /// Custom favicon, overriding the theme/default favicon.
    pub custom_favicon: Option<FaviconAsset>,
}

/// Assembles complete HTML documents from rendered page bodies.
pub struct HtmlRenderer {
    style: SiteStyle,
}

impl HtmlRenderer {
    /// Renderer with built-in default styling (no theme, bundled CSS).
    pub fn new() -> Self {
        Self {
            style: SiteStyle::default(),
        }
    }

    /// Renderer with a color theme overriding the default palette.
    pub fn with_theme(theme: ThemeAppearance) -> Self {
        Self {
            style: SiteStyle {
                theme: Some(theme),
                ..SiteStyle::default()
            },
        }
    }

    /// Renderer with a fully caller-specified [`SiteStyle`].
    pub fn with_style(style: SiteStyle) -> Self {
        Self { style }
    }

    /// Get the CSS stylesheet: custom CSS if provided, otherwise the bundled
    /// base stylesheet with theme color overrides appended.
    fn css(&self) -> String {
        if let Some(custom) = &self.style.custom_css {
            return custom.clone();
        }
        let base = get_base_css();
        match &self.style.theme {
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

    /// Resolve the favicon: custom favicon if provided, else the theme's
    /// favicon (or its accent-derived default). `None` when no styling at all.
    fn favicon(&self) -> Option<FaviconAsset> {
        if let Some(fav) = &self.style.custom_favicon {
            return Some(fav.clone());
        }
        self.style.theme.as_ref().map(|t| t.favicon_or_default())
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

    /// Wrap a rendered page into a complete HTML document.
    pub fn render_page(&self, page: &PublishedPage, site_title: &str, single_file: bool) -> String {
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

    /// Render all pages into a single combined document.
    pub fn render_single_document(&self, pages: &[PublishedPage], site_title: &str) -> String {
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

    /// Render a page with full site context (nav, breadcrumbs, SEO, feeds).
    pub fn render_page_with_context(
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

    /// Static assets to write alongside output files: the stylesheet and, when
    /// available, the favicon. Returns `(filename, content)` pairs.
    pub fn static_assets(&self) -> Vec<(String, Vec<u8>)> {
        let mut assets = vec![("style.css".to_string(), self.css().into_bytes())];
        if let Some(fav) = self.favicon() {
            assets.push((fav.filename, fav.data));
        }
        assets
    }
}

impl Default for HtmlRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Get the built-in base CSS stylesheet (without theme overrides).
fn get_base_css() -> &'static str {
    include_str!("html_format_css.css")
}

#[cfg(test)]
mod tests {
    use super::*;
    use diaryx_core::appearance::{ColorPalette, ThemeAppearance};
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
    fn render_page_installs_html_attachment_resize_listener() {
        let page = make_page("index.html", "Home", true);
        let rendered = HtmlRenderer::new().render_page(&page, "My Site", false);

        assert!(rendered.contains("diaryx-html-attachment-measure"));
        assert!(rendered.contains("diaryx-html-attachment-size"));
        assert!(rendered.contains("iframe.diaryx-island"));
    }

    #[test]
    fn default_css_has_no_overrides() {
        let css = HtmlRenderer::new().css();
        assert!(css.contains("body {"));
        assert!(!css.contains("Theme overrides"));
    }

    #[test]
    fn theme_css_includes_overrides() {
        let theme = ThemeAppearance {
            id: Some("custom".into()),
            light: ColorPalette {
                bg: Some("#ff0000".into()),
                ..Default::default()
            },
            dark: Default::default(),
            ..Default::default()
        };

        let css = HtmlRenderer::with_theme(theme).css();
        assert!(css.contains("body {"));
        assert!(css.contains("Theme overrides"));
        assert!(css.contains("--bg: #ff0000"));
    }

    #[test]
    fn custom_css_replaces_base() {
        let style = SiteStyle {
            custom_css: Some("/* mine */ body { color: red }".to_string()),
            ..SiteStyle::default()
        };
        let css = HtmlRenderer::with_style(style).css();
        assert_eq!(css, "/* mine */ body { color: red }");
        assert!(!css.contains("Theme overrides"));
    }

    #[test]
    fn custom_favicon_overrides_theme() {
        let style = SiteStyle {
            custom_favicon: Some(FaviconAsset {
                filename: "fav.png".into(),
                mime_type: "image/png".into(),
                data: vec![1, 2, 3],
            }),
            ..SiteStyle::default()
        };
        let assets = HtmlRenderer::with_style(style).static_assets();
        assert!(assets.iter().any(|(n, _)| n == "fav.png"));
    }

    #[test]
    fn themed_page_inlines_overrides_in_single_file() {
        let theme = ThemeAppearance {
            id: None,
            light: ColorPalette {
                bg: Some("oklch(0.98 0 0)".into()),
                ..Default::default()
            },
            dark: Default::default(),
            ..Default::default()
        };

        let page = make_page("index.html", "Home", true);
        let html = HtmlRenderer::with_theme(theme).render_page(&page, "Test Site", true);
        assert!(html.contains("--bg: oklch(0.98 0 0)"));
    }

    #[test]
    fn themed_static_assets_include_overrides() {
        let theme = ThemeAppearance {
            id: None,
            light: ColorPalette {
                accent: Some("hotpink".into()),
                ..Default::default()
            },
            dark: Default::default(),
            ..Default::default()
        };

        let assets = HtmlRenderer::with_theme(theme).static_assets();
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
