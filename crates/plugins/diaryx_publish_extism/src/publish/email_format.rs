//! Email digest renderer.
//!
//! Renders a collection of published pages into a single email-friendly HTML
//! document using MJML (via the `mrml` crate) for responsive layout and
//! automatic CSS inlining.
//!
//! The email structure is:
//! 1. Optional cover file content (personalized intro)
//! 2. Table of contents with anchor links
//! 3. Full rendered entries concatenated with dividers
//! 4. Footer with optional "View on web" link and unsubscribe link

use super::html_format::html_escape;
use super::types::{PublishTheme, PublishedPage};
use diaryx_core::entry::slugify;

/// Options for rendering an email digest.
pub struct EmailDigestOptions<'a> {
    /// Pre-rendered HTML from the cover file (optional per-audience intro).
    pub cover_html: Option<&'a str>,
    /// Site/workspace title.
    pub site_title: &'a str,
    /// Base URL of the published site (None if no site is published).
    pub base_url: Option<&'a str>,
    /// URL template for unsubscribe links. `{token}` is replaced per-recipient.
    pub unsubscribe_url: &'a str,
    /// Theme for color styling.
    pub theme: Option<&'a PublishTheme>,
}

/// Render a digest email from a list of published pages.
///
/// Returns fully inlined HTML ready for email delivery.
pub fn render_email_digest(entries: &[PublishedPage], options: &EmailDigestOptions<'_>) -> String {
    let colors = resolve_colors(options.theme);
    let mjml = build_mjml(entries, options, &colors);

    // Parse and render MJML to HTML
    match mrml::parse(&mjml) {
        Ok(root) => {
            let opts = mrml::prelude::render::RenderOptions::default();
            match root.element.render(&opts) {
                Ok(html) => html,
                Err(e) => {
                    log::warn!("MJML render failed, using fallback: {}", e);
                    build_fallback_html(entries, options, &colors)
                }
            }
        }
        Err(e) => {
            log::warn!("MJML parse failed, using fallback: {}", e);
            build_fallback_html(entries, options, &colors)
        }
    }
}

// ============================================================================
// Color resolution
// ============================================================================

struct EmailColors {
    bg: String,
    text: String,
    text_muted: String,
    accent: String,
    border: String,
    code_bg: String,
}

fn resolve_colors(theme: Option<&PublishTheme>) -> EmailColors {
    // Use light mode colors for email (single mode, no prefers-color-scheme)
    match theme {
        Some(t) => EmailColors {
            bg: t.light.bg.clone().unwrap_or_else(|| "#fafaf9".into()),
            text: t.light.text.clone().unwrap_or_else(|| "#0f172a".into()),
            text_muted: t
                .light
                .text_muted
                .clone()
                .unwrap_or_else(|| "#64748b".into()),
            accent: t.light.accent.clone().unwrap_or_else(|| "#3b82f6".into()),
            border: t.light.border.clone().unwrap_or_else(|| "#e5e7eb".into()),
            code_bg: t.light.code_bg.clone().unwrap_or_else(|| "#f3f4f6".into()),
        },
        None => EmailColors {
            bg: "#fafaf9".into(),
            text: "#0f172a".into(),
            text_muted: "#64748b".into(),
            accent: "#3b82f6".into(),
            border: "#e5e7eb".into(),
            code_bg: "#f3f4f6".into(),
        },
    }
}

// ============================================================================
// MJML construction
// ============================================================================

