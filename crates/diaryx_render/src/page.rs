//! Page-shell helpers: navigation, breadcrumbs, SEO meta, feed/sitemap/robots
//! generation, and small HTML/XML escaping utilities.
//!
//! These are pure functions over the value types in [`crate::types`]. The page
//! *assembly* (full `<html>` document, theme/CSS/favicon) still lives in the
//! publish plugin and will move here in a later slice.

use diaryx_core::entry::slugify;
use diaryx_core::link_parser;

use crate::links::root_prefix;
use crate::types::{NavLink, PublishedPage, SiteNavNode, SiteNavigation};

/// Escape HTML special characters.
pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Convert a title to an anchor ID.
pub fn title_to_anchor(title: &str) -> String {
    slugify(title)
}

/// Render the full site navigation sidebar.
pub fn render_site_nav(nav: &SiteNavigation, root_prefix: &str) -> String {
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
pub fn render_full_breadcrumbs(breadcrumbs: &[NavLink], prefix: &str) -> String {
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
pub fn render_breadcrumb(page: &PublishedPage, single_file: bool) -> String {
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

/// Generate SEO meta tags for a page.
pub fn generate_seo_meta(page: &PublishedPage, site_title: &str, base_url: &str) -> String {
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

/// Generate `<link>` tags for Atom and RSS feeds.
pub fn generate_feed_link_tags(root_prefix: &str) -> String {
    format!(
        r#"<link rel="alternate" type="application/atom+xml" title="Atom Feed" href="{}feed.xml">
    <link rel="alternate" type="application/rss+xml" title="RSS Feed" href="{}rss.xml">"#,
        root_prefix, root_prefix,
    )
}

/// Generate a sitemap.xml from published pages.
pub fn generate_sitemap(pages: &[PublishedPage], base_url: &str) -> String {
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

/// Generate robots.txt content.
pub fn generate_robots_txt(base_url: &str, is_public: bool) -> String {
    if is_public {
        format!(
            "User-agent: *\nAllow: /\nSitemap: {}/sitemap.xml\n",
            base_url.trim_end_matches('/')
        )
    } else {
        "User-agent: *\nDisallow: /\n".to_string()
    }
}

/// Generate an Atom 1.0 feed.
pub fn generate_atom_feed(
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
pub fn generate_rss_feed(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::NavLink;
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
}
