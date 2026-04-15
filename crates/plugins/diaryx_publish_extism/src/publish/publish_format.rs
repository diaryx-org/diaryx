//! Format-agnostic publishing trait.
//!
//! Implement `PublishFormat` to support a new output format (HTML, EPUB, PDF, etc.).
//! The default implementation is `HtmlFormat` (behind the `html-publish` feature).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::types::{PublishOptions, PublishedPage, SiteNavigation};

/// Trait for format-specific publishing behavior.
///
/// `Publisher` delegates all format-specific operations to this trait:
/// body conversion, link transformation, page rendering, and static assets.
pub trait PublishFormat: Send + Sync {
    /// File extension for output files (e.g., "html").
    fn output_extension(&self) -> &str;

    /// Convert a workspace-relative markdown path to an output filename.
    ///
    /// Default: replaces `.md` extension with `output_extension()`,
    /// and sanitizes each path component to be URL-safe.
    fn output_filename(&self, path: &Path, workspace_dir: &Path) -> String {
        let relative = path.strip_prefix(workspace_dir).unwrap_or(path);
        let with_ext = relative.with_extension(self.output_extension());
        // Sanitize each path component to be URL-safe
        let sanitized: PathBuf = with_ext
            .components()
            .map(|c| match c {
                std::path::Component::Normal(s) => {
                    let s = s.to_string_lossy();
                    std::ffi::OsString::from(sanitize_path_component(&s))
                }
                other => other.as_os_str().to_owned(),
            })
            .collect();
        sanitized.to_string_lossy().into_owned()
    }

    /// Preprocess custom markdown syntax before body conversion.
    ///
    /// Called before `convert_body`. Override for format-specific syntax
    /// transformations (e.g., highlights, spoilers). Default: returns body unchanged.
    fn preprocess_body(&self, markdown: &str) -> String {
        markdown.to_string()
    }

    /// Convert markdown body to the output format.
    fn convert_body(&self, preprocessed_markdown: &str) -> String;

    /// Transform internal links in rendered output.
    ///
    /// Called after `convert_body`. Override to rewrite `.md` links to output
    /// format links. Default: returns rendered output unchanged.
    fn transform_links(
        &self,
        rendered: &str,
        _current_path: &Path,
        _path_to_filename: &HashMap<PathBuf, String>,
        _workspace_dir: &Path,
        _dest_filename: &str,
    ) -> String {
        rendered.to_string()
    }

    /// Wrap a rendered page into a complete document.
    fn render_page(&self, page: &PublishedPage, site_title: &str, single_file: bool) -> String;

    /// Render all pages into a single combined document.
    fn render_single_document(&self, pages: &[PublishedPage], site_title: &str) -> String;

    /// Render a page with full site context (nav, SEO, feeds).
    ///
    /// Default: delegates to `render_page()` ignoring new params.
    fn render_page_with_context(
        &self,
        page: &PublishedPage,
        site_title: &str,
        single_file: bool,
        _site_nav: &SiteNavigation,
        _seo_meta: &str,
        _feed_links: &str,
    ) -> String {
        self.render_page(page, site_title, single_file)
    }

    /// Render SEO meta tags for a page. Default: empty string.
    fn render_seo_meta(
        &self,
        _page: &PublishedPage,
        _site_title: &str,
        _options: &PublishOptions,
    ) -> String {
        String::new()
    }

    /// Render feed link tags for a page's `<head>`. Default: empty string.
    fn render_feed_links(&self, _page: &PublishedPage) -> String {
        String::new()
    }

    /// Generate supplementary files (sitemap, robots, feeds).
    ///
    /// Called after all pages are rendered. Returns `(filename, content)` pairs.
    fn supplementary_files(
        &self,
        _pages: &[PublishedPage],
        _options: &PublishOptions,
    ) -> Vec<(String, Vec<u8>)> {
        vec![]
    }

    /// Static assets to write alongside output files (e.g., CSS for HTML).
    ///
    /// Returns `(filename, content)` pairs. Default: no assets.
    fn static_assets(&self) -> Vec<(String, Vec<u8>)> {
        vec![]
    }
}

/// Sanitize a single path component for safe use in URLs.
///
/// Replaces spaces with `%20` and removes characters that are unsafe or
/// problematic in URLs (e.g., `!`, `?`, `#`, `&`, etc.). Preserves dots,
/// hyphens, underscores, and alphanumerics.
fn sanitize_path_component(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            ' ' => result.push_str("%20"),
            c if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' => result.push(c),
            _ => {} // drop unsafe characters like !, ?, #, &, etc.
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_path_component() {
        assert_eq!(sanitize_path_component("hello world"), "hello%20world");
        assert_eq!(sanitize_path_component("First post!"), "First%20post");
        assert_eq!(sanitize_path_component("what?"), "what");
        assert_eq!(sanitize_path_component("a&b#c"), "abc");
        assert_eq!(
            sanitize_path_component("normal-file_name.html"),
            "normal-file_name.html"
        );
    }
}