fn build_mjml(
    entries: &[PublishedPage],
    options: &EmailDigestOptions<'_>,
    colors: &EmailColors,
) -> String {
    let mut body_sections = String::new();

    // Cover section
    if let Some(cover) = options.cover_html {
        body_sections.push_str(&format!(
            r#"<mj-section padding="0 24px">
  <mj-column>
    <mj-text color="{text}" font-size="16px" line-height="1.7" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif">
      {cover}
    </mj-text>
    <mj-divider border-color="{border}" border-width="1px" padding="16px 0" />
  </mj-column>
</mj-section>"#,
            text = html_escape(&colors.text),
            border = html_escape(&colors.border),
            cover = cover,
        ));
    }

    // TOC section (only if more than one entry)
    if entries.len() > 1 {
        let mut toc_items = String::new();
        for entry in entries {
            let anchor = slugify(&entry.title);
            toc_items.push_str(&format!(
                r##"<li style="margin: 4px 0;"><a href="#{anchor}" style="color: {accent}; text-decoration: none;">{title}</a></li>"##,
                anchor = html_escape(&anchor),
                accent = html_escape(&colors.accent),
                title = html_escape(&entry.title),
            ));
        }
        body_sections.push_str(&format!(
            r#"<mj-section padding="0 24px">
  <mj-column>
    <mj-text color="{text_muted}" font-size="14px" font-weight="600" text-transform="uppercase" letter-spacing="0.05em" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif">
      In this issue
    </mj-text>
    <mj-text color="{text}" font-size="15px" line-height="1.6" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif">
      <ul style="margin: 0; padding-left: 20px;">{toc_items}</ul>
    </mj-text>
    <mj-divider border-color="{border}" border-width="1px" padding="16px 0" />
  </mj-column>
</mj-section>"#,
            text_muted = html_escape(&colors.text_muted),
            text = html_escape(&colors.text),
            border = html_escape(&colors.border),
            toc_items = toc_items,
        ));
    }

    // Entry sections
    for entry in entries {
        let anchor = slugify(&entry.title);
        // Transform spoilers for email: keep hidden via CSS text selection
        let body = transform_spoilers_for_email(&entry.rendered_body);

        body_sections.push_str(&format!(
            r#"<mj-section padding="0 24px" css-class="entry-section">
  <mj-column>
    <mj-text mj-class="entry-title" color="{text}" font-size="22px" font-weight="700" line-height="1.3" padding="16px 0 8px" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif">
      <a name="{anchor}" id="{anchor}"></a>{title}
    </mj-text>
    <mj-text color="{text}" font-size="16px" line-height="1.7" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif">
      {body}
    </mj-text>
    <mj-divider border-color="{border}" border-width="1px" padding="24px 0 8px" />
  </mj-column>
</mj-section>"#,
            text = html_escape(&colors.text),
            anchor = html_escape(&anchor),
            title = html_escape(&entry.title),
            body = body,
            border = html_escape(&colors.border),
        ));
    }

    // Footer section
    let mut footer_links = Vec::new();
    if let Some(base_url) = options.base_url {
        footer_links.push(format!(
            r#"<a href="{}" style="color: {accent}; text-decoration: none;">View on web</a>"#,
            html_escape(base_url),
            accent = html_escape(&colors.accent),
        ));
    }
    footer_links.push(format!(
        r#"<a href="{}" style="color: {text_muted}; text-decoration: none;">Unsubscribe</a>"#,
        html_escape(options.unsubscribe_url),
        text_muted = html_escape(&colors.text_muted),
    ));

    body_sections.push_str(&format!(
        r#"<mj-section padding="8px 24px 32px">
  <mj-column>
    <mj-text align="center" color="{text_muted}" font-size="13px" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif">
      {links}
    </mj-text>
  </mj-column>
</mj-section>"#,
        text_muted = html_escape(&colors.text_muted),
        links = footer_links.join(r#" &middot; "#),
    ));

    // Spoiler CSS for email: text is transparent, revealed on selection
    let spoiler_css = format!(
        r#"
    .spoiler-mark {{
      color: transparent;
      background-color: {bg};
      border-radius: 0.2em;
      padding: 0.1em 0.3em;
      border: 1px dashed {border};
    }}
    .spoiler-mark::selection {{
      color: {text};
      background-color: {border};
    }}
    .spoiler-mark::-moz-selection {{
      color: {text};
      background-color: {border};
    }}"#,
        bg = colors.code_bg,
        border = colors.border,
        text = colors.text,
    );

    format!(
        r#"<mjml>
<mj-head>
  <mj-attributes>
    <mj-all font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif" />
    <mj-text padding="0" />
    <mj-section padding="0" />
  </mj-attributes>
  <mj-style>{spoiler_css}</mj-style>
  <mj-style>
    a {{ color: {accent}; text-decoration: none; }}
    a:hover {{ text-decoration: underline; }}
    pre {{ background: {code_bg}; padding: 12px; border-radius: 6px; overflow-x: auto; font-size: 14px; line-height: 1.5; }}
    code {{ background: {code_bg}; padding: 0.15em 0.4em; border-radius: 4px; font-size: 0.9em; font-family: "SF Mono", Consolas, monospace; }}
    pre code {{ background: none; padding: 0; }}
    blockquote {{ border-left: 3px solid {accent}; margin: 1em 0; padding-left: 1em; color: {text_muted}; font-style: italic; }}
    img {{ max-width: 100%; height: auto; border-radius: 6px; }}
    hr {{ border: none; border-top: 1px solid {border}; margin: 2em 0; }}
    .highlight-mark {{ padding: 0.1em 0.2em; border-radius: 0.2em; }}
    .highlight-red {{ background: #fecaca; }}
    .highlight-orange {{ background: #fed7aa; }}
    .highlight-yellow {{ background: #fef08a; }}
    .highlight-green {{ background: #bbf7d0; }}
    .highlight-cyan {{ background: #a5f3fc; }}
    .highlight-blue {{ background: #bfdbfe; }}
    .highlight-violet {{ background: #ddd6fe; }}
    .highlight-pink {{ background: #fbcfe8; }}
    .highlight-brown {{ background: #d6cfc7; }}
    .highlight-grey {{ background: #e5e7eb; }}
  </mj-style>
</mj-head>
<mj-body background-color="{bg}">
  <mj-section padding="32px 24px 8px">
    <mj-column>
      <mj-text color="{text}" font-size="24px" font-weight="700" line-height="1.3" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif">
        {site_title}
      </mj-text>
    </mj-column>
  </mj-section>
  {body_sections}
</mj-body>
</mjml>"#,
        spoiler_css = spoiler_css,
        accent = colors.accent,
        code_bg = colors.code_bg,
        text_muted = colors.text_muted,
        border = colors.border,
        bg = colors.bg,
        text = colors.text,
        site_title = html_escape(options.site_title),
        body_sections = body_sections,
    )
}

// ============================================================================
// Spoiler transformation
// ============================================================================

/// Transform spoiler spans for email rendering.
///
/// In the web version, spoilers use JS click handlers to toggle visibility.
/// In email, we strip the `spoiler-hidden` class (which set `user-select: none`)
/// and rely on CSS `color: transparent` + `::selection` for reveal-on-highlight.
fn transform_spoilers_for_email(html: &str) -> String {
    html.replace("spoiler-hidden", "spoiler-mark")
        .replace("spoiler-revealed", "spoiler-mark")
}

// ============================================================================
// Fallback HTML (if MJML parsing/rendering fails)
// ============================================================================

fn build_fallback_html(
    entries: &[PublishedPage],
    options: &EmailDigestOptions<'_>,
    colors: &EmailColors,
) -> String {
    let mut body = String::new();

    // Title
    body.push_str(&format!(
        "<h1 style=\"font-size: 24px; margin: 0 0 16px;\">{}</h1>",
        html_escape(options.site_title)
    ));

    // Cover
    if let Some(cover) = options.cover_html {
        body.push_str(cover);
        body.push_str("<hr>");
    }

    // TOC
    if entries.len() > 1 {
        body.push_str("<p><strong>In this issue:</strong></p><ul>");
        for entry in entries {
            let anchor = slugify(&entry.title);
            body.push_str(&format!(
                "<li><a href=\"#{}\">{}</a></li>",
                html_escape(&anchor),
                html_escape(&entry.title)
            ));
        }
        body.push_str("</ul><hr>");
    }

    // Entries
    for entry in entries {
        let anchor = slugify(&entry.title);
        body.push_str(&format!(
            "<h2 id=\"{}\">{}</h2>",
            html_escape(&anchor),
            html_escape(&entry.title)
        ));
        body.push_str(&transform_spoilers_for_email(&entry.rendered_body));
        body.push_str("<hr>");
    }

    // Footer
    let mut footer = Vec::new();
    if let Some(base_url) = options.base_url {
        footer.push(format!(
            "<a href=\"{}\">View on web</a>",
            html_escape(base_url)
        ));
    }
    footer.push(format!(
        "<a href=\"{}\">Unsubscribe</a>",
        html_escape(options.unsubscribe_url)
    ));
    body.push_str(&format!(
        "<p style=\"text-align: center; font-size: 13px; color: {};\">{}</p>",
        html_escape(&colors.text_muted),
        footer.join(" &middot; ")
    ));

    format!(
        r#"<!DOCTYPE html>
<html>
<head><meta charset="UTF-8"><meta name="viewport" content="width=device-width, initial-scale=1.0"></head>
<body style="font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; max-width: 600px; margin: 0 auto; padding: 32px 16px; background: {bg}; color: {text}; line-height: 1.7;">
{body}
</body>
</html>"#,
        bg = html_escape(&colors.bg),
        text = html_escape(&colors.text),
        body = body,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publish::types::PublishedPage;
    use std::path::PathBuf;

    fn make_entry(title: &str, body: &str) -> PublishedPage {
        PublishedPage {
            source_path: PathBuf::from(format!("/workspace/{}.md", title.to_lowercase())),
            dest_filename: format!("{}.html", title.to_lowercase()),
            title: title.to_string(),
            rendered_body: body.to_string(),
            markdown_body: String::new(),
            contents_links: vec![],
            parent_link: None,
            is_root: false,
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

    fn default_options() -> EmailDigestOptions<'static> {
        EmailDigestOptions {
            cover_html: None,
            site_title: "Test Newsletter",
            base_url: Some("https://example.com"),
            unsubscribe_url: "https://example.com/unsubscribe?token={token}",
            theme: None,
        }
    }

    #[test]
    fn test_render_basic_digest() {
        let entries = vec![
            make_entry("First Post", "<p>Hello world</p>"),
            make_entry("Second Post", "<p>Goodbye world</p>"),
        ];
        let options = default_options();
        let html = render_email_digest(&entries, &options);

        assert!(html.contains("Test Newsletter"));
        assert!(html.contains("First Post"));
        assert!(html.contains("Second Post"));
        assert!(html.contains("Hello world"));
        assert!(html.contains("Goodbye world"));
        assert!(html.contains("Unsubscribe"));
        assert!(html.contains("View on web"));
    }

    #[test]
    fn test_render_with_cover() {
        let entries = vec![make_entry("Post", "<p>Content</p>")];
        let mut options = default_options();
        options.cover_html = Some("<p>Welcome to this week's edition!</p>");
        let html = render_email_digest(&entries, &options);

        assert!(html.contains("Welcome to this week"));
        assert!(html.contains("Post"));
    }

    #[test]
    fn test_render_no_site_url() {
        let entries = vec![make_entry("Post", "<p>Content</p>")];
        let mut options = default_options();
        options.base_url = None;
        let html = render_email_digest(&entries, &options);

        assert!(!html.contains("View on web"));
        assert!(html.contains("Unsubscribe"));
    }

    #[test]
    fn test_toc_only_with_multiple_entries() {
        // Single entry — no TOC
        let single = vec![make_entry("Solo", "<p>Just one</p>")];
        let options = default_options();
        let html = render_email_digest(&single, &options);
        assert!(!html.contains("In this issue"));

        // Multiple entries — has TOC
        let multiple = vec![
            make_entry("One", "<p>First</p>"),
            make_entry("Two", "<p>Second</p>"),
        ];
        let html = render_email_digest(&multiple, &options);
        assert!(html.contains("In this issue"));
    }

    #[test]
    fn test_spoilers_hidden_in_email() {
        let entries = vec![make_entry(
            "Spoiler Test",
            r#"<span data-spoiler="" class="spoiler-mark spoiler-hidden">secret text</span>"#,
        )];
        let options = default_options();
        let html = render_email_digest(&entries, &options);

        // Should not have the JS-toggle class
        assert!(!html.contains("spoiler-hidden"));
        // Should have email-friendly spoiler styling
        assert!(html.contains("color: transparent"));
    }

    #[test]
    fn test_no_navigation_or_js() {
        let entries = vec![make_entry("Post", "<p>Content</p>")];
        let options = default_options();
        let html = render_email_digest(&entries, &options);

        assert!(!html.contains("<nav"));
        assert!(!html.contains("site-nav"));
        assert!(!html.contains("<script"));
        assert!(!html.contains("metadata-pill"));
    }

    #[test]
    fn test_themed_email() {
        use crate::publish::types::{PublishColorPalette, PublishTheme};

        let theme = PublishTheme {
            id: Some("custom".into()),
            light: PublishColorPalette {
                bg: Some("#1a1a2e".into()),
                text: Some("#eaeaea".into()),
                accent: Some("#e94560".into()),
                ..Default::default()
            },
            dark: Default::default(),
            ..Default::default()
        };

        let entries = vec![make_entry("Themed", "<p>Styled content</p>")];
        let options = EmailDigestOptions {
            cover_html: None,
            site_title: "Themed Newsletter",
            base_url: None,
            unsubscribe_url: "https://example.com/unsub",
            theme: Some(&theme),
        };
        let html = render_email_digest(&entries, &options);

        assert!(html.contains("#1a1a2e"));
        assert!(html.contains("#e94560"));
    }
}
