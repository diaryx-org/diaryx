//! HTML output format for the publish pipeline.
//!
//! Implements [`PublishFormat`] using comrak for markdown-to-HTML conversion,
//! with custom syntax preprocessing (highlights, spoilers), link rewriting,
//! metadata pills, and a built-in CSS stylesheet.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use diaryx_core::entry::slugify;
use diaryx_core::link_parser;

use super::publish_format::PublishFormat;
use super::types::{
    FaviconAsset, NavLink, PublishOptions, PublishTheme, PublishedPage, SiteNavNode, SiteNavigation,
};

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
        preprocess_custom_syntax(markdown)
    }

    fn convert_body(&self, preprocessed_markdown: &str) -> String {
        use comrak::{Options, markdown_to_html};

        let mut options = Options::default();
        options.extension.strikethrough = true;
        options.extension.table = true;
        options.extension.autolink = true;
        options.extension.tasklist = true;
        options.extension.footnotes = true;
        options.render.r#unsafe = true; // Allow raw HTML

        markdown_to_html(preprocessed_markdown, &options)
    }

    fn transform_links(
        &self,
        html: &str,
        current_path: &Path,
        path_to_filename: &HashMap<PathBuf, String>,
        workspace_dir: &Path,
        dest_filename: &str,
    ) -> String {
        let prefix = root_prefix(dest_filename);
        // to_canonical expects workspace-relative paths
        let current_relative = current_path
            .strip_prefix(workspace_dir)
            .unwrap_or(current_path);

        let mut result = String::with_capacity(html.len());
        let mut remaining = html;

        while let Some(href_start) = remaining.find("href=\"") {
            result.push_str(&remaining[..href_start + 6]);
            remaining = &remaining[href_start + 6..];

            if let Some(href_end) = remaining.find('"') {
                let rest = &remaining[href_end..];
                let raw_href = &remaining[..href_end];

                if raw_href.ends_with(".md")
                    && !raw_href.starts_with("http://")
                    && !raw_href.starts_with("https://")
                    && !raw_href.starts_with('#')
                {
                    let decoded_href = super::publisher::percent_decode(raw_href);
                    let parsed = link_parser::parse_link(&decoded_href);
                    let canonical = link_parser::to_canonical(&parsed, current_relative);
                    let target_path = workspace_dir.join(&canonical);

                    let html_path =
                        path_to_filename
                            .get(&target_path)
                            .cloned()
                            .unwrap_or_else(|| {
                                Path::new(&canonical)
                                    .with_extension("html")
                                    .to_string_lossy()
                                    .into_owned()
                            });

                    result.push_str(&format!("{}{}", prefix, html_path));
                } else {
                    result.push_str(raw_href);
                }

                remaining = rest;
            }
        }
        result.push_str(remaining);

        result
    }

    fn render_page(&self, page: &PublishedPage, site_title: &str, single_file: bool) -> String {
        let prefix = root_prefix(&page.dest_filename);
        let css_link = if single_file {
            format!("<style>{}</style>", self.css())
        } else {
            format!(r#"<link rel="stylesheet" href="{}style.css">"#, prefix)
        };
        let favicon_link = self.favicon_link_tag(&prefix);

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
        <p>Generated by <a href="https://github.com/diaryx-org/diaryx-core">diaryx</a></p>
    </footer>
    <script>
    (function() {{
        document.querySelectorAll('.spoiler-mark').forEach(function(el) {{
            el.addEventListener('click', function() {{
                el.classList.toggle('spoiler-hidden');
                el.classList.toggle('spoiler-revealed');
            }});
        }});
    }})();
    </script>
</body>
</html>"#,
            page_title = html_escape(&page.title),
            site_title = html_escape(site_title),
            css_link = css_link,
            favicon_link = favicon_link,
            breadcrumb = breadcrumb_html,
            content = page.rendered_body,
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
        <p>Generated by <a href="https://github.com/diaryx-org/diaryx-core">diaryx</a></p>
    </footer>
    <script>
    (function() {{
        document.querySelectorAll('.spoiler-mark').forEach(function(el) {{
            el.addEventListener('click', function() {{
                el.classList.toggle('spoiler-hidden');
                el.classList.toggle('spoiler-revealed');
            }});
        }});
    }})();
    </script>
</body>
</html>"#,
            site_title = html_escape(site_title),
            css = self.css(),
            favicon_link = favicon_link,
            toc = toc,
            sections = sections.join("\n<hr>\n"),
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
        <p>Generated by <a href="https://github.com/diaryx-org/diaryx-core">diaryx</a></p>
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
        document.querySelectorAll('.spoiler-mark').forEach(function(el) {{
            el.addEventListener('click', function() {{
                el.classList.toggle('spoiler-hidden');
                el.classList.toggle('spoiler-revealed');
            }});
        }});
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
pub(crate) fn root_prefix(dest_filename: &str) -> String {
    let depth = dest_filename.matches('/').count();
    if depth == 0 {
        String::new()
    } else {
        "../".repeat(depth)
    }
}

/// Convert a title to an anchor ID.
fn title_to_anchor(title: &str) -> String {
    slugify(title)
}

/// Escape HTML special characters.
pub(crate) fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Render the full site navigation sidebar.
fn render_site_nav(nav: &SiteNavigation, root_prefix: &str) -> String {
    if nav.tree.is_empty() {
        return String::new();
    }

    fn render_nodes(nodes: &[SiteNavNode], prefix: &str) -> String {
        let mut html = String::from("<ul class=\"nav-list\">");
        for node in nodes {
            let mut classes = Vec::new();
            if node.is_current {
                classes.push("nav-current");
            }
            if node.is_ancestor_of_current {
                classes.push("nav-ancestor");
            }

            let class_attr = if classes.is_empty() {
                String::new()
            } else {
                format!(r#" class="{}""#, classes.join(" "))
            };

            let aria = if node.is_current {
                r#" aria-current="page""#
            } else {
                ""
            };

            html.push_str(&format!(
                r#"<li{class}><a href="{prefix}{href}"{aria}>{title}</a>"#,
                class = class_attr,
                prefix = prefix,
                href = html_escape(&node.href),
                aria = aria,
                title = html_escape(&node.title),
            ));

            if !node.children.is_empty() {
                html.push_str(&render_nodes(&node.children, prefix));
            }

            html.push_str("</li>");
        }
        html.push_str("</ul>");
        html
    }

    let nav_list = render_nodes(&nav.tree, root_prefix);

    format!(
        r#"<button class="nav-toggle" aria-label="Toggle navigation" aria-expanded="false">&#9776;</button>
<nav class="site-nav" aria-label="Site navigation">
{nav_list}
</nav>"#,
        nav_list = nav_list,
    )
}

/// Render full breadcrumb trail from root to current page.
fn render_full_breadcrumbs(breadcrumbs: &[NavLink], prefix: &str) -> String {
    if breadcrumbs.len() <= 1 {
        return String::new();
    }

    let items: Vec<String> = breadcrumbs
        .iter()
        .enumerate()
        .map(|(i, crumb)| {
            if i == breadcrumbs.len() - 1 {
                // Current page — no link
                format!(
                    r#"<span aria-current="page">{}</span>"#,
                    html_escape(&crumb.title)
                )
            } else {
                format!(
                    r#"<a href="{}{}">{}</a>"#,
                    prefix,
                    html_escape(&crumb.href),
                    html_escape(&crumb.title)
                )
            }
        })
        .collect();

    format!(
        r#"<nav class="breadcrumbs" aria-label="Breadcrumb">{}</nav>"#,
        items.join(r#" <span class="breadcrumb-sep">/</span> "#)
    )
}

/// Render breadcrumb navigation (parent link above the title).
fn render_breadcrumb(page: &PublishedPage, single_file: bool) -> String {
    let prefix = root_prefix(&page.dest_filename);
    if let Some(ref parent) = page.parent_link {
        let href = if single_file {
            format!("#{}", title_to_anchor(&parent.title))
        } else {
            format!("{}{}", prefix, parent.href)
        };
        format!(
            r#"<nav class="breadcrumb" aria-label="Breadcrumb"><a href="{}">{}</a></nav>"#,
            html_escape(&href),
            html_escape(&parent.title),
        )
    } else {
        String::new()
    }
}

// ============================================================================
// Custom syntax preprocessing
// ============================================================================

/// Pre-process custom markdown syntax (highlights, spoilers) into raw HTML
/// before passing to comrak. Skips fenced code blocks and inline code.
fn preprocess_custom_syntax(markdown: &str) -> String {
    let bytes = markdown.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len);
    let mut i = 0;

    while i < len {
        // Skip fenced code blocks (``` ... ```)
        if i + 2 < len && bytes[i] == b'`' && bytes[i + 1] == b'`' && bytes[i + 2] == b'`' {
            let fence_start = i;
            i += 3;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            loop {
                if i >= len {
                    out.push_str(&markdown[fence_start..]);
                    return out;
                }
                if bytes[i] == b'\n'
                    && i + 3 < len
                    && bytes[i + 1] == b'`'
                    && bytes[i + 2] == b'`'
                    && bytes[i + 3] == b'`'
                {
                    i += 4;
                    while i < len && bytes[i] != b'\n' {
                        i += 1;
                    }
                    break;
                }
                i += 1;
            }
            out.push_str(&markdown[fence_start..i]);
            continue;
        }

        // Skip inline code (` ... `)
        if bytes[i] == b'`' {
            let start = i;
            i += 1;
            while i < len && bytes[i] != b'`' {
                i += 1;
            }
            if i < len {
                i += 1;
            }
            out.push_str(&markdown[start..i]);
            continue;
        }

        // Try highlight: ==text== or =={color}text==
        if i + 1 < len && bytes[i] == b'=' && bytes[i + 1] == b'=' {
            if let Some((html, consumed)) = try_parse_highlight(&markdown[i..]) {
                out.push_str(&html);
                i += consumed;
                continue;
            }
        }

        // Try spoiler: ||text||
        if i + 1 < len && bytes[i] == b'|' && bytes[i + 1] == b'|' {
            if let Some((html, consumed)) = try_parse_spoiler(&markdown[i..]) {
                out.push_str(&html);
                i += consumed;
                continue;
            }
        }

        out.push(markdown[i..].chars().next().unwrap());
        i += markdown[i..].chars().next().unwrap().len_utf8();
    }

    out
}

/// Try to parse a highlight starting at `==`. Returns `(html, bytes_consumed)`.
fn try_parse_highlight(s: &str) -> Option<(String, usize)> {
    const VALID_COLORS: &[&str] = &[
        "red", "orange", "yellow", "green", "cyan", "blue", "violet", "pink", "brown", "grey",
    ];

    if !s.starts_with("==") {
        return None;
    }

    let after_open = &s[2..];
    if after_open.is_empty() || after_open.starts_with("==") {
        return None;
    }

    let (color, content_start) = if after_open.starts_with('{') {
        let close_brace = after_open.find('}')?;
        let color_name = &after_open[1..close_brace];
        if !VALID_COLORS.contains(&color_name) {
            return None;
        }
        (color_name, close_brace + 1)
    } else {
        ("yellow", 0)
    };

    let content_region = &after_open[content_start..];
    let close_pos = content_region.find("==")?;
    if close_pos == 0 {
        return None;
    }

    let content = &content_region[..close_pos];
    if content.contains('\n') {
        return None;
    }

    let total_consumed = 2 + content_start + close_pos + 2;
    let html = format!(
        r#"<mark data-highlight-color="{color}" class="highlight-mark highlight-{color}">{content}</mark>"#,
        color = color,
        content = html_escape(content),
    );

    Some((html, total_consumed))
}

/// Try to parse a spoiler starting at `||`. Returns `(html, bytes_consumed)`.
fn try_parse_spoiler(s: &str) -> Option<(String, usize)> {
    if !s.starts_with("||") {
        return None;
    }

    let after_open = &s[2..];
    if after_open.is_empty() || after_open.starts_with("||") {
        return None;
    }

    let close_pos = after_open.find("||")?;
    if close_pos == 0 {
        return None;
    }

    let content = &after_open[..close_pos];
    if content.contains('|') || content.contains('\n') {
        return None;
    }

    let total_consumed = 2 + close_pos + 2;
    let html = format!(
        r#"<span data-spoiler="" class="spoiler-mark spoiler-hidden">{content}</span>"#,
        content = html_escape(content),
    );

    Some((html, total_consumed))
}

// ============================================================================
// SEO meta tags
// ============================================================================

/// Generate SEO meta tags for a page.
fn generate_seo_meta(page: &PublishedPage, site_title: &str, base_url: &str) -> String {
    let mut tags = Vec::new();

    // og:title
    tags.push(format!(
        r#"<meta property="og:title" content="{}">"#,
        html_escape(&page.title)
    ));

    // description + og:description
    if let Some(ref desc) = page.description {
        tags.push(format!(
            r#"<meta name="description" content="{}">"#,
            html_escape(desc)
        ));
        tags.push(format!(
            r#"<meta property="og:description" content="{}">"#,
            html_escape(desc)
        ));
    }

    // author
    if let Some(ref author) = page.author {
        tags.push(format!(
            r#"<meta name="author" content="{}">"#,
            html_escape(author)
        ));
    }

    // article:published_time
    if let Some(ref created) = page.created {
        tags.push(format!(
            r#"<meta property="article:published_time" content="{}">"#,
            html_escape(created)
        ));
    }

    // article:modified_time
    if let Some(ref updated) = page.updated {
        tags.push(format!(
            r#"<meta property="article:modified_time" content="{}">"#,
            html_escape(updated)
        ));
    }

    // og:image — scan attachments for images, then fall back to first <img> in body
    let og_image = find_og_image(page);
    if let Some(img_url) = og_image {
        let full_url = if img_url.starts_with("http://") || img_url.starts_with("https://") {
            img_url
        } else if !base_url.is_empty() {
            format!(
                "{}/{}",
                base_url.trim_end_matches('/'),
                img_url.trim_start_matches('/')
            )
        } else {
            img_url
        };
        tags.push(format!(
            r#"<meta property="og:image" content="{}">"#,
            html_escape(&full_url)
        ));
    }

    // og:type
    let og_type = if page.is_root { "website" } else { "article" };
    tags.push(format!(
        r#"<meta property="og:type" content="{}">"#,
        og_type
    ));

    // og:site_name
    tags.push(format!(
        r#"<meta property="og:site_name" content="{}">"#,
        html_escape(site_title)
    ));

    // og:url + canonical
    if !base_url.is_empty() {
        let url = format!("{}/{}", base_url.trim_end_matches('/'), &page.dest_filename);
        tags.push(format!(
            r#"<meta property="og:url" content="{}">"#,
            html_escape(&url)
        ));
        tags.push(format!(
            r#"<link rel="canonical" href="{}">"#,
            html_escape(&url)
        ));
    }

    tags.join("\n    ")
}

/// Find the best og:image for a page.
fn find_og_image(page: &PublishedPage) -> Option<String> {
    const IMAGE_EXTENSIONS: &[&str] = &[".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg"];

    // Check attachments for images
    for s in &page.attachments {
        let lower = s.to_lowercase();
        if IMAGE_EXTENSIONS.iter().any(|ext| lower.ends_with(ext)) {
            // Extract raw path from link syntax if present
            let parsed = link_parser::parse_link(s);
            return Some(parsed.path);
        }
    }

    // Fall back to first <img src="..."> in rendered body
    if let Some(pos) = page.rendered_body.find("src=\"") {
        let after = &page.rendered_body[pos + 5..];
        if let Some(end) = after.find('"') {
            return Some(after[..end].to_string());
        }
    }

    None
}

// ============================================================================
// Feed link tags
// ============================================================================

/// Generate `<link>` tags for Atom and RSS feeds.
fn generate_feed_link_tags(root_prefix: &str) -> String {
    format!(
        r#"<link rel="alternate" type="application/atom+xml" title="Atom Feed" href="{}feed.xml">
    <link rel="alternate" type="application/rss+xml" title="RSS Feed" href="{}rss.xml">"#,
        root_prefix, root_prefix,
    )
}

// ============================================================================
// Sitemap
// ============================================================================

/// Generate a sitemap.xml from published pages.
fn generate_sitemap(pages: &[PublishedPage], base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
"#,
    );

    for page in pages {
        let loc = format!("{}/{}", base, &page.dest_filename);
        let lastmod = page
            .updated
            .as_deref()
            .or(page.created.as_deref())
            .unwrap_or("");
        let priority = if page.is_root {
            "1.0"
        } else if !page.contents_links.is_empty() {
            "0.8"
        } else {
            "0.6"
        };

        xml.push_str("  <url>\n");
        xml.push_str(&format!("    <loc>{}</loc>\n", xml_escape(&loc)));
        if !lastmod.is_empty() {
            xml.push_str(&format!("    <lastmod>{}</lastmod>\n", xml_escape(lastmod)));
        }
        xml.push_str(&format!("    <priority>{}</priority>\n", priority));
        xml.push_str("  </url>\n");
    }

    xml.push_str("</urlset>\n");
    xml
}

// ============================================================================
// Robots.txt
// ============================================================================

/// Generate robots.txt content.
fn generate_robots_txt(base_url: &str, is_public: bool) -> String {
    if is_public {
        format!(
            "User-agent: *\nAllow: /\nSitemap: {}/sitemap.xml\n",
            base_url.trim_end_matches('/')
        )
    } else {
        "User-agent: *\nDisallow: /\n".to_string()
    }
}

// ============================================================================
// Atom + RSS feeds
// ============================================================================

/// Generate an Atom 1.0 feed.
fn generate_atom_feed(
    pages: &[PublishedPage],
    site_title: &str,
    base_url: &str,
    site_description: &str,
    site_author: &str,
) -> String {
    let base = base_url.trim_end_matches('/');

    // Feed items: non-root leaf pages, not hidden from feed
    let mut items: Vec<&PublishedPage> = pages
        .iter()
        .filter(|p| !p.is_root && p.contents_links.is_empty() && !p.hide_from_feed)
        .collect();

    // Sort by created/updated descending
    items.sort_by(|a, b| {
        let date_a = a.updated.as_deref().or(a.created.as_deref()).unwrap_or("");
        let date_b = b.updated.as_deref().or(b.created.as_deref()).unwrap_or("");
        date_b.cmp(date_a)
    });

    items.truncate(50);

    let feed_updated = items
        .first()
        .and_then(|p| p.updated.as_deref().or(p.created.as_deref()))
        .unwrap_or("1970-01-01T00:00:00Z");

    let mut xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>{title}</title>
  <link href="{base}/" rel="alternate"/>
  <link href="{base}/feed.xml" rel="self"/>
  <id>{base}/</id>
  <updated>{updated}</updated>
"#,
        title = xml_escape(site_title),
        base = xml_escape(base),
        updated = xml_escape(feed_updated),
    );

    if !site_author.is_empty() {
        xml.push_str(&format!(
            "  <author><name>{}</name></author>\n",
            xml_escape(site_author)
        ));
    }
    if !site_description.is_empty() {
        xml.push_str(&format!(
            "  <subtitle>{}</subtitle>\n",
            xml_escape(site_description)
        ));
    }

    for page in &items {
        let link = format!("{}/{}", base, &page.dest_filename);
        let published = page.created.as_deref().unwrap_or("");
        let updated = page
            .updated
            .as_deref()
            .or(page.created.as_deref())
            .unwrap_or("");
        let summary = strip_html_truncate(&page.rendered_body, 280);

        xml.push_str("  <entry>\n");
        xml.push_str(&format!("    <title>{}</title>\n", xml_escape(&page.title)));
        xml.push_str(&format!(
            "    <link href=\"{}\" rel=\"alternate\"/>\n",
            xml_escape(&link)
        ));
        xml.push_str(&format!("    <id>{}</id>\n", xml_escape(&link)));
        if !published.is_empty() {
            xml.push_str(&format!(
                "    <published>{}</published>\n",
                xml_escape(published)
            ));
        }
        if !updated.is_empty() {
            xml.push_str(&format!("    <updated>{}</updated>\n", xml_escape(updated)));
        }
        if !summary.is_empty() {
            xml.push_str(&format!(
                "    <summary>{}</summary>\n",
                xml_escape(&summary)
            ));
        }
        xml.push_str(&format!(
            "    <content type=\"html\"><![CDATA[{}]]></content>\n",
            &page.rendered_body
        ));
        xml.push_str("  </entry>\n");
    }

    xml.push_str("</feed>\n");
    xml
}

/// Generate an RSS 2.0 feed.
fn generate_rss_feed(
    pages: &[PublishedPage],
    site_title: &str,
    base_url: &str,
    site_description: &str,
    _site_author: &str,
) -> String {
    let base = base_url.trim_end_matches('/');

    let mut items: Vec<&PublishedPage> = pages
        .iter()
        .filter(|p| !p.is_root && p.contents_links.is_empty() && !p.hide_from_feed)
        .collect();

    items.sort_by(|a, b| {
        let date_a = a.updated.as_deref().or(a.created.as_deref()).unwrap_or("");
        let date_b = b.updated.as_deref().or(b.created.as_deref()).unwrap_or("");
        date_b.cmp(date_a)
    });

    items.truncate(50);

    let last_build = items
        .first()
        .and_then(|p| p.updated.as_deref().or(p.created.as_deref()))
        .unwrap_or("");

    let desc = if site_description.is_empty() {
        site_title
    } else {
        site_description
    };

    let mut xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom">
<channel>
  <title>{title}</title>
  <link>{base}/</link>
  <description>{description}</description>
  <atom:link href="{base}/rss.xml" rel="self" type="application/rss+xml"/>
"#,
        title = xml_escape(site_title),
        base = xml_escape(base),
        description = xml_escape(desc),
    );

    if !last_build.is_empty() {
        xml.push_str(&format!(
            "  <lastBuildDate>{}</lastBuildDate>\n",
            xml_escape(last_build)
        ));
    }

    for page in &items {
        let link = format!("{}/{}", base, &page.dest_filename);
        let pub_date = page.created.as_deref().unwrap_or("");

        xml.push_str("  <item>\n");
        xml.push_str(&format!("    <title>{}</title>\n", xml_escape(&page.title)));
        xml.push_str(&format!("    <link>{}</link>\n", xml_escape(&link)));
        xml.push_str(&format!(
            "    <guid isPermaLink=\"true\">{}</guid>\n",
            xml_escape(&link)
        ));
        if !pub_date.is_empty() {
            xml.push_str(&format!(
                "    <pubDate>{}</pubDate>\n",
                xml_escape(pub_date)
            ));
        }
        xml.push_str(&format!(
            "    <description><![CDATA[{}]]></description>\n",
            &page.rendered_body
        ));
        xml.push_str("  </item>\n");
    }

    xml.push_str("</channel>\n</rss>\n");
    xml
}

/// Strip HTML tags and truncate to `max_len` characters.
fn strip_html_truncate(html: &str, max_len: usize) -> String {
    let mut text = String::new();
    let mut in_tag = false;

    for ch in html.chars() {
        if ch == '<' {
            in_tag = true;
            continue;
        }
        if ch == '>' {
            in_tag = false;
            continue;
        }
        if !in_tag {
            text.push(ch);
            if text.len() >= max_len {
                break;
            }
        }
    }

    text.trim().to_string()
}

/// Escape characters for XML content.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

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
        }
    }

    #[test]
    fn test_seo_meta_basic() {
        let mut page = make_page("about.html", "About", false);
        page.description = Some("A test page".into());
        page.author = Some("Alice".into());
        let meta = generate_seo_meta(&page, "My Site", "https://example.com");

        assert!(meta.contains(r#"og:title" content="About""#));
        assert!(meta.contains(r#"name="description" content="A test page""#));
        assert!(meta.contains(r#"og:description" content="A test page""#));
        assert!(meta.contains(r#"name="author" content="Alice""#));
        assert!(meta.contains(r#"og:type" content="article""#));
        assert!(meta.contains(r#"og:site_name" content="My Site""#));
        assert!(meta.contains(r#"og:url" content="https://example.com/about.html""#));
        assert!(meta.contains(r#"canonical" href="https://example.com/about.html""#));
    }

    #[test]
    fn test_seo_meta_root_is_website_type() {
        let page = make_page("index.html", "Home", true);
        let meta = generate_seo_meta(&page, "My Site", "https://example.com");
        assert!(meta.contains(r#"og:type" content="website""#));
    }

    #[test]
    fn test_seo_meta_no_base_url() {
        let page = make_page("page.html", "Page", false);
        let meta = generate_seo_meta(&page, "Site", "");
        assert!(!meta.contains("canonical"));
        assert!(!meta.contains("og:url"));
    }

    #[test]
    fn test_sitemap_structure() {
        let root = make_page("index.html", "Home", true);
        let mut child = make_page("child.html", "Child", false);
        child.contents_links = vec![NavLink {
            href: "leaf.html".into(),
            title: "Leaf".into(),
        }];
        let leaf = make_page("leaf.html", "Leaf", false);

        let sitemap = generate_sitemap(&[root, child, leaf], "https://example.com");

        assert!(sitemap.contains("<loc>https://example.com/index.html</loc>"));
        assert!(sitemap.contains("<priority>1.0</priority>")); // root
        assert!(sitemap.contains("<priority>0.8</priority>")); // child with contents
        assert!(sitemap.contains("<priority>0.6</priority>")); // leaf
    }

    #[test]
    fn test_robots_txt_public() {
        let robots = generate_robots_txt("https://example.com", true);
        assert!(robots.contains("Allow: /"));
        assert!(robots.contains("Sitemap: https://example.com/sitemap.xml"));
    }

    #[test]
    fn test_robots_txt_private() {
        let robots = generate_robots_txt("https://example.com", false);
        assert!(robots.contains("Disallow: /"));
        assert!(!robots.contains("Sitemap"));
    }

    #[test]
    fn test_atom_feed_excludes_root_and_index_pages() {
        let root = make_page("index.html", "Home", true);
        let mut index_child = make_page("section.html", "Section", false);
        index_child.contents_links = vec![NavLink {
            href: "leaf.html".into(),
            title: "Leaf".into(),
        }];
        let leaf = make_page("leaf.html", "Leaf", false);

        let atom = generate_atom_feed(
            &[root, index_child, leaf],
            "Site",
            "https://example.com",
            "",
            "",
        );

        // Only the leaf should appear as an entry
        assert_eq!(atom.matches("<entry>").count(), 1);
        assert!(atom.contains("<title>Leaf</title>"));
        assert!(!atom.contains("<title>Home</title>"));
        assert!(!atom.contains("<title>Section</title>"));
    }

    #[test]
    fn test_atom_feed_hide_from_feed() {
        let root = make_page("index.html", "Home", true);
        let mut hidden = make_page("hidden.html", "Hidden", false);
        hidden.hide_from_feed = true;
        let visible = make_page("visible.html", "Visible", false);

        let atom = generate_atom_feed(
            &[root, hidden, visible],
            "Site",
            "https://example.com",
            "",
            "",
        );

        assert_eq!(atom.matches("<entry>").count(), 1);
        assert!(atom.contains("<title>Visible</title>"));
        assert!(!atom.contains("<title>Hidden</title>"));
    }

    #[test]
    fn test_rss_feed_structure() {
        let root = make_page("index.html", "Home", true);
        let mut leaf = make_page("post.html", "Post", false);
        leaf.created = Some("2024-01-15".into());

        let rss = generate_rss_feed(
            &[root, leaf],
            "My Blog",
            "https://example.com",
            "A blog",
            "Author",
        );

        assert!(rss.contains("<title>My Blog</title>"));
        assert!(rss.contains("<description>A blog</description>"));
        assert!(rss.contains("<title>Post</title>"));
        assert!(rss.contains("<guid isPermaLink=\"true\">https://example.com/post.html</guid>"));
        assert!(rss.contains("<pubDate>2024-01-15</pubDate>"));
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
    fn test_feed_links() {
        let links = generate_feed_link_tags("");
        assert!(links.contains("application/atom+xml"));
        assert!(links.contains("feed.xml"));
        assert!(links.contains("application/rss+xml"));
        assert!(links.contains("rss.xml"));
    }

    #[test]
    fn test_strip_html_truncate() {
        let html = "<p>Hello <strong>world</strong>, this is a test.</p>";
        let result = strip_html_truncate(html, 11);
        assert_eq!(result, "Hello world");
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
